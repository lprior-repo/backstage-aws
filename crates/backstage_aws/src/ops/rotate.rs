//! Rotates the application database password. The password exists only in
//! Secrets Manager, the task-scoped ephemeral volume, and process memory.
#[path = "rotation/state.rs"]
mod state;

use crate::{
    ctx::{Ctx, StepTiming, step},
    db::{self, Checkpoint, SecretVersionId, TaskAttempt, TaskAttemptId, TaskLaunch},
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use aws_sdk_s3::primitives::ByteStream;
use serde::Serialize;
use serde_json::Value;
use state::{RotationOperation, RotationState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_sdk_ecs::client::Waiters;

const ROTATION_LOCK_KEY: &str = "operations/rotation/active.json";

#[derive(Serialize)]
pub(crate) struct RotationResult {
    secret_arn: String,
    version_id: String,
    services_restarted: Vec<String>,
    timings: Vec<StepTiming>,
}

#[derive(Clone, Debug)]
struct RotationLock {
    checkpoint: Checkpoint<RotationOperation>,
}

/// Resumes or starts a rotation through adjacent, ETag-guarded checkpoints.
pub(crate) async fn rotate_password(ctx: &Ctx) -> Result<RotationResult> {
    ctx.require_approval()?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let secret_arn = output_str(&base, "database_application_secret_arn")?.to_owned();
    let bucket = output_str(&base, "seed_bucket_name")?.to_owned();
    let mut lock = acquire_rotation_lock(ctx, &bucket).await?;
    let mut timings = Vec::with_capacity(4);
    let cluster = output_str(&base, "ecs_cluster_name")?;

    for _ in 0..12 {
        let operation = &lock.checkpoint.record;
        match operation.state.clone() {
            RotationState::Prepared => {
                let previous = current_version(ctx, &secret_arn).await?;
                let state = operation.state.clone().pin_baseline(previous)?;
                lock = persist_rotation(ctx, &bucket, &lock, state).await?;
            }
            RotationState::BaselinePinned {
                previous_version_id,
            } => {
                let replacement = operation.replacement_version_id()?;
                let ((), timing) = step(
                    "secret.stage",
                    stage_new_password(ctx, &secret_arn, &replacement, &previous_version_id),
                )
                .await?;
                timings.push(timing);
                let state = operation.state.clone().stage_secret()?;
                lock = persist_rotation(ctx, &bucket, &lock, state).await?;
            }
            RotationState::SecretStaged { .. } => {
                let state = operation.state.clone().begin_role_change()?;
                lock = persist_rotation(ctx, &bucket, &lock, state).await?;
            }
            RotationState::RoleChange { .. } => {
                // The SDK task/waiter state makes this future >20 KiB; keep it
                // off the parent state machine without spawning or detaching it.
                let (lock_after_task, timing) = Box::pin(step(
                    "database.alter-role",
                    run_role_change(ctx, &base, &bucket, &lock),
                ))
                .await?;
                timings.push(timing);
                lock = lock_after_task;
                let RotationState::RoleChange { .. } = lock.checkpoint.record.state else {
                    continue;
                };
                lock = promote_rotation(ctx, &secret_arn, &bucket, &lock).await?;
            }
            RotationState::SecretPromoted {
                previous_version_id: _,
                task_arn: _,
                promoted_at_epoch_ms: _,
            } => {
                let (services, timing) =
                    Box::pin(step("services.restart", restart_services(ctx, cluster))).await?;
                timings.push(timing);
                let state = lock
                    .checkpoint
                    .record
                    .state
                    .clone()
                    .restart_services(services)?;
                lock = persist_rotation(ctx, &bucket, &lock, state).await?;
            }
            RotationState::ServicesRestarted { .. } => {
                let state = operation.state.clone().complete()?;
                lock = persist_rotation(ctx, &bucket, &lock, state).await?;
            }
            RotationState::Completed {
                services_restarted, ..
            } => {
                return Ok(RotationResult {
                    secret_arn,
                    version_id: lock.checkpoint.record.id.as_str(),
                    services_restarted,
                    timings,
                });
            }
        }
    }
    Err(Error::precondition_failed(
        "rotation advanced concurrently; rerun to resume the latest checkpoint",
    ))
}

async fn promote_rotation(
    ctx: &Ctx,
    secret_arn: &str,
    bucket: &str,
    lock: &RotationLock,
) -> Result<RotationLock> {
    let replacement = lock.checkpoint.record.replacement_version_id()?;
    let previous = lock
        .checkpoint
        .record
        .state
        .previous_version_id()
        .ok_or_else(|| Error::state_failed("rotation baseline is missing"))?;
    promote_version(ctx, secret_arn, &replacement, previous).await?;
    let promoted_at_epoch_ms: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::state_failed("system clock is before the UNIX epoch"))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::state_failed("rotation timestamp is out of range"))?;
    let state = lock
        .checkpoint
        .record
        .state
        .clone()
        .promote_secret(promoted_at_epoch_ms)?;
    persist_rotation(ctx, bucket, lock, state).await
}

