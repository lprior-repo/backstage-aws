use super::*;
use crate::db::test_support::{
    CLUSTER, Project, SECRET, SERVICE, TestResult, checkpoint_label, locked,
};
use aws_sdk_ecs::{
    operation::{
        describe_services::DescribeServicesOutput, list_services::ListServicesOutput,
        update_service::UpdateServiceOutput,
    },
    types::{Deployment, Service},
};
use aws_sdk_secretsmanager::{
    operation::{
        get_random_password::GetRandomPasswordOutput,
        get_secret_value::{GetSecretValueError, GetSecretValueOutput},
        put_secret_value::PutSecretValueOutput,
        update_secret_version_stage::UpdateSecretVersionStageOutput,
    },
    types::error::ResourceNotFoundException,
};
use aws_smithy_mocks::{MockResponse, Rule, RuleMode, mock, mock_client};
use serde_json::json;

const BASELINE: &str = "22222222-2222-4222-8222-222222222222";
const OPERATION: &str = "11111111-1111-4111-8111-111111111111";
const ATTEMPT: &str = "33333333-3333-4333-8333-333333333333";
const PASSWORD: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn secret_reads(project: &Project) -> Rule {
    let reads = project.journal.clone();
    mock!(aws_sdk_secretsmanager::Client::get_secret_value).then_compute_response(move |request| {
        let mut journal = locked(&reads);
        assert_eq!(request.secret_id(), Some(SECRET));
        if request.version_stage() == Some("AWSCURRENT") {
            let label = checkpoint_label(journal.record());
            assert!(matches!(label.as_str(), "prepared" | "succeeded"));
            journal.event("current");
            drop(journal);
            return MockResponse::Output(
                GetSecretValueOutput::builder().version_id(BASELINE).build(),
            );
        }
        assert_eq!(checkpoint_label(journal.record()), "baseline_pinned");
        assert_eq!(
            journal
                .record()
                .pointer("/state/previous_version_id")
                .and_then(Value::as_str),
            Some(BASELINE)
        );
        if request.version_id() == journal.record()["id"].as_str() {
            journal.event("pending");
            drop(journal);
            return MockResponse::Error(GetSecretValueError::ResourceNotFoundException(
                ResourceNotFoundException::builder().build(),
            ));
        }
        assert_eq!(request.version_id(), Some(BASELINE));
        journal.event("predecessor");
        drop(journal);
        MockResponse::Output(
            GetSecretValueOutput::builder()
                .version_id(BASELINE)
                .secret_string(r#"{"username":"backstage","password":"old-password","port":5432}"#)
                .build(),
        )
    })
}

fn install_secrets(project: &mut Project) -> [Rule; 4] {
    let get = secret_reads(project);
    let generated = project.journal.clone();
    let random = mock!(aws_sdk_secretsmanager::Client::get_random_password).then_compute_output(
        move |request| {
            assert_eq!(request.password_length(), Some(40));
            assert_eq!(request.exclude_punctuation(), Some(true));
            let mut journal = locked(&generated);
            assert_eq!(checkpoint_label(journal.record()), "baseline_pinned");
            journal.event("random");
            drop(journal);
            GetRandomPasswordOutput::builder()
                .random_password(PASSWORD)
                .build()
        },
    );
    let staged = project.journal.clone();
    let put = mock!(aws_sdk_secretsmanager::Client::put_secret_value).then_compute_output(
        move |request| {
            let mut journal = locked(&staged);
            assert_eq!(checkpoint_label(journal.record()), "baseline_pinned");
            assert_eq!(request.secret_id(), Some(SECRET));
            assert_eq!(
                request.client_request_token(),
                journal.record()["id"].as_str()
            );
            assert_eq!(request.version_stages(), &["AWSPENDING"]);
            let document: Value = serde_json::from_str(request.secret_string().unwrap_or_default())
                .unwrap_or(Value::Null);
            assert_eq!(
                document,
                json!({"username": "backstage", "password": PASSWORD, "port": 5432})
            );
            journal.event("stage");
            drop(journal);
            PutSecretValueOutput::builder()
                .version_id(request.client_request_token().unwrap_or_default())
                .build()
        },
    );
    let promoted = project.journal.clone();
    let promote = mock!(aws_sdk_secretsmanager::Client::update_secret_version_stage)
        .then_compute_output(move |request| {
            let mut journal = locked(&promoted);
            assert_eq!(checkpoint_label(journal.record()), "succeeded");
            assert_eq!(request.secret_id(), Some(SECRET));
            assert_eq!(request.version_stage(), Some("AWSCURRENT"));
            assert_eq!(
                request.move_to_version_id(),
                journal.record()["id"].as_str()
            );
            assert_eq!(request.remove_from_version_id(), Some(BASELINE));
            journal.event("promote");
            drop(journal);
            UpdateSecretVersionStageOutput::builder().build()
        });
    project.ctx.secrets = mock_client!(
        aws_sdk_secretsmanager,
        RuleMode::MatchAny,
        [&get, &random, &put, &promote]
    );
    [get, random, put, promote]
}

fn install_ecs(project: &mut Project, exit_codes: impl IntoIterator<Item = i32>) -> [Rule; 5] {
    let [run, observe] = project.tasks(true, exit_codes);
    let listed = project.journal.clone();
    let list = mock!(aws_sdk_ecs::Client::list_services).then_compute_output(move |request| {
        let mut journal = locked(&listed);
        assert_eq!(checkpoint_label(journal.record()), "secret_promoted");
        assert_eq!(request.cluster(), Some(CLUSTER));
        assert_eq!(request.max_results(), Some(100));
        journal.event("list");
        drop(journal);
        ListServicesOutput::builder().service_arns(SERVICE).build()
    });
    let restarted = project.journal.clone();
    let update = mock!(aws_sdk_ecs::Client::update_service).then_compute_output(move |request| {
        let mut journal = locked(&restarted);
        assert_eq!(checkpoint_label(journal.record()), "secret_promoted");
        assert_eq!(request.cluster(), Some(CLUSTER));
        assert_eq!(request.service(), Some(SERVICE));
        assert_eq!(request.force_new_deployment(), Some(true));
        journal.event("restart");
        drop(journal);
        UpdateServiceOutput::builder().build()
    });
    let settled = project.journal.clone();
    let stable =
        mock!(aws_sdk_ecs::Client::describe_services).then_compute_output(move |request| {
            let mut journal = locked(&settled);
            assert_eq!(checkpoint_label(journal.record()), "secret_promoted");
            assert_eq!(request.cluster(), Some(CLUSTER));
            assert_eq!(request.services(), &[SERVICE]);
            journal.event("stable");
            drop(journal);
            DescribeServicesOutput::builder()
                .services(
                    Service::builder()
                        .service_arn(SERVICE)
                        .status("ACTIVE")
                        .desired_count(1)
                        .running_count(1)
                        .pending_count(0)
                        .deployments(
                            Deployment::builder()
                                .status("PRIMARY")
                                .desired_count(1)
                                .running_count(1)
                                .build(),
                        )
                        .build(),
                )
                .build()
        });
    project.ctx.ecs = mock_client!(
        aws_sdk_ecs,
        RuleMode::MatchAny,
        [&run, &observe, &list, &update, &stable]
    );
    [run, observe, list, update, stable]
}

async fn invoke(
    project: &Project,
) -> std::result::Result<Result<RotationResult>, tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(5), rotate_password(&project.ctx)).await
}

