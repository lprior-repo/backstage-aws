//! Missing ECS metadata is not a terminal outcome. Reconcile the live database
//! using an operation-bound receipt without repeating restore or cutover.
use super::{
    OperationId, SeedDumpKey, TaskArn, TaskTerminal, observe_stopped, run_task, task_succeeded,
};
use crate::{
    ctx::Ctx,
    error::{Error, Result},
};
use aws_sdk_ecs::types::{ContainerOverride, KeyValuePair};
use serde_json::Value;

const RECONCILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../template/infra/base/scripts/reconcile-seed.sh"
));

pub(super) async fn observe_seed_task(
    ctx: &Ctx,
    base: &Value,
    arn: &TaskArn,
    operation: OperationId,
    key: &SeedDumpKey,
) -> Result<TaskTerminal> {
    if let Some(task) = observe_stopped(ctx, base, arn).await? {
        return Ok(if task_succeeded(&task) {
            TaskTerminal::Succeeded
        } else {
            TaskTerminal::Failed
        });
    }
    let env = |name: &str, value: &str| KeyValuePair::builder().name(name).value(value).build();
    let overrides = vec![
        ContainerOverride::builder()
            .name("download")
            .environment(env("SEED_OBJECT_KEY", key.as_str()))
            .build(),
        ContainerOverride::builder()
            .name("restore")
            .set_command(Some(vec![RECONCILE.to_owned()]))
            .environment(env("SEED_OPERATION_ID", &operation.as_str()))
            .environment(env("SEED_OBJECT_KEY", key.as_str()))
            .build(),
    ];
    run_task(ctx, base, "db_admin_seed_restore_task_definition_arn", overrides)
        .await
        .map_err(|error| Error::precondition_failed(format!(
            "seed operation {} task {} is unavailable; read-only reconciliation did not prove completion; checkpoint and pinned dump retained: {error}",
            operation.as_str(), arn.as_str()
        )))?;
    Ok(TaskTerminal::Succeeded)
}
