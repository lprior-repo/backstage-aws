use super::{
    Checkpoint, OperationId, SeedDumpKey, TaskArn, TaskAttempt, TaskAttemptId, TaskLaunch,
    read_checkpoint_body, seed_recovery::observe_seed_task, start_tracked_task,
};
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecs::types::{ContainerOverride, KeyValuePair};
use aws_sdk_s3::primitives::ByteStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const SEED_LOCK_KEY: &str = "operations/seed/active.json";
const SEED_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SeedOperation {
    schema_version: u32,
    id: OperationId,
    state: SeedState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum SeedState {
    Active {
        object_key: SeedDumpKey,
        attempt: TaskAttempt,
    },
    Completed {
        object_key: SeedDumpKey,
        task_arn: TaskArn,
    },
}

impl SeedOperation {
    fn new(id: OperationId) -> Self {
        Self {
            schema_version: SEED_SCHEMA_VERSION,
            id,
            state: SeedState::Active {
                object_key: SeedDumpKey::for_operation(id),
                attempt: TaskAttempt::Ready,
            },
        }
    }

    fn validate(self) -> Result<Self> {
        if self.schema_version != SEED_SCHEMA_VERSION {
            return Err(Error::state_failed("unsupported seed operation schema"));
        }
        let key = match &self.state {
            SeedState::Active { object_key, .. } | SeedState::Completed { object_key, .. } => {
                object_key
            }
        };
        SeedDumpKey::parse(key.as_str().to_owned(), self.id)?;
        if let SeedState::Active { attempt, .. } = &self.state
            && attempt
                .task_attempt_id()
                .is_some_and(|attempt_id| attempt_id.as_str() == self.id.as_str())
        {
            return Err(Error::state_failed(
                "seed task attempt ID cannot equal its operation ID",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug)]
struct SeedLock {
    checkpoint: Checkpoint<SeedOperation>,
}

#[derive(Serialize)]
pub struct SeedResult {
    bucket: String,
    key: String,
    operation_id: String,
    resumed: bool,
}

/// Resumes an active restore rather than replacing its pinned input with a new dump.
pub async fn seed(ctx: &Ctx, file: &Path) -> Result<SeedResult> {
    ctx.require_approval()?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let bucket = output_str(&base, "seed_bucket_name")?.to_owned();
    let (lock, resumed) = match load_seed_lock(ctx, &bucket).await? {
        Some(
            lock @ SeedLock {
                checkpoint:
                    Checkpoint {
                        record:
                            SeedOperation {
                                state: SeedState::Active { .. },
                                ..
                            },
                        ..
                    },
            },
        ) => (lock, true),
        previous => {
            let operation = upload_operation(ctx, &bucket, file).await?;
            match put_seed_operation(ctx, &bucket, &operation, previous.as_ref()).await {
                Ok(lock) => (lock, false),
                Err(error) => match load_seed_lock(ctx, &bucket).await? {
                    Some(
                        lock @ SeedLock {
                            checkpoint:
                                Checkpoint {
                                    record:
                                        SeedOperation {
                                            state: SeedState::Active { .. },
                                            ..
                                        },
                                    ..
                                },
                        },
                    ) => (lock, true),
                    _ => return Err(error),
                },
            }
        }
    };
    let lock = restore_seed(ctx, &base, &bucket, lock).await?;
    let SeedState::Completed { object_key, .. } = lock.checkpoint.record.state else {
        return Err(Error::state_failed("seed restore did not reach completion"));
    };
    Ok(SeedResult {
        bucket,
        key: object_key.as_str().to_owned(),
        operation_id: lock.checkpoint.record.id.as_str(),
        resumed,
    })
}

async fn upload_operation(ctx: &Ctx, bucket: &str, file: &Path) -> Result<SeedOperation> {
    let id = OperationId::new();
    let operation = SeedOperation::new(id);
    let SeedState::Active { object_key, .. } = &operation.state else {
        return Err(Error::state_failed("new seed operation is not active"));
    };
    let body = ByteStream::from_path(file)
        .await
        .map_err(|error| Error::invalid_input(format!("{}: {error}", file.display())))?;
    ctx.s3
        .put_object()
        .bucket(bucket)
        .key(object_key.as_str())
        .body(body)
        .send()
        .await
        .map_err(|error| Error::aws_failed("s3", "PutObject", error.to_string()))?;
    Ok(operation)
}

async fn put_seed_operation(
    ctx: &Ctx,
    bucket: &str,
    operation: &SeedOperation,
    previous: Option<&SeedLock>,
) -> Result<SeedLock> {
    let bytes = serde_json::to_vec(operation)
        .map_err(|error| Error::state_failed(format!("seed operation: {error}")))?;
    let mut request = ctx
        .s3
        .put_object()
        .bucket(bucket)
        .key(SEED_LOCK_KEY)
        .body(ByteStream::from(bytes));
    request = if let Some(previous) = previous {
        request.if_match(&previous.checkpoint.etag)
    } else {
        request.if_none_match("*")
    };
    let output = request
        .send()
        .await
        .map_err(|error| Error::aws_failed("s3", "PutObject", error.to_string()))?;
    let checkpoint = Checkpoint::from_s3(operation.clone(), output.version_id(), output.e_tag())?;
    Ok(SeedLock { checkpoint })
}

async fn load_seed_lock(ctx: &Ctx, bucket: &str) -> Result<Option<SeedLock>> {
    let object = match ctx
        .s3
        .get_object()
        .bucket(bucket)
        .key(SEED_LOCK_KEY)
        .send()
        .await
    {
        Ok(object) => object,
        Err(error)
            if error
                .as_service_error()
                .is_some_and(aws_sdk_s3::operation::get_object::GetObjectError::is_no_such_key) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(Error::aws_failed("s3", "GetObject", error.to_string())),
    };
    let version_id = object.version_id().map(str::to_owned);
    let etag = object.e_tag().map(str::to_owned);
    let bytes = read_checkpoint_body(object.body).await?;
    let operation = serde_json::from_slice::<SeedOperation>(&bytes)
        .map_err(|error| Error::state_failed(format!("seed operation: {error}")))?;
    let operation = operation.validate()?;
    let checkpoint = Checkpoint::from_s3(operation, version_id.as_deref(), etag.as_deref())?;
    Ok(Some(SeedLock { checkpoint }))
}

async fn persist_seed(
    ctx: &Ctx,
    bucket: &str,
    previous: &SeedLock,
    operation: &SeedOperation,
) -> Result<SeedLock> {
    match put_seed_operation(ctx, bucket, operation, Some(previous)).await {
        Ok(lock) => Ok(lock),
        Err(error) => match load_seed_lock(ctx, bucket).await? {
            Some(current)
                if current.checkpoint.etag != previous.checkpoint.etag
                    && current.checkpoint.record.id == previous.checkpoint.record.id =>
            {
                Ok(current)
            }
            _ => Err(error),
        },
    }
}

/// Reserve at most once, using only the attempt present at invocation entry.
async fn reserve_seed_attempt(ctx: &Ctx, bucket: &str, mut lock: SeedLock) -> Result<SeedLock> {
    if matches!(lock.checkpoint.record.state, SeedState::Completed { .. }) {
        return Ok(lock);
    }
    let attempt = match &lock.checkpoint.record.state {
        SeedState::Active { attempt, .. } => attempt,
        SeedState::Completed { .. } => return Ok(lock),
    };
    if matches!(attempt, TaskAttempt::Ready | TaskAttempt::Failed { .. }) {
        let attempt = attempt
            .for_explicit_invocation(TaskAttemptId::new_distinct_from(lock.checkpoint.record.id));
        let mut operation = lock.checkpoint.record.clone();
        let SeedState::Active {
            attempt: current, ..
        } = &mut operation.state
        else {
            return Err(Error::state_failed("seed operation changed state"));
        };
        *current = attempt;
        lock = persist_seed(ctx, bucket, &lock, &operation).await?;
    }
    Ok(lock)
}

async fn restore_seed(ctx: &Ctx, base: &Value, bucket: &str, lock: SeedLock) -> Result<SeedLock> {
    let mut lock = reserve_seed_attempt(ctx, bucket, lock).await?;
    if matches!(lock.checkpoint.record.state, SeedState::Completed { .. }) {
        return Ok(lock);
    }
    let (attempt_id, launch) = match &lock.checkpoint.record.state {
        SeedState::Active {
            attempt: TaskAttempt::InFlight { attempt_id, launch },
            ..
        } => (*attempt_id, launch.clone()),
        SeedState::Active {
            attempt: TaskAttempt::Succeeded { .. },
            ..
        } => return complete_seed(ctx, bucket, &lock).await,
        SeedState::Active {
            attempt: TaskAttempt::Failed { .. },
            ..
        } => {
            return Err(Error::db_task_failed(
                "seed task failed; explicitly rerun to retry",
            ));
        }
        SeedState::Active {
            attempt: TaskAttempt::Ready,
            ..
        } => return Err(Error::state_failed("seed attempt was not reserved")),
        SeedState::Completed { .. } => return Ok(lock),
    };
    let launch = match launch {
        TaskLaunch::Reserved => {
            let overrides = seed_overrides(&lock.checkpoint.record)?;
            let task_arn = start_tracked_task(
                ctx,
                base,
                "db_admin_seed_restore_task_definition_arn",
                overrides,
                attempt_id,
            )
            .await?;
            let mut operation = lock.checkpoint.record.clone();
            let SeedState::Active { attempt, .. } = &mut operation.state else {
                return Err(Error::state_failed("seed operation changed state"));
            };
            *attempt = attempt.clone().submitted(task_arn)?;
            lock = persist_seed(ctx, bucket, &lock, &operation).await?;
            match &lock.checkpoint.record.state {
                SeedState::Active {
                    attempt:
                        TaskAttempt::InFlight {
                            attempt_id: current_id,
                            launch,
                        },
                    ..
                } if *current_id == attempt_id => launch.clone(),
                SeedState::Active {
                    attempt: TaskAttempt::Succeeded { .. },
                    ..
                }
                | SeedState::Completed { .. } => return complete_seed(ctx, bucket, &lock).await,
                SeedState::Active { .. } => {
                    return Err(Error::precondition_failed(
                        "seed attempt changed during launch",
                    ));
                }
            }
        }
        submitted @ TaskLaunch::Submitted { .. } => submitted,
    };
    let TaskLaunch::Submitted { task_arn } = launch else {
        return Err(Error::state_failed("seed task ARN is missing after launch"));
    };
    let SeedState::Active { object_key, .. } = &lock.checkpoint.record.state else {
        return Err(Error::state_failed("seed operation changed state"));
    };
    let terminal =
        observe_seed_task(ctx, base, &task_arn, lock.checkpoint.record.id, object_key).await?;
    let mut operation = lock.checkpoint.record.clone();
    let SeedState::Active { attempt, .. } = &mut operation.state else {
        return Err(Error::state_failed("seed operation changed state"));
    };
    *attempt = attempt.clone().observed_terminal(terminal)?;
    lock = persist_seed(ctx, bucket, &lock, &operation).await?;
    match &lock.checkpoint.record.state {
        SeedState::Active {
            attempt: TaskAttempt::Succeeded { .. },
            ..
        }
        | SeedState::Completed { .. } => complete_seed(ctx, bucket, &lock).await,
        SeedState::Active {
            attempt: TaskAttempt::Failed { .. },
            ..
        } => Err(Error::db_task_failed(
            "seed task failed; explicitly rerun to retry",
        )),
        SeedState::Active { .. } => Err(Error::precondition_failed(
            "seed attempt changed during observation",
        )),
    }
}

fn seed_overrides(operation: &SeedOperation) -> Result<Vec<ContainerOverride>> {
    let SeedState::Active { object_key, .. } = &operation.state else {
        return Err(Error::state_failed(
            "seed task requires an active operation",
        ));
    };
    let env = |name: &str, value: &str| KeyValuePair::builder().name(name).value(value).build();
    Ok(vec![
        ContainerOverride::builder()
            .name("download")
            .environment(env("SEED_OBJECT_KEY", object_key.as_str()))
            .build(),
        ContainerOverride::builder()
            .name("restore")
            .environment(env("SEED_OPERATION_ID", &operation.id.as_str()))
            .environment(env("SEED_OBJECT_KEY", object_key.as_str()))
            .build(),
    ])
}

async fn complete_seed(ctx: &Ctx, bucket: &str, lock: &SeedLock) -> Result<SeedLock> {
    match &lock.checkpoint.record.state {
        SeedState::Completed { .. } => Ok(lock.clone()),
        SeedState::Active {
            attempt: TaskAttempt::Succeeded { task_arn, .. },
            object_key,
        } => {
            let mut operation = lock.checkpoint.record.clone();
            operation.state = SeedState::Completed {
                object_key: object_key.clone(),
                task_arn: task_arn.clone(),
            };
            persist_seed(ctx, bucket, lock, &operation).await
        }
        SeedState::Active { .. } => Err(Error::precondition_failed(
            "seed completion requires an observed successful task",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_operation_is_bound_to_its_unique_dump_key() {
        let operation = SeedOperation::new(OperationId::new());
        let expected_key = SeedDumpKey::for_operation(operation.id);
        let parsed = SeedDumpKey::parse(expected_key.as_str().to_owned(), operation.id);
        assert!(parsed.is_ok());
        assert_eq!(parsed.ok(), Some(expected_key));
        assert!(operation.validate().is_ok());
    }

    #[test]
    fn seed_operation_rejects_a_dump_key_from_another_operation() {
        let operation = SeedOperation::new(OperationId::new());
        let other_id = OperationId::new();
        let mismatched = SeedOperation {
            schema_version: SEED_SCHEMA_VERSION,
            id: operation.id,
            state: SeedState::Active {
                object_key: SeedDumpKey::for_operation(other_id),
                attempt: TaskAttempt::Ready,
            },
        };
        assert!(mismatched.validate().is_err());
    }
}

#[cfg(test)]
#[path = "seed_tests.rs"]
mod behavior_tests;