fn reserved_operation() -> Value {
    json!({"schema_version": 1, "id": OPERATION, "state": {
        "phase": "role_change", "previous_version_id": BASELINE,
        "attempt": {"status": "in_flight", "attempt_id": ATTEMPT, "launch": {"launch": "reserved"}}
    }})
}

#[tokio::test]
async fn rotation_pins_versions_and_checkpoints_before_effects_then_promotes_restarts_and_completes()
-> TestResult {
    // Given no rotation record and an existing application secret/service.
    let mut project = Project::new(ROTATION_LOCK_KEY, None)?;
    let secrets = install_secrets(&mut project);
    let ecs = install_ecs(&mut project, [0]);
    // When the actual approved rotation entrypoint runs to completion.
    let result = invoke(&project).await??;
    // Then pinned identities, CAS predecessors, effect order, and durable completion agree.
    let journal = locked(&project.journal);
    assert_eq!(
        journal.events,
        [
            "base",
            "load",
            "put:prepared",
            "current",
            "put:baseline_pinned",
            "pending",
            "predecessor",
            "random",
            "stage",
            "put:secret_staged",
            "put:ready",
            "put:in_flight:reserved",
            "run",
            "put:in_flight:submitted",
            "observe",
            "put:succeeded",
            "current",
            "promote",
            "put:secret_promoted",
            "list",
            "restart",
            "stable",
            "put:services_restarted",
            "put:completed"
        ]
    );
    assert_eq!(result.secret_arn, SECRET);
    assert_eq!(result.services_restarted, [SERVICE]);
    assert_eq!(result.timings.len(), 3);
    assert_eq!(journal.record()["id"], result.version_id);
    assert_eq!(
        journal
            .record()
            .pointer("/state/phase")
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        journal
            .record()
            .pointer("/state/previous_version_id")
            .and_then(Value::as_str),
        Some(BASELINE)
    );
    assert!(
        journal
            .writes
            .iter()
            .all(|record| record["id"] == result.version_id)
    );
    assert!(journal.writes.iter().skip(1).all(|record| {
        record
            .pointer("/state/previous_version_id")
            .and_then(Value::as_str)
            == Some(BASELINE)
    }));
    drop(journal);
    assert_eq!(secrets.map(|rule| rule.num_calls()), [4, 1, 1, 1]);
    assert_eq!(ecs.map(|rule| rule.num_calls()), [1, 1, 1, 1, 1]);
    project.assert_consumed(2, 10);
    Ok(())
}

