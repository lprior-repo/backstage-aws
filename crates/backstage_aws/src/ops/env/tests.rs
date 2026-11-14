use super::*;
use aws_sdk_ecs::{
    operation::{
        describe_services::DescribeServicesOutput,
        describe_tasks::DescribeTasksOutput,
        run_task::{RunTaskInput, RunTaskOutput},
    },
    types::{Container, Deployment, Task},
};
use aws_sdk_elasticloadbalancingv2::{
    operation::describe_target_health::DescribeTargetHealthOutput,
    types::{TargetHealth, TargetHealthDescription},
};
use aws_sdk_s3::{
    operation::{
        delete_object::DeleteObjectOutput,
        get_object::{GetObjectError, GetObjectOutput},
    },
    primitives::ByteStream,
    types::error::NoSuchKey,
};
use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod rollout;

const NEW: &str = "arn:aws:ecs:us-east-1:123456789012:task-definition/portal-pr-38:7";
const OLD: &str = "arn:aws:ecs:us-east-1:123456789012:task-definition/portal-pr-38:6";
const DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const FAKE_TERRAFORM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/terraform");
const ENV_STATE: &str = r#"{"outputs":{
    "url":{"value":"https://pr-38.preview.example.com"},
    "ecs_cluster_name":{"value":"portal-cluster"},
    "ecs_service_name":{"value":"portal-pr-38"},
    "task_definition_arn":{"value":"arn:aws:ecs:us-east-1:123456789012:task-definition/portal-pr-38:7"},
    "target_group_arn":{"value":"arn:targetgroup/bs-1"},
    "image_digest":{"value":"sha256:1111111111111111111111111111111111111111111111111111111111111111"}}}"#;
const BASE_STATE: &str = r#"{"outputs":{
    "ecs_cluster_name":{"value":"portal-cluster"},
    "db_admin_seed_task_definition_arn":{"value":"arn:task-definition/portal-db-admin-clone:3"},
    "db_admin_network_configuration":{"value":{"awsvpcConfiguration":{
        "subnets":["subnet-a"],"securityGroups":["sg-admin"],"assignPublicIp":"DISABLED"}}}}}"#;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type Rollout<'a> = (&'a str, &'a str, DeploymentRolloutState);

fn service(deployments: &[Rollout]) -> Service {
    let deployments = deployments.iter().map(|(status, task_definition, state)| {
        Deployment::builder()
            .status(*status)
            .task_definition(*task_definition)
            .rollout_state(state.clone())
            .rollout_state_reason("tasks failed to start")
            .build()
    });
    Service::builder()
        .set_deployments(Some(deployments.collect()))
        .build()
}

/// A project driven by the fake Terraform, whose invocations land in a log
/// that the AWS mocks consult to enforce the order of operations.
struct Project {
    dir: tempfile::TempDir,
    ctx: Ctx,
}

impl Project {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join("infra/env"))?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        ctx.terraform = FAKE_TERRAFORM.into();
        Ok(Self { dir, ctx })
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("calls.log")
    }

    fn terraform_calls(&self) -> Vec<String> {
        let log = fs::read_to_string(self.log()).unwrap_or_default();
        log.lines().map(str::to_owned).collect()
    }
}

fn applied(log: &Path) -> bool {
    fs::read_to_string(log).is_ok_and(|calls| calls.contains("env: apply"))
}

fn state(key: &'static str, body: &'static str) -> Rule {
    mock!(aws_sdk_s3::Client::get_object)
        .match_requests(move |request| request.key() == Some(key))
        .then_output(move || {
            GetObjectOutput::builder()
                .body(ByteStream::from_static(body.as_bytes()))
                .build()
        })
}

