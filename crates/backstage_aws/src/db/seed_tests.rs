use super::*;
use crate::db::test_support::{
    BUCKET, Project, TASK, TestResult, at, checkpoint_label, locked, present, succeeded,
};
use aws_sdk_ecs::{
    operation::{describe_tasks::DescribeTasksOutput, run_task::RunTaskOutput},
    types::{Container, Failure, Task},
};
use aws_smithy_mocks::{RuleMode, mock, mock_client};
use serde_json::json;
use std::time::Duration;

const ID: &str = "11111111-1111-4111-8111-111111111111";
const ATTEMPT: &str = "22222222-2222-4222-8222-222222222222";
const KEY: &str = "operations/seed/11111111-1111-4111-8111-111111111111.dump";

fn active(launch: &Value) -> Value {
    json!({"schema_version": 1, "id": ID, "state": {
        "status": "active", "object_key": KEY,
        "attempt": {"status": "in_flight", "attempt_id": ATTEMPT, "launch": launch}
    }})
}

async fn invoke(project: &Project, file: &Path) -> Result<SeedResult> {
    succeeded(
        tokio::time::timeout(Duration::from_secs(5), seed(&project.ctx, file)).await,
        "seed entrypoint is bounded",
    )
}

#[tokio::test]
async fn seed_uploads_then_reserves_launches_observes_and_durably_completes() -> TestResult {
    // Given a new real dump file and no active checkpoint.
    let mut project = Project::new(SEED_LOCK_KEY, None)?;
    let file = project.dir.path().join("new.dump");
    std::fs::write(&file, b"PGDMP-real-local-input")?;
    let tasks = project.tasks(false, [0]);
    // When the actual approved seed entrypoint runs.
    let result = invoke(&project, &file).await?;
    // Then every checkpoint precedes its effect and completion remains in S3.
    let journal = locked(&project.journal);
    assert_eq!(
        journal.events,
        [
            "base",
            "load",
            "upload",
            "put:ready",
            "put:in_flight:reserved",
            "run",
            "put:in_flight:submitted",
            "observe",
            "put:succeeded",
            "put:completed"
        ]
    );
    assert_eq!(journal.uploads, std::slice::from_ref(&result.key));
    assert_eq!(
        result.key,
        format!("operations/seed/{}.dump", result.operation_id)
    );
    assert_eq!(result.bucket, BUCKET);
    assert!(!result.resumed);
    assert_eq!(at(journal.record(), "/state/status"), "completed");
    assert_eq!(*at(journal.record(), "/id"), result.operation_id);
    assert_eq!(*at(journal.record(), "/state/object_key"), result.key);
    assert_eq!(
        at(journal.record(), "/state/task_arn"),
        "arn:aws:ecs:us-east-1:123456789012:task/portal-cluster/attempt-1"
    );
    assert!(
        journal
            .writes
            .iter()
            .all(|record| *at(record, "/id") == result.operation_id
                && *at(record, "/state/object_key") == result.key)
    );
    drop(journal);
    assert_eq!(tasks.map(|rule| rule.num_calls()), [1, 1]);
    project.assert_consumed(2, 6);
    Ok(())
}

#[tokio::test]
async fn seed_resumes_reserved_attempt_with_pinned_input_without_opening_new_file() -> TestResult {
    // Given a reserved attempt whose original dump is pinned in S3.
    let mut project = Project::new(SEED_LOCK_KEY, Some(active(&json!({"launch": "reserved"}))))?;
    let tasks = project.tasks(false, [0]);
    // When a different, nonexistent input is supplied to the real entrypoint.
    let result = invoke(&project, &project.dir.path().join("do-not-read.dump")).await?;
    // Then the old operation, dump, and ECS idempotency token are reused.
    let journal = locked(&project.journal);
    assert!(result.resumed);
    assert_eq!(result.operation_id, ID);
    assert_eq!(result.key, KEY);
    assert_eq!(journal.tokens, [ATTEMPT]);
    assert_eq!(journal.uploads, Vec::<String>::new());
    assert_eq!(
        journal
            .writes
            .iter()
            .map(checkpoint_label)
            .collect::<Vec<_>>(),
        ["in_flight:submitted", "succeeded", "completed"]
    );
    assert_eq!(tasks.map(|rule| rule.num_calls()), [1, 1]);
    project.assert_consumed(2, 3);
    Ok(())
}