#[tokio::test]
async fn rotation_observed_failure_returns_without_retry_and_later_approval_retries_same_versions()
-> TestResult {
    // Given an existing operation whose task will fail once, then succeed.
    let mut project = Project::new(ROTATION_LOCK_KEY, Some(reserved_operation()))?;
    let secrets = install_secrets(&mut project);
    let ecs = install_ecs(&mut project, [1, 0]);
    // When the first invocation observes a failed task, it must stop immediately.
    assert_eq!(
        invoke(&project)
            .await?
            .map(|_| ())
            .map_err(|error| error.code()),
        Err("DB_TASK_FAILED")
    );
    {
        let journal = locked(&project.journal);
        assert_eq!(checkpoint_label(journal.record()), "failed");
        assert_eq!(journal.tokens, [ATTEMPT]);
        assert_eq!(
            journal.events,
            [
                "base",
                "load",
                "run",
                "put:in_flight:submitted",
                "observe",
                "put:failed"
            ]
        );
    }
    assert_eq!(
        secrets.iter().map(Rule::num_calls).collect::<Vec<_>>(),
        [0, 0, 0, 0]
    );
    // Then only a later approved invocation may reserve a distinct attempt.
    project.ctx.approve = false;
    assert_eq!(
        invoke(&project)
            .await?
            .map(|_| ())
            .map_err(|error| error.code()),
        Err("APPROVAL_REQUIRED")
    );
    project.ctx.approve = true;
    let result = invoke(&project).await??;
    let journal = locked(&project.journal);
    assert_eq!(result.version_id, OPERATION);
    assert_eq!(result.services_restarted, [SERVICE]);
    assert_eq!(journal.tokens.len(), 2);
    assert_ne!(journal.tokens.first(), journal.tokens.get(1));
    assert_eq!(
        journal
            .record()
            .pointer("/state/previous_version_id")
            .and_then(Value::as_str),
        Some(BASELINE)
    );
    assert_eq!(checkpoint_label(journal.record()), "completed");
    drop(journal);
    assert_eq!(secrets.map(|rule| rule.num_calls()), [1, 0, 0, 1]);
    assert_eq!(ecs.map(|rule| rule.num_calls()), [2, 2, 1, 1, 1]);
    project.assert_consumed(4, 8);
    Ok(())
}

#[tokio::test]
async fn rotation_resumes_an_uncertain_reserved_attempt_without_restaging_secret_or_changing_token()
-> TestResult {
    // Given a pinned role-change operation with a durable reserved attempt.
    let mut project = Project::new(ROTATION_LOCK_KEY, Some(reserved_operation()))?;
    let secrets = install_secrets(&mut project);
    let ecs = install_ecs(&mut project, [0]);
    // When an approved invocation resumes the operation.
    let result = invoke(&project).await??;
    // Then it reuses the old token/replacement/predecessor and completes normally.
    let journal = locked(&project.journal);
    assert_eq!(journal.tokens, [ATTEMPT]);
    assert_eq!(result.version_id, OPERATION);
    assert_eq!(checkpoint_label(journal.record()), "completed");
    drop(journal);
    assert_eq!(secrets.map(|rule| rule.num_calls()), [1, 0, 0, 1]);
    assert_eq!(ecs.map(|rule| rule.num_calls()), [1, 1, 1, 1, 1]);
    project.assert_consumed(2, 5);
    Ok(())
}

#[tokio::test]
async fn rotation_rejects_oversized_checkpoint_without_aws_effects() -> TestResult {
    // Given hostile persisted JSON that exceeds the checkpoint byte budget.
    let mut record = reserved_operation();
    record
        .as_object_mut()
        .ok_or("reserved operation is a JSON object")?
        .insert("padding".to_owned(), Value::String("x".repeat(65536)));
    let mut project = Project::new(ROTATION_LOCK_KEY, Some(record))?;
    let secrets = install_secrets(&mut project);
    let ecs = install_ecs(&mut project, []);
    // When the real rotation entrypoint reads it.
    let result = invoke(&project).await?;
    // Then no secret, task, service, or checkpoint mutation is made.
    assert_eq!(
        result.map(|_| ()).map_err(|error| error.to_string()),
        Err("operation checkpoint exceeds the 65536-byte limit".to_owned())
    );
    assert_eq!(secrets.map(|rule| rule.num_calls()), [0, 0, 0, 0]);
    assert_eq!(ecs.map(|rule| rule.num_calls()), [0, 0, 0, 0, 0]);
    project.assert_consumed(2, 0);
    Ok(())
}