/// A db-admin task run with `action` for `pr_38`, accepted only while the
/// Terraform apply has, or has not yet, happened.
fn database_task(action: &'static str, log: PathBuf, after_apply: bool) -> Rule {
    let expected = move |request: &RunTaskInput| {
        let overrides = request.overrides().map(|all| all.container_overrides());
        let environment = overrides
            .and_then(<[_]>::first)
            .map(|one| one.environment());
        let pairs = environment.into_iter().flatten();
        let pairs: Vec<_> = pairs.map(|pair| (pair.name(), pair.value())).collect();
        pairs
            == [
                (Some("DB_ACTION"), Some(action)),
                (Some("DB_NAME"), Some("pr_38")),
            ]
            && applied(&log) == after_apply
    };
    let task = Task::builder().task_arn("arn:task/abc").build();
    mock!(aws_sdk_ecs::Client::run_task)
        .match_requests(expected)
        .then_output(move || RunTaskOutput::builder().tasks(task.clone()).build())
}

fn stopped(exit_code: i32) -> Rule {
    mock!(aws_sdk_ecs::Client::describe_tasks).then_output(move || {
        let container = Container::builder().exit_code(exit_code).build();
        let task = Task::builder().last_status("STOPPED").containers(container);
        DescribeTasksOutput::builder().tasks(task.build()).build()
    })
}

/// Accepts the deletion only once the database task has run.
fn delete_state(database_task: &Rule) -> Rule {
    let dropped = database_task.clone();
    mock!(aws_sdk_s3::Client::delete_object)
        .match_requests(move |request| {
            request.key() == Some("env/pr-38.tfstate") && dropped.num_calls() == 1
        })
        .then_output(|| DeleteObjectOutput::builder().build())
}

async fn tear_down(
    env_state: Rule,
    destroys: bool,
) -> std::result::Result<(Project, Result<bool>, Rule, Rule), Box<dyn std::error::Error>> {
    let mut project = Project::new()?;
    let base = state("base/terraform.tfstate", BASE_STATE);
    let drop_database = database_task("drop", project.log(), destroys);
    let delete = delete_state(&drop_database);
    project.ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&env_state, &base, &delete]);
    let finished = stopped(0);
    project.ctx.ecs = mock_client!(aws_sdk_ecs, RuleMode::MatchAny, [&drop_database, &finished]);
    let result = env_down(&project.ctx).await.map(|result| result.destroyed);
    Ok((project, result, drop_database, delete))
}

#[tokio::test]
async fn teardown_destroys_the_stack_then_drops_the_database_then_forgets_the_state() -> TestResult
{
    let (project, destroyed, drop_database, delete) =
        tear_down(state("env/pr-38.tfstate", ENV_STATE), true).await?;
    assert!(destroyed?);
    let calls = project.terraform_calls();
    assert_eq!(
        calls.get(1).map(String::as_str),
        Some("env: plan -input=false -out backstage-aws.tfplan -destroy")
    );
    assert_eq!(
        calls.get(2).map(String::as_str),
        Some("env: apply -input=false backstage-aws.tfplan")
    );
    assert_eq!((drop_database.num_calls(), delete.num_calls()), (1, 1));
    Ok(())
}

#[tokio::test]
async fn a_failed_destroy_keeps_the_database_and_the_state() -> TestResult {
    let env_state = state("env/pr-38.tfstate", ENV_STATE);
    let mut project = Project::new()?;
    fs::write(project.dir.path().join("fail-apply"), "")?;
    let drop_database = database_task("drop", project.log(), true);
    let delete = delete_state(&drop_database);
    project.ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&env_state, &delete]);
    project.ctx.ecs = mock_client!(aws_sdk_ecs, RuleMode::MatchAny, [&drop_database]);
    let failed = env_down(&project.ctx).await.map(|result| result.destroyed);
    assert_eq!(failed.map_err(|error| error.code()), Err("TOOL_FAILED"));
    assert_eq!((drop_database.num_calls(), delete.num_calls()), (0, 0));
    Ok(())
}

