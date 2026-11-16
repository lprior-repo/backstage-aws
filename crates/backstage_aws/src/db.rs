//! One-shot database administration tasks on Fargate.
//!
//! The base stack defines the task definitions (`psql` and `pg_restore`). This
//! module only starts them, with validated non-secret environment overrides,
//! and waits for the result.
mod operation;
pub(crate) use operation::{
    Checkpoint, OperationId, SecretVersionId, SeedDumpKey, TaskArn, TaskAttempt, TaskAttemptId,
    TaskLaunch, TaskTerminal,
};
mod seed;
mod seed_recovery;
pub(crate) use operation::read_checkpoint_body;
pub(crate) use seed::seed;

#[cfg(test)]
pub(crate) mod test_support;

use crate::{
    ctx::{Ctx, poll},
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecs::types::{
    AssignPublicIp, AwsVpcConfiguration, ContainerOverride, KeyValuePair, LaunchType,
    NetworkConfiguration, TaskOverride,
};

use serde_json::Value;
use std::time::Duration;

/// Creates the application role and the empty `seed` database, and sets the
/// role's password from its secret. Safe to repeat.
pub(crate) async fn initialize_database(ctx: &Ctx) -> Result<()> {
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    run_task(ctx, &base, "db_admin_init_task_definition_arn", Vec::new()).await
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
    run_task(ctx, &base, task, vec![overrides]).await
}

/// Runs one db-admin task to completion and requires every container to exit
/// successfully.
pub(crate) async fn run_task(
    ctx: &Ctx,
    base: &Value,
    task_output: &str,
    overrides: Vec<ContainerOverride>,
) -> Result<()> {
    run_task_with_token(ctx, base, task_output, overrides, None).await
}

pub(crate) async fn start_rotation_task(
    ctx: &Ctx,
    base: &Value,
    task_output: &str,
    operation_id: OperationId,
    attempt_id: TaskAttemptId,
) -> Result<TaskArn> {
    let operation_id = operation_id.as_str();
    let pair = || {
        KeyValuePair::builder()
            .name("ROTATION_ID")
            .value(&operation_id)
            .build()
    };
    let secret_reader = ContainerOverride::builder()
        .name("rotation-secret")
        .environment(pair())
        .build();
    let db_admin = ContainerOverride::builder()
        .name("db-admin")
        .environment(pair())
        .build();
    start_tracked_task(
        ctx,
        base,
        task_output,
        vec![secret_reader, db_admin],
        attempt_id,
    )
    .await
}

async fn run_task_with_token(
    ctx: &Ctx,
    base: &Value,
    task_output: &str,
    overrides: Vec<ContainerOverride>,
    client_token: Option<&str>,
) -> Result<()> {
    let arn = start_task(ctx, base, task_output, overrides, client_token).await?;
    let stopped = await_stopped(ctx, base, &arn).await?;
    if task_succeeded(&stopped) {
        return Ok(());
    }
    let reason = stopped.stopped_reason().map_or(
        "a container exited with a non-zero status",
        crate::fallback::pass_through,
    );
    Err(Error::db_task_failed(format!(
        "{} failed ({}); see the db-admin log group",
        arn.as_str(),
        reason
    )))
}

pub(crate) async fn start_tracked_task(
    ctx: &Ctx,
    base: &Value,
    task_output: &str,
    overrides: Vec<ContainerOverride>,
    attempt_id: TaskAttemptId,
) -> Result<TaskArn> {
    let token = attempt_id.as_str();
    start_task(ctx, base, task_output, overrides, Some(&token)).await
}

pub(crate) async fn observe_tracked_task(
    ctx: &Ctx,
    base: &Value,
    arn: &TaskArn,
) -> Result<TaskTerminal> {
    let stopped = await_stopped(ctx, base, arn).await?;
    Ok(if task_succeeded(&stopped) {
        TaskTerminal::Succeeded
    } else {
        TaskTerminal::Failed
    })
}

async fn start_task(
    ctx: &Ctx,
    base: &Value,
    task_output: &str,
    overrides: Vec<ContainerOverride>,
    client_token: Option<&str>,
) -> Result<TaskArn> {
    let cluster = output_str(base, "ecs_cluster_name")?;
    let failed = |detail: String| Error::aws_failed("ecs", "RunTask", detail);
    let started = ctx
        .ecs
        .run_task()
        .cluster(cluster)
        .task_definition(output_str(base, task_output)?)
        .launch_type(LaunchType::Fargate)
        .network_configuration(network(base)?)
        .set_client_token(client_token.map(str::to_owned))
        .overrides(
            TaskOverride::builder()
                .set_container_overrides(Some(overrides))
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
    TaskArn::parse(arn.to_owned())
}

async fn await_stopped(ctx: &Ctx, base: &Value, arn: &TaskArn) -> Result<aws_sdk_ecs::types::Task> {
    observe_stopped(ctx, base, arn).await?.ok_or_else(|| {
        Error::precondition_failed(format!(
            "ECS no longer has task {}; its outcome is unknown, not failed",
            arn.as_str()
        ))
    })
}

async fn observe_stopped(
    ctx: &Ctx,
    base: &Value,
    arn: &TaskArn,
) -> Result<Option<aws_sdk_ecs::types::Task>> {
    let cluster = output_str(base, "ecs_cluster_name")?;
    let describe = || async {
        let request = ctx
            .ecs
            .describe_tasks()
            .cluster(cluster)
            .tasks(arn.as_str());
        let described = request
            .send()
            .await
            .map_err(|error| Error::aws_failed("ecs", "DescribeTasks", error.to_string()))?;
        if described
            .failures()
            .iter()
            .any(|failure| failure.reason() == Some("MISSING"))
        {
            return Ok(Some(None));
        }
        if !described.failures().is_empty() {
            return Err(Error::aws_failed(
                "ecs",
                "DescribeTasks",
                format!("{:?}", described.failures()),
            ));
        }
        let task = described.tasks().first().cloned();
        Ok(task
            .filter(|task| task.last_status() == Some("STOPPED"))
            .map(Some))
    };
    poll(
        "database task",
        arn.as_str(),
        Duration::from_secs(2),
        900,
        describe,
    )
    .await
}

fn task_succeeded(stopped: &aws_sdk_ecs::types::Task) -> bool {
    let containers = stopped.containers();
    !containers.is_empty() && containers.iter().all(|one| one.exit_code() == Some(0))
}

/// Where the base stack says db-admin tasks run. They need a public address
/// to reach AWS APIs exactly when the deployment has no NAT gateway.
fn network(base: &Value) -> Result<NetworkConfiguration> {
    let vpc = base.pointer("/db_admin_network_configuration/awsvpcConfiguration");
    let list = |name: &str| {
        let values = vpc.and_then(|vpc| vpc.get(name)?.as_array());
        let strings = values.into_iter().flatten().filter_map(Value::as_str);
        Some(strings.map(str::to_owned).collect::<Vec<_>>())
    };
    let public = vpc.and_then(|vpc| vpc.get("assignPublicIp")?.as_str());
    let vpc = AwsVpcConfiguration::builder()
        .set_subnets(list("subnets"))
        .set_security_groups(list("securityGroups"))
        .assign_public_ip(AssignPublicIp::from(
            public.map_or("DISABLED", |value| value),
        ))
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
                "assignPublicIp": "ENABLED",
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
        overrides: Vec<ContainerOverride>,
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
    async fn a_task_runs_where_the_base_stack_says_with_its_overrides_until_it_stops() {
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
                        == Some(&AssignPublicIp::Enabled)
                    && request.overrides().map(TaskOverride::container_overrides)
                        == Some(std::slice::from_ref(&expected))
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
        assert_eq!(run(&[&started, &progress], vec![overrides]).await, Ok(()));
        assert_eq!(progress.num_calls(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_container_that_fails_or_never_reports_an_exit_code_fails_the_task() {
        for exit_code in [Some(1), None] {
            let stopped =
                mock!(Client::describe_tasks).then_output(move || described("STOPPED", exit_code));
            assert_eq!(
                run(&[&started(), &stopped], Vec::new()).await,
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
        assert_eq!(run(&[&refused], Vec::new()).await, Err("AWS_FAILED"));
    }
}