#[tokio::test]
async fn seed_resumes_submitted_attempt_by_observing_the_same_task_without_run_or_upload()
-> TestResult {
    // Given an already-submitted restore, including its durable ARN.
    let mut project = Project::new(
        SEED_LOCK_KEY,
        Some(active(&json!({"launch": "submitted", "task_arn": TASK}))),
    )?;
    let tasks = project.tasks(false, [0]);
    // When a later approved invocation resumes it.
    let result = invoke(&project, Path::new("not-a-local-dump")).await?;
    // Then no replacement attempt or upload occurs and the same ARN completes.
    let journal = locked(&project.journal);
    assert_eq!(result.key, KEY);
    assert!(result.resumed);
    assert_eq!(journal.tokens, Vec::<String>::new());
    assert_eq!(journal.uploads, Vec::<String>::new());
    assert_eq!(at(journal.record(), "/state/task_arn"), TASK);
    assert_eq!(
        journal.events,
        ["base", "load", "observe", "put:succeeded", "put:completed"]
    );
    assert_eq!(tasks.map(|rule| rule.num_calls()), [0, 1]);
    project.assert_consumed(2, 2);
    Ok(())
}

#[tokio::test]
async fn seed_observed_failure_returns_and_only_a_later_approved_invocation_retries() -> TestResult
{
    // Given one reserved operation and a failing, then successful AWS task.
    let mut project = Project::new(SEED_LOCK_KEY, Some(active(&json!({"launch": "reserved"}))))?;
    let tasks = project.tasks(false, [1, 0]);
    // When the first invocation observes STOPPED/nonzero.
    let first = invoke(&project, Path::new("unused.dump")).await;
    // Then it returns failure without retrying or replacing its pinned input.
    assert_eq!(
        first.map(|_| ()).map_err(|error| error.code()),
        Err("DB_TASK_FAILED")
    );
    {
        let journal = locked(&project.journal);
        assert_eq!(checkpoint_label(journal.record()), "failed");
        assert_eq!(journal.tokens, [ATTEMPT]);
        assert_eq!(journal.writes.len(), 2);
    }
    project.ctx.approve = false;
    assert_eq!(
        invoke(&project, Path::new("unused.dump"))
            .await
            .map(|_| ())
            .map_err(|error| error.code()),
        Err("APPROVAL_REQUIRED")
    );
    project.ctx.approve = true;
    let result = invoke(&project, Path::new("different-missing.dump")).await?;
    let journal = locked(&project.journal);
    assert_eq!(result.operation_id, ID);
    assert_eq!(result.key, KEY);
    assert!(result.resumed);
    assert_eq!(journal.tokens.len(), 2);
    assert_ne!(journal.tokens.first(), journal.tokens.get(1));
    assert_eq!(journal.uploads, Vec::<String>::new());
    assert_eq!(checkpoint_label(journal.record()), "completed");
    assert_eq!(tasks.map(|rule| rule.num_calls()), [2, 2]);
    project.assert_consumed(4, 6);
    Ok(())
}

#[tokio::test]
async fn seed_replaces_only_a_completed_checkpoint_using_its_current_etag() -> TestResult {
    // Given durable completion of an older operation and a new local dump.
    let completed = json!({"schema_version": 1, "id": ID, "state": {"status": "completed", "object_key": KEY, "task_arn": TASK}});
    let mut project = Project::new(SEED_LOCK_KEY, Some(completed))?;
    let file = project.dir.path().join("next.dump");
    std::fs::write(&file, b"next-dump")?;
    let tasks = project.tasks(false, [0]);
    // When a new seed is explicitly requested.
    let result = invoke(&project, &file).await?;
    // Then a new identity is CAS-installed, not a deletion or stale resurrection.
    assert_ne!(result.operation_id, ID);
    assert!(!result.resumed);
    assert_eq!(
        *at(locked(&project.journal).record(), "/id"),
        result.operation_id
    );
    assert_eq!(tasks.map(|rule| rule.num_calls()), [1, 1]);
    project.assert_consumed(2, 6);
    Ok(())
}

