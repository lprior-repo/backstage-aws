//! One-shot database administration tasks on Fargate.
//!
//! The base stack defines the task definitions (`psql` and `pg_restore`). This
//! module only starts them, with validated non-secret environment overrides,
//! and waits for the result.
use crate::{
    ctx::{Ctx, poll},
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecs::types::{
    AssignPublicIp, AwsVpcConfiguration, ContainerOverride, KeyValuePair, LaunchType,
    NetworkConfiguration, TaskOverride,
};
use aws_sdk_s3::primitives::ByteStream;
use serde::Serialize;
use serde_json::Value;
use std::{path::Path, time::Duration};

/// The object key the restore task downloads.
const SEED_KEY: &str = "seed.dump";

#[derive(Serialize)]
pub(crate) struct SeedResult {
    bucket: String,
    key: &'static str,
}

/// Creates the application role and the empty `seed` database, and sets the
/// role's password from its secret. Safe to repeat.
pub(crate) async fn initialize_database(ctx: &Ctx) -> Result<()> {
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    run_task(ctx, &base, "db_admin_init_task_definition_arn", None).await
}

/// Creates the namespace database from `seed` when it does not exist yet.
pub(crate) async fn clone_database(ctx: &Ctx) -> Result<()> {
    namespace_task(ctx, "clone").await
}

/// Drops the namespace database, disconnecting any session still attached.
pub(crate) async fn drop_database(ctx: &Ctx) -> Result<()> {
    namespace_task(ctx, "drop").await
}

async fn namespace_task(ctx: &Ctx, action: &str) -> Result<()> {
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let pair = |name: &str, value: &str| KeyValuePair::builder().name(name).value(value).build();
    let overrides = ContainerOverride::builder()
        .name("db-admin")
        .environment(pair("DB_ACTION", action))
        .environment(pair("DB_NAME", ctx.settings.database_name()))
        .build();
    let task = "db_admin_seed_task_definition_arn";
    run_task(ctx, &base, task, Some(overrides)).await
}

/// Uploads a custom-format `pg_dump` file and restores it into `seed`.
pub(crate) async fn seed(ctx: &Ctx, file: &Path) -> Result<SeedResult> {
    ctx.require_approval()?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let bucket = output_str(&base, "seed_bucket_name")?.to_owned();
    let body = ByteStream::from_path(file)
        .await
        .map_err(|error| Error::invalid_input(format!("{}: {error}", file.display())))?;
    ctx.s3
        .put_object()
        .bucket(&bucket)
        .key(SEED_KEY)
        .body(body)
        .send()
        .await
        .map_err(|error| Error::aws_failed("s3", "PutObject", error.to_string()))?;
    let task = "db_admin_seed_restore_task_definition_arn";
    run_task(ctx, &base, task, None).await?;
    Ok(SeedResult {
        bucket,
        key: SEED_KEY,
    })
}

/// Runs one db-admin task definition to completion and requires every
/// container to exit with status 0.
pub(crate) async fn run_task(
    ctx: &Ctx,
    base: &Value,
    task_output: &str,
    overrides: Option<ContainerOverride>,
) -> Result<()> {
    let cluster = output_str(base, "ecs_cluster_name")?;
    let failed = |detail: String| Error::aws_failed("ecs", "RunTask", detail);
    let started = ctx
        .ecs
        .run_task()
        .cluster(cluster)
        .task_definition(output_str(base, task_output)?)
        .launch_type(LaunchType::Fargate)
        .network_configuration(network(base)?)
        .overrides(
            TaskOverride::builder()
                .set_container_overrides(overrides.map(|one| vec![one]))
                .build(),
        )
        .send()
        .await
        .map_err(|error| failed(error.to_string()))?;
    let arn = started
        .tasks()
        .first()
        .and_then(|task| task.task_arn())
        .ok_or_else(|| failed(format!("no task started: {:?}", started.failures())))?;
    let describe = || async {
        let request = ctx.ecs.describe_tasks().cluster(cluster).tasks(arn);
        let described = request
            .send()
            .await
            .map_err(|error| Error::aws_failed("ecs", "DescribeTasks", error.to_string()))?;
        let task = described.tasks().first().cloned();
        Ok(task.filter(|task| task.last_status() == Some("STOPPED")))
    };
    // Thirty minutes, checked every two seconds.
    let stopped = poll("database task", arn, Duration::from_secs(2), 900, describe).await?;
    let containers = stopped.containers();
    if !containers.is_empty() && containers.iter().all(|one| one.exit_code() == Some(0)) {
        return Ok(());
    }
    Err(Error::db_task_failed(format!(
        "{arn} failed ({}); see the db-admin log group",
        stopped
            .stopped_reason()
            .unwrap_or("a container exited with a non-zero status")
    )))
}

fn network(base: &Value) -> Result<NetworkConfiguration> {
    let vpc = base.pointer("/db_admin_network_configuration/awsvpcConfiguration");
    let list = |name: &str| {
        let values = vpc.and_then(|vpc| vpc.get(name)?.as_array());
        let strings = values.into_iter().flatten().filter_map(Value::as_str);
        Some(strings.map(str::to_owned).collect::<Vec<_>>())
    };
    let vpc = AwsVpcConfiguration::builder()
        .set_subnets(list("subnets"))
        .set_security_groups(list("securityGroups"))
        .assign_public_ip(AssignPublicIp::Disabled)
        .build()
        .map_err(|error| Error::precondition_failed(format!("db-admin network: {error}")))?;
    Ok(NetworkConfiguration::builder()
        .awsvpc_configuration(vpc)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_ecs::{
        Client,
        operation::{describe_tasks::DescribeTasksOutput, run_task::RunTaskOutput},
        types::{Container, Failure, Task},
    };
    use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};
    use serde_json::json;

    const TASK: &str = "arn:aws:ecs:us-east-1:123456789012:task/portal-cluster/abc";

    fn base() -> Value {
        json!({
            "ecs_cluster_name": "portal-cluster",
            "db_admin_seed_task_definition_arn": "arn:task-definition/portal-db-admin-clone:3",
            "db_admin_network_configuration": {"awsvpcConfiguration": {
                "subnets": ["subnet-a", "subnet-b"],
                "securityGroups": ["sg-admin"],
                "assignPublicIp": "DISABLED",
            }},
        })
    }

    fn described(status: &'static str, exit_code: Option<i32>) -> DescribeTasksOutput {
        let container = Container::builder().set_exit_code(exit_code).build();
        let task = Task::builder()
            .task_arn(TASK)
            .last_status(status)
            .containers(container);
        DescribeTasksOutput::builder().tasks(task.build()).build()
    }

    fn started() -> Rule {
        mock!(Client::run_task).then_output(|| {
            RunTaskOutput::builder()
                .tasks(Task::builder().task_arn(TASK).build())
                .build()
        })
    }

    async fn run(
        rules: &[&Rule],
        overrides: Option<ContainerOverride>,
    ) -> std::result::Result<(), &'static str> {
        let dir = std::env::temp_dir();
        let mut ctx = Ctx::sample(&dir, true).map_err(|error| error.code())?;
        ctx.ecs = mock_client!(aws_sdk_ecs, RuleMode::MatchAny, rules);
        let task = "db_admin_seed_task_definition_arn";
        run_task(&ctx, &base(), task, overrides)
            .await
            .map_err(|error| error.code())
    }

    #[tokio::test(start_paused = true)]
    async fn a_task_runs_privately_with_its_overrides_and_is_awaited_until_it_stops() {
        let overrides = ContainerOverride::builder()
            .name("db-admin")
            .environment(
                KeyValuePair::builder()
                    .name("DB_NAME")
                    .value("pr_38")
                    .build(),
            )
            .build();
        let expected = overrides.clone();
        let started = mock!(Client::run_task)
            .match_requests(move |request| {
                let vpc = request
                    .network_configuration()
                    .and_then(|net| net.awsvpc_configuration());
                request.cluster() == Some("portal-cluster")
                    && request.task_definition()
                        == Some("arn:task-definition/portal-db-admin-clone:3")
                    && request.launch_type() == Some(&LaunchType::Fargate)
                    && vpc.map(AwsVpcConfiguration::subnets)
                        == Some(&["subnet-a".to_owned(), "subnet-b".to_owned()])
                    && vpc.map(AwsVpcConfiguration::security_groups)
                        == Some(&["sg-admin".to_owned()])
                    && vpc.and_then(AwsVpcConfiguration::assign_public_ip)
                        == Some(&AssignPublicIp::Disabled)
                    && request.overrides().map(TaskOverride::container_overrides)
                        == Some(&[expected.clone()])
            })
            .then_output(|| {
                RunTaskOutput::builder()
                    .tasks(Task::builder().task_arn(TASK).build())
                    .build()
            });
        let progress = mock!(Client::describe_tasks)
            .sequence()
            .output(|| DescribeTasksOutput::builder().build())
            .output(|| described("RUNNING", None))
            .output(|| described("STOPPED", Some(0)))
            .build();
        assert_eq!(run(&[&started, &progress], Some(overrides)).await, Ok(()));
        assert_eq!(progress.num_calls(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_container_that_fails_or_never_reports_an_exit_code_fails_the_task() {
        for exit_code in [Some(1), None] {
            let stopped =
                mock!(Client::describe_tasks).then_output(move || described("STOPPED", exit_code));
            assert_eq!(
                run(&[&started(), &stopped], None).await,
                Err("DB_TASK_FAILED")
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_task_that_cannot_be_placed_is_an_error() {
        let refused = mock!(Client::run_task).then_output(|| {
            RunTaskOutput::builder()
                .failures(Failure::builder().reason("RESOURCE:ENI").build())
                .build()
        });
        assert_eq!(run(&[&refused], None).await, Err("AWS_FAILED"));
    }
}
