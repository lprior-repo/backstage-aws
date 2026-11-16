//! AWS-boundary harness: real entrypoints, bounded mock calls, and retained S3 history.
use crate::ctx::Ctx;
use aws_sdk_ecs::{
    operation::{describe_tasks::DescribeTasksOutput, run_task::RunTaskOutput},
    types::{Container, Task},
};
use aws_sdk_s3::{
    operation::{
        get_object::{GetObjectError, GetObjectOutput},
        put_object::PutObjectOutput,
    },
    primitives::ByteStream,
    types::error::NoSuchKey,
};
use aws_smithy_mocks::{MockResponse, Rule, RuleMode, mock, mock_client};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fmt::Debug,
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

static NULL: Value = Value::Null;

/// Looks up a JSON pointer, yielding `null` for a missing path like `Value` indexing does.
pub fn at<'a>(value: &'a Value, pointer: &str) -> &'a Value {
    value.pointer(pointer).unwrap_or(&NULL)
}

/// Unwraps a harness invariant that the scenario itself guarantees.
pub fn present<T>(value: Option<T>, invariant: &str) -> T {
    let Some(value) = value else {
        unreachable!("harness invariant violated: {invariant}")
    };
    value
}

/// Unwraps a fallible harness step that the scenario itself guarantees.
pub fn succeeded<T, E: Debug>(result: Result<T, E>, step: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => unreachable!("harness step failed: {step}: {error:?}"),
    }
}

/// Locks shared harness state; a panicking sibling assertion must not mask the real failure.
pub fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
pub const BUCKET: &str = "portal-seed";
pub const SECRET: &str = "arn:aws:secretsmanager:us-east-1:123456789012:secret:portal/db";
pub const CLUSTER: &str = "portal-cluster";
pub const TASK: &str = "arn:aws:ecs:us-east-1:123456789012:task/portal-cluster/pinned";
pub const SERVICE: &str = "arn:aws:ecs:us-east-1:123456789012:service/portal-cluster/portal-pr-38";

#[derive(Default)]
pub struct Journal {
    pub record: Option<Value>,
    pub writes: Vec<Value>,
    pub events: Vec<String>,
    pub uploads: Vec<String>,
    pub tokens: Vec<String>,
    revision: usize,
}

impl Journal {
    pub fn record(&self) -> &Value {
        present(self.record.as_ref(), "a durable checkpoint exists")
    }

    pub fn event(&mut self, event: impl Into<String>) {
        assert!(
            self.events.len() < 128,
            "mock protocol exceeded its call budget"
        );
        self.events.push(event.into());
    }
}

pub type SharedJournal = Arc<Mutex<Journal>>;

pub struct Project {
    pub dir: tempfile::TempDir,
    pub ctx: Ctx,
    pub journal: SharedJournal,
    pub s3_rules: [Rule; 2],
}

fn base_state() -> Vec<u8> {
    let outputs = json!({
        "ecs_cluster_name": CLUSTER,
        "seed_bucket_name": BUCKET,
        "database_application_secret_arn": SECRET,
        "db_admin_seed_restore_task_definition_arn": "seed-restore:7",
        "db_admin_rotate_task_definition_arn": "rotate:9",
        "db_admin_network_configuration": {"awsvpcConfiguration": {
            "subnets": ["subnet-a"], "securityGroups": ["sg-admin"], "assignPublicIp": "DISABLED"
        }}
    });
    let wrapped = present(outputs.as_object(), "outputs are an object")
        .iter()
        .map(|(key, value)| (key.clone(), json!({"value": value})))
        .collect::<serde_json::Map<_, _>>();
    succeeded(
        serde_json::to_vec(&json!({"outputs": wrapped})),
        "fixture serializes",
    )
}

pub fn checkpoint_label(record: &Value) -> String {
    let state = at(record, "/state");
    if let Some(phase) = at(state, "/phase").as_str() {
        if phase != "role_change" {
            return phase.to_owned();
        }
    } else if at(state, "/status") == "completed" {
        return "completed".to_owned();
    }
    let status = present(at(state, "/attempt/status").as_str(), "attempt status");
    at(state, "/attempt/launch/launch")
        .as_str()
        .map_or_else(|| status.to_owned(), |launch| format!("{status}:{launch}"))
}