async fn current_version(ctx: &Ctx, secret_arn: &str) -> Result<SecretVersionId> {
    let secret = ctx
        .secrets
        .get_secret_value()
        .secret_id(secret_arn)
        .version_stage("AWSCURRENT")
        .send()
        .await
        .map_err(|_| {
            Error::aws_failed(
                "secretsmanager",
                "GetSecretValue",
                "current version unavailable",
            )
        })?;
    let version = secret.version_id().ok_or_else(|| {
        Error::aws_failed(
            "secretsmanager",
            "GetSecretValue",
            "current version has no version id",
        )
    })?;
    SecretVersionId::parse(version.to_owned())
}

async fn acquire_rotation_lock(ctx: &Ctx, bucket: &str) -> Result<RotationLock> {
    let previous = load_rotation_lock(ctx, bucket).await?;
    if let Some(lock) = &previous
        && !lock.checkpoint.record.state.is_completed()
    {
        return Ok(lock.clone());
    }
    let operation = RotationOperation::new();
    match put_rotation_operation(ctx, bucket, &operation, previous.as_ref()).await {
        Ok(lock) => Ok(lock),
        Err(error) => match load_rotation_lock(ctx, bucket).await? {
            Some(lock) if !lock.checkpoint.record.state.is_completed() => Ok(lock),
            _ => Err(error),
        },
    }
}

async fn put_rotation_operation(
    ctx: &Ctx,
    bucket: &str,
    operation: &RotationOperation,
    previous: Option<&RotationLock>,
) -> Result<RotationLock> {
    let bytes = serde_json::to_vec(operation)
        .map_err(|error| Error::state_failed(format!("rotation state: {error}")))?;
    let mut request = ctx
        .s3
        .put_object()
        .bucket(bucket)
        .key(ROTATION_LOCK_KEY)
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
    Ok(RotationLock { checkpoint })
}

async fn load_rotation_lock(ctx: &Ctx, bucket: &str) -> Result<Option<RotationLock>> {
    let object = match ctx
        .s3
        .get_object()
        .bucket(bucket)
        .key(ROTATION_LOCK_KEY)
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
    let bytes = db::read_checkpoint_body(object.body).await?;
    let operation = serde_json::from_slice::<RotationOperation>(&bytes)
        .map_err(|error| Error::state_failed(format!("rotation operation: {error}")))?;
    let operation = operation.validate()?;
    let checkpoint = Checkpoint::from_s3(operation, version_id.as_deref(), etag.as_deref())?;
    Ok(Some(RotationLock { checkpoint }))
}