#[tokio::test]
async fn seed_rejects_oversized_persisted_json_before_any_mutation() -> TestResult {
    // Given a hostile checkpoint larger than the small-record byte budget.
    let mut record = active(&json!({"launch": "reserved"}));
    present(record.as_object_mut(), "checkpoint is an object")
        .insert("padding".to_owned(), Value::String("x".repeat(65_536)));
    let mut project = Project::new(SEED_LOCK_KEY, Some(record))?;
    let tasks = project.tasks(false, []);
    // When the real entrypoint attempts to resume it.
    let result = invoke(&project, Path::new("unused.dump")).await;
    // Then the bounded reader fails closed without upload, CAS, or ECS effects.
    assert_eq!(
        result.map(|_| ()).map_err(|error| error.to_string()),
        Err("operation checkpoint exceeds the 65536-byte limit".to_owned())
    );
    assert_eq!(tasks.map(|rule| rule.num_calls()), [0, 0]);
    project.assert_consumed(2, 0);
    Ok(())
}

#[tokio::test]
async fn expired_seed_task_requires_positive_reconciliation_before_checkpoint_completion()
-> TestResult {
    // Given an in-flight operation whose original ECS metadata has expired.
    for reconciliation_exit in [0, 3] {
        let initial = active(&json!({"launch": "submitted", "task_arn": TASK}));
        let mut project = Project::new(SEED_LOCK_KEY, Some(initial.clone()))?;
        let observed =
            mock!(aws_sdk_ecs::Client::describe_tasks).then_compute_output(move |request| {
                if request.tasks() == [TASK] {
                    return DescribeTasksOutput::builder()
                        .failures(Failure::builder().arn(TASK).reason("MISSING").build())
                        .build();
                }
                DescribeTasksOutput::builder()
                    .tasks(
                        Task::builder()
                            .last_status("STOPPED")
                            .containers(Container::builder().exit_code(reconciliation_exit).build())
                            .build(),
                    )
                    .build()
            });
        let reconciled = mock!(aws_sdk_ecs::Client::run_task).then_output(|| {
            RunTaskOutput::builder()
                .tasks(
                    Task::builder()
                        .task_arn(
                            "arn:aws:ecs:us-east-1:123456789012:task/portal-cluster/reconciliation",
                        )
                        .build(),
                )
                .build()
        });
        project.ctx.ecs = mock_client!(aws_sdk_ecs, RuleMode::MatchAny, [&observed, &reconciled]);
        // When resuming without opening any replacement dump.
        let result = invoke(&project, Path::new("does-not-exist.dump")).await;
        let uploads = locked(&project.journal).uploads.clone();
        assert_eq!(uploads, Vec::<String>::new());
        assert_eq!(reconciled.num_calls(), 1);
        assert_eq!(observed.num_calls(), 2);
        if reconciliation_exit == 0 {
            let result = result?;
            assert!(result.resumed);
            assert_eq!(result.key, KEY);
            assert_eq!(
                at(locked(&project.journal).record(), "/state/status"),
                "completed"
            );
            assert_eq!(
                at(locked(&project.journal).record(), "/state/task_arn"),
                TASK
            );
        } else {
            assert_eq!(
                result.map(|_| ()).map_err(|error| error.code()),
                Err("PRECONDITION_FAILED")
            );
            assert_eq!(locked(&project.journal).record(), &initial);
            assert!(
                locked(&project.journal).writes.is_empty(),
                "unknown must not authorize a retry"
            );
        }
    }
    Ok(())
}