impl Project {
    pub fn new(
        lock_key: &'static str,
        record: Option<Value>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        let journal = Arc::new(Mutex::new(Journal {
            revision: usize::from(record.is_some()),
            record,
            ..Journal::default()
        }));
        let reads = journal.clone();
        let read = mock!(aws_sdk_s3::Client::get_object).then_compute_response(move |request| {
            let mut journal = locked(&reads);
            if request.key() == Some("base/terraform.tfstate") {
                assert_eq!(
                    request.bucket(),
                    Some("portal-123456789012-us-east-1-tfstate")
                );
                journal.event("base");
                return MockResponse::Output(
                    GetObjectOutput::builder()
                        .body(ByteStream::from(base_state()))
                        .build(),
                );
            }
            assert_eq!(request.bucket(), Some(BUCKET));
            assert_eq!(request.key(), Some(lock_key));
            journal.event("load");
            let stored = journal.record.as_ref().map(|record| {
                (
                    succeeded(serde_json::to_vec(record), "record serializes"),
                    journal.revision,
                )
            });
            drop(journal);
            stored.map_or_else(
                || MockResponse::Error(GetObjectError::NoSuchKey(NoSuchKey::builder().build())),
                |(body, revision)| {
                    MockResponse::Output(
                        GetObjectOutput::builder()
                            .body(ByteStream::from(body))
                            .version_id(format!("version-{revision}"))
                            .e_tag(format!("etag-{revision}"))
                            .build(),
                    )
                },
            )
        });
        let writes = journal.clone();
        let write = mock!(aws_sdk_s3::Client::put_object).then_compute_output(move |request| {
            let mut journal = locked(&writes);
            assert_eq!(request.bucket(), Some(BUCKET));
            let key = present(request.key(), "S3 write has a key");
            if key != lock_key {
                assert!(
                    key.starts_with("operations/seed/")
                        && Path::new(key).extension().is_some_and(|ext| ext == "dump")
                );
                assert_eq!(request.if_match(), None);
                assert_eq!(request.if_none_match(), None);
                journal.uploads.push(key.to_owned());
                journal.event("upload");
                return PutObjectOutput::builder()
                    .version_id("dump-version")
                    .e_tag("dump-etag")
                    .build();
            }
            if journal.record.is_some() {
                assert_eq!(
                    request.if_match(),
                    Some(format!("etag-{}", journal.revision).as_str())
                );
                assert_eq!(request.if_none_match(), None);
            } else {
                assert_eq!(request.if_none_match(), Some("*"));
                assert_eq!(request.if_match(), None);
            }
            let record: Value = succeeded(
                serde_json::from_slice(present(
                    request.body().bytes(),
                    "checkpoint body is in memory",
                )),
                "valid checkpoint JSON",
            );
            journal.event(format!("put:{}", checkpoint_label(&record)));
            journal.writes.push(record.clone());
            journal.record = Some(record);
            journal.revision = journal.revision.saturating_add(1);
            let revision = journal.revision;
            drop(journal);
            PutObjectOutput::builder()
                .version_id(format!("version-{revision}"))
                .e_tag(format!("etag-{revision}"))
                .build()
        });
        ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&read, &write]);
        Ok(Self {
            dir,
            ctx,
            journal,
            s3_rules: [read, write],
        })
    }

    pub fn tasks(
        &mut self,
        rotation: bool,
        exit_codes: impl IntoIterator<Item = i32>,
    ) -> [Rule; 2] {
        let launches = self.journal.clone();
        let started = mock!(aws_sdk_ecs::Client::run_task).then_compute_output(move |request| {
            let mut journal = locked(&launches);
            let record = journal.record();
            let attempt = at(record, "/state/attempt");
            assert_eq!(at(attempt, "/status"), "in_flight");
            assert_eq!(at(attempt, "/launch/launch"), "reserved");
            assert_eq!(request.client_token(), at(attempt, "/attempt_id").as_str());
            assert_ne!(request.client_token(), at(record, "/id").as_str());
            assert_eq!(request.cluster(), Some(CLUSTER));
            assert_eq!(
                request.task_definition(),
                Some(if rotation {
                    "rotate:9"
                } else {
                    "seed-restore:7"
                })
            );
            let overrides = present(request.overrides(), "task overrides").container_overrides();
            assert_eq!(overrides.len(), 2);
            for container in overrides {
                let expected = if rotation {
                    assert!(matches!(
                        container.name(),
                        Some("rotation-secret" | "db-admin")
                    ));
                    vec![(Some("ROTATION_ID"), at(record, "/id").as_str())]
                } else {
                    match container.name() {
                        Some("download") => vec![(
                            Some("SEED_OBJECT_KEY"),
                            at(record, "/state/object_key").as_str(),
                        )],
                        Some("restore") => vec![
                            (Some("SEED_OPERATION_ID"), at(record, "/id").as_str()),
                            (
                                Some("SEED_OBJECT_KEY"),
                                at(record, "/state/object_key").as_str(),
                            ),
                        ],
                        other => unreachable!("unexpected seed container {other:?}"),
                    }
                };
                assert_eq!(
                    container
                        .environment()
                        .iter()
                        .map(|pair| (pair.name(), pair.value()))
                        .collect::<Vec<_>>(),
                    expected
                );
            }
            journal
                .tokens
                .push(present(request.client_token(), "attempt token").to_owned());
            journal.event("run");
            let attempts = journal.tokens.len();
            drop(journal);
            let arn = format!(
                "arn:aws:ecs:us-east-1:123456789012:task/portal-cluster/attempt-{attempts}"
            );
            RunTaskOutput::builder()
                .tasks(Task::builder().task_arn(arn).build())
                .build()
        });
        let observations = self.journal.clone();
        let exits = Arc::new(Mutex::new(exit_codes.into_iter().collect::<VecDeque<_>>()));
        let stopped =
            mock!(aws_sdk_ecs::Client::describe_tasks).then_compute_output(move |request| {
                let mut journal = locked(&observations);
                let attempt = at(journal.record(), "/state/attempt");
                assert_eq!(at(attempt, "/status"), "in_flight");
                assert_eq!(at(attempt, "/launch/launch"), "submitted");
                let arn = present(
                    at(attempt, "/launch/task_arn").as_str(),
                    "persisted task ARN",
                )
                .to_owned();
                assert_eq!(request.cluster(), Some(CLUSTER));
                assert_eq!(request.tasks(), std::slice::from_ref(&arn));
                journal.event("observe");
                drop(journal);
                let code = present(locked(&exits).pop_front(), "bounded task observation");
                DescribeTasksOutput::builder()
                    .tasks(
                        Task::builder()
                            .task_arn(arn)
                            .last_status("STOPPED")
                            .containers(Container::builder().exit_code(code).build())
                            .build(),
                    )
                    .build()
            });
        self.ctx.ecs = mock_client!(aws_sdk_ecs, RuleMode::MatchAny, [&started, &stopped]);
        [started, stopped]
    }

    pub fn assert_consumed(&self, reads: usize, writes: usize) {
        assert_eq!(
            present(self.s3_rules.first(), "read rule").num_calls(),
            reads
        );
        assert_eq!(
            present(self.s3_rules.get(1), "write rule").num_calls(),
            writes
        );
    }
}