#[tokio::test]
async fn teardown_of_a_namespace_without_state_still_drops_its_database() -> TestResult {
    let missing = mock!(aws_sdk_s3::Client::get_object)
        .match_requests(|request| request.key() == Some("env/pr-38.tfstate"))
        .then_error(|| GetObjectError::NoSuchKey(NoSuchKey::builder().build()));
    let (project, destroyed, drop_database, delete) = tear_down(missing, false).await?;
    assert!(!destroyed?);
    assert_eq!(project.terraform_calls(), Vec::<String>::new());
    assert_eq!((drop_database.num_calls(), delete.num_calls()), (1, 1));
    Ok(())
}

/// Deploys an image that is already in the registry, so no Docker is needed.
async fn deploy(
    clone_exit_code: i32,
) -> std::result::Result<(Project, Result<EnvUpResult>), Box<dyn std::error::Error>> {
    let mut project = Project::new()?;
    let log = project.log();
    let states = [
        state("base/terraform.tfstate", BASE_STATE),
        state("env/pr-38.tfstate", ENV_STATE),
    ];
    project.ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, &states);
    let rolled_out = mock!(aws_sdk_ecs::Client::describe_services)
        .match_requests(move |request| request.services() == ["portal-pr-38"] && applied(&log))
        .then_output(|| {
            let done = service(&[("PRIMARY", NEW, DeploymentRolloutState::Completed)]);
            DescribeServicesOutput::builder().services(done).build()
        });
    let ecs = [
        database_task("clone", project.log(), false),
        stopped(clone_exit_code),
        rolled_out,
    ];
    project.ctx.ecs = mock_client!(aws_sdk_ecs, RuleMode::MatchAny, &ecs);
    let healthy = mock!(aws_sdk_elasticloadbalancingv2::Client::describe_target_health)
        .match_requests(|request| request.target_group_arn() == Some("arn:targetgroup/bs-1"))
        .then_output(|| {
            let health = TargetHealth::builder()
                .state(TargetHealthStateEnum::Healthy)
                .build();
            let target = TargetHealthDescription::builder()
                .target_health(health)
                .build();
            DescribeTargetHealthOutput::builder()
                .target_health_descriptions(target)
                .build()
        });
    project.ctx.elb = mock_client!(
        aws_sdk_elasticloadbalancingv2,
        RuleMode::MatchAny,
        [&healthy]
    );
    let result = env_up(&project.ctx, Some(DIGEST), false).await;
    Ok((project, result))
}

#[tokio::test]
async fn a_deployment_clones_applies_then_waits_for_the_rollout_and_a_healthy_target() -> TestResult
{
    let (project, result) = deploy(0).await?;
    let result = result?;
    assert_eq!(result.url, "https://pr-38.preview.example.com");
    assert_eq!(result.image_digest, DIGEST);
    // No pull request is configured, so GitHub is never contacted.
    assert_eq!(result.comment_url, None);
    let steps: Vec<_> = result
        .timings
        .iter()
        .map(|step| (step.name.as_str(), step.status.as_str()))
        .collect();
    let expected = [
        "image.build",
        "database.clone",
        "terraform.apply",
        "ecs.rollout",
        "alb.healthy",
    ];
    assert_eq!(steps, expected.map(|name| (name, "ok")));
    let vars = fs::read(project.dir.path().join("infra/env/terraform.tfvars.json"))?;
    let vars: Value = serde_json::from_slice(&vars)?;
    assert_eq!(vars.get("image_digest"), Some(&Value::from(DIGEST)));
    assert_eq!(vars.get("database_name"), Some(&Value::from("pr_38")));
    assert_eq!(vars.get("spot"), Some(&Value::from(true)));
    let bucket = Value::from("portal-123456789012-us-east-1-tfstate");
    assert_eq!(vars.get("state_bucket_name"), Some(&bucket));
    Ok(())
}

#[tokio::test]
async fn nothing_is_applied_when_the_database_clone_fails() -> TestResult {
    let (project, result) = deploy(1).await?;
    assert_eq!(
        result.map(|up| up.url).map_err(|error| error.code()),
        Err("DB_TASK_FAILED")
    );
    assert_eq!(project.terraform_calls(), Vec::<String>::new());
    Ok(())
}