async fn persist_rotation(
    ctx: &Ctx,
    bucket: &str,
    previous: &RotationLock,
    state: RotationState,
) -> Result<RotationLock> {
    let mut operation = previous.checkpoint.record.clone();
    operation.state = state;
    match put_rotation_operation(ctx, bucket, &operation, Some(previous)).await {
        Ok(lock) => Ok(lock),
        Err(error) => match load_rotation_lock(ctx, bucket).await? {
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

/// Only the checkpoint present at invocation entry may authorize a fresh retry.
async fn reserve_role_change(
    ctx: &Ctx,
    bucket: &str,
    initial: &RotationLock,
) -> Result<RotationLock> {
    let RotationState::RoleChange { attempt, .. } = &initial.checkpoint.record.state else {
        return Ok(initial.clone());
    };
    if !matches!(attempt, TaskAttempt::Ready | TaskAttempt::Failed { .. }) {
        return Ok(initial.clone());
    }
    let next = attempt.for_explicit_invocation(TaskAttemptId::new_distinct_from(
        initial.checkpoint.record.id,
    ));
    let state = initial
        .checkpoint
        .record
        .state
        .clone()
        .with_task_attempt(next, initial.checkpoint.record.id)?;
    persist_rotation(ctx, bucket, initial, state).await
}

async fn run_role_change(
    ctx: &Ctx,
    base: &Value,
    bucket: &str,
    initial: &RotationLock,
) -> Result<RotationLock> {
    let mut lock = reserve_role_change(ctx, bucket, initial).await?;
    for _ in 0..4 {
        let RotationState::RoleChange { attempt, .. } = &lock.checkpoint.record.state else {
            return Ok(lock);
        };
        match attempt {
            TaskAttempt::InFlight {
                attempt_id,
                launch: TaskLaunch::Reserved,
            } => {
                let task_arn = db::start_rotation_task(
                    ctx,
                    base,
                    "db_admin_rotate_task_definition_arn",
                    lock.checkpoint.record.id,
                    *attempt_id,
                )
                .await?;
                let next = attempt.clone().submitted(task_arn)?;
                let state = lock
                    .checkpoint
                    .record
                    .state
                    .clone()
                    .with_task_attempt(next, lock.checkpoint.record.id)?;
                lock = persist_rotation(ctx, bucket, &lock, state).await?;
            }
            TaskAttempt::InFlight {
                launch: TaskLaunch::Submitted { task_arn },
                ..
            } => {
                let terminal = db::observe_tracked_task(ctx, base, task_arn).await?;
                let next = attempt.clone().observed_terminal(terminal)?;
                let failed = next.is_failed();
                let state = lock
                    .checkpoint
                    .record
                    .state
                    .clone()
                    .with_task_attempt(next, lock.checkpoint.record.id)?;
                lock = persist_rotation(ctx, bucket, &lock, state).await?;
                if failed {
                    return Err(Error::db_task_failed(
                        "rotation task failed; explicitly rerun to retry",
                    ));
                }
            }
            TaskAttempt::Succeeded { .. } => return Ok(lock),
            TaskAttempt::Failed { .. } => {
                return Err(Error::db_task_failed(
                    "rotation task failed; explicitly rerun to retry",
                ));
            }
            TaskAttempt::Ready => {
                return Err(Error::state_failed("rotation task attempt did not advance"));
            }
        }
    }
    if matches!(
        lock.checkpoint.record.state,
        RotationState::RoleChange {
            attempt: TaskAttempt::Succeeded { .. },
            ..
        }
    ) {
        Ok(lock)
    } else {
        Err(Error::precondition_failed(
            "rotation task checkpoint changed concurrently; rerun to resume",
        ))
    }
}

async fn promote_version(
    ctx: &Ctx,
    secret_arn: &str,
    replacement: &SecretVersionId,
    previous: &SecretVersionId,
) -> Result<()> {
    let current = current_version(ctx, secret_arn).await?;
    if current == *replacement {
        return Ok(());
    }
    if current != *previous {
        return Err(Error::precondition_failed(
            "AWSCURRENT no longer matches the rotation's pinned predecessor",
        ));
    }
    ctx.secrets
        .update_secret_version_stage()
        .secret_id(secret_arn)
        .version_stage("AWSCURRENT")
        .move_to_version_id(replacement.as_str())
        .remove_from_version_id(previous.as_str())
        .send()
        .await
        .map_err(|_| {
            Error::aws_failed(
                "secretsmanager",
                "UpdateSecretVersionStage",
                "the pinned version could not be promoted",
            )
        })?;
    Ok(())
}

fn is_valid_rotation_password(password: &str) -> bool {
    password.len() == 40 && password.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// A pinned replacement is reused only after verifying its version and token.
async fn replacement_is_staged(
    ctx: &Ctx,
    secret_arn: &str,
    version_id: &SecretVersionId,
) -> Result<bool> {
    let pending = ctx
        .secrets
        .get_secret_value()
        .secret_id(secret_arn)
        .version_id(version_id.as_str())
        .send()
        .await;
    match pending {
        Ok(secret) if secret.version_id() == Some(version_id.as_str()) => {
            let has_valid_password = secret
                .secret_string()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .and_then(|document| {
                    document
                        .get("password")
                        .and_then(Value::as_str)
                        .map(is_valid_rotation_password)
                })
                .is_some_and(|valid| valid);
            if has_valid_password {
                return Ok(true);
            }
            return Err(Error::state_failed(
                "the pinned replacement secret has an invalid password token",
            ));
        }
        Ok(_) => {
            return Err(Error::state_failed(
                "Secrets Manager returned a different pinned replacement version",
            ));
        }
        Err(error)
            if error
                .as_service_error()
                .is_some_and(
                    aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError::is_resource_not_found_exception,
                ) => {}
        Err(_) => {
            return Err(Error::aws_failed(
                "secretsmanager",
                "GetSecretValue",
                "the pinned version could not be read",
            ));
        }
    }
    Ok(false)
}

async fn stage_new_password(
    ctx: &Ctx,
    secret_arn: &str,
    version_id: &SecretVersionId,
    previous_version_id: &SecretVersionId,
) -> Result<()> {
    if replacement_is_staged(ctx, secret_arn, version_id).await? {
        return Ok(());
    }
    let previous = ctx
        .secrets
        .get_secret_value()
        .secret_id(secret_arn)
        .version_id(previous_version_id.as_str())
        .send()
        .await
        .map_err(|_| {
            Error::aws_failed(
                "secretsmanager",
                "GetSecretValue",
                "the pinned predecessor secret could not be read",
            )
        })?;
    if previous.version_id() != Some(previous_version_id.as_str()) {
        return Err(Error::state_failed(
            "Secrets Manager returned a different pinned predecessor version",
        ));
    }
    let mut document: Value = previous
        .secret_string()
        .and_then(|text| serde_json::from_str(text).ok())
        .filter(Value::is_object)
        .ok_or_else(|| {
            Error::aws_failed(
                "secretsmanager",
                "GetSecretValue",
                "the secret is not a JSON object",
            )
        })?;
    let generated = ctx
        .secrets
        .get_random_password()
        .password_length(40)
        .exclude_punctuation(true)
        .send()
        .await
        .map_err(|_| {
            Error::aws_failed(
                "secretsmanager",
                "GetRandomPassword",
                "password generation failed",
            )
        })?;
    let password = generated.random_password().ok_or_else(|| {
        Error::aws_failed(
            "secretsmanager",
            "GetRandomPassword",
            "no password returned",
        )
    })?;
    if !is_valid_rotation_password(password) {
        return Err(Error::state_failed(
            "Secrets Manager returned a password outside the rotation token contract",
        ));
    }
    let Some(fields) = document.as_object_mut() else {
        return Err(Error::state_failed("the secret is not a JSON object"));
    };
    fields.insert("password".to_owned(), Value::from(password));
    let stored = ctx
        .secrets
        .put_secret_value()
        .secret_id(secret_arn)
        .secret_string(document.to_string())
        .client_request_token(version_id.as_str())
        .version_stages("AWSPENDING")
        .send()
        .await
        .map_err(|_| {
            Error::aws_failed(
                "secretsmanager",
                "PutSecretValue",
                "the new password was not stored",
            )
        })?;
    if stored.version_id() != Some(version_id.as_str()) {
        return Err(Error::state_failed(
            "Secrets Manager did not preserve the rotation version token",
        ));
    }
    Ok(())
}

/// Starts a new deployment of every service in the cluster, then waits for all
/// of them, so each task picks up the new secret value.
async fn restart_services(ctx: &Ctx, cluster: &str) -> Result<Vec<String>> {
    let listed = ctx
        .ecs
        .list_services()
        .cluster(cluster)
        .max_results(100)
        .send()
        .await
        .map_err(|error| Error::aws_failed("ecs", "ListServices", error.to_string()))?;
    if listed.next_token().is_some() {
        return Err(Error::precondition_failed(
            "the cluster has more than 100 services; restart them in batches by hand",
        ));
    }
    let services = listed.service_arns();
    for service in services {
        ctx.ecs
            .update_service()
            .cluster(cluster)
            .service(service)
            .force_new_deployment(true)
            .send()
            .await
            .map_err(|error| Error::aws_failed("ecs", "UpdateService", error.to_string()))?;
    }
    // DescribeServices, which the waiter polls, accepts ten services a call.
    for batch in services.chunks(10) {
        ctx.ecs
            .wait_until_services_stable()
            .cluster(cluster)
            .set_services(Some(batch.to_vec()))
            .wait(Duration::from_mins(30))
            .await
            .map_err(|error| Error::waiter("service restart", cluster, error))?;
    }
    Ok(services.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_secretsmanager::{Client, operation::get_secret_value::GetSecretValueOutput};
    use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};

    const SECRET: &str = "arn:aws:secretsmanager:us-east-1:123456789012:secret:portal/db";
    const OPERATION_VERSION: &str = "11111111-1111-4111-8111-111111111111";
    const BASELINE_VERSION: &str = "22222222-2222-4222-8222-222222222222";

    fn already_staged(version_id: &'static str, document: &'static str) -> Rule {
        mock!(Client::get_secret_value)
            .match_requests(|request| request.version_id() == Some(OPERATION_VERSION))
            .then_output(move || {
                GetSecretValueOutput::builder()
                    .secret_string(document)
                    .version_id(version_id)
                    .build()
            })
    }

    async fn stage(rules: &[&Rule]) -> std::result::Result<(), String> {
        let dir = std::env::temp_dir();
        let mut ctx = Ctx::sample(&dir, true).map_err(|error| error.to_string())?;
        ctx.secrets = mock_client!(aws_sdk_secretsmanager, RuleMode::MatchAny, rules);
        let replacement = SecretVersionId::parse(OPERATION_VERSION.to_owned())
            .map_err(|error| error.to_string())?;
        let previous = SecretVersionId::parse(BASELINE_VERSION.to_owned())
            .map_err(|error| error.to_string())?;
        tokio::time::timeout(
            Duration::from_secs(5),
            stage_new_password(&ctx, SECRET, &replacement, &previous),
        )
        .await
        .map_err(|_| "staging mock exceeded its deadline".to_owned())?
        .map_err(|error| error.to_string())
    }

    #[tokio::test]
    async fn an_existing_pinned_secret_version_is_reused_without_regeneration() {
        let staged = already_staged(
            OPERATION_VERSION,
            "{\"password\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"}",
        );
        let result = stage(&[&staged]).await;
        assert_eq!(result, Ok(()));
        assert_eq!(staged.num_calls(), 1);
    }

    #[tokio::test]
    async fn a_different_returned_version_is_rejected() {
        let staged = already_staged(
            BASELINE_VERSION,
            "{\"password\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"}",
        );
        let result = stage(&[&staged]).await;
        assert_eq!(
            result,
            Err("Secrets Manager returned a different pinned replacement version".to_owned())
        );
    }
    #[tokio::test]
    async fn malformed_pinned_password_is_not_reused() {
        let staged = already_staged(OPERATION_VERSION, "{\"password\":\"short\"}");
        let result = stage(&[&staged]).await;
        assert_eq!(
            result,
            Err("the pinned replacement secret has an invalid password token".to_owned())
        );
    }
}

#[cfg(test)]
#[path = "rotation/behavior_tests.rs"]
mod behavior_tests;
