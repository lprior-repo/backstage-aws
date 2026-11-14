//! `db dump`: an on-demand `pg_dump` of the namespace's database.
//!
//! The database accepts connections only from inside the VPC, so the dump
//! runs there as a Fargate task, which stores it in the seed bucket.
use crate::{
    ctx::Ctx,
    db,
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecs::types::{ContainerOverride, KeyValuePair};
use aws_sdk_s3::primitives::{DateTime, DateTimeFormat};
use serde::Serialize;
use std::{path::Path, time::SystemTime};
use tokio::fs::OpenOptions;

#[derive(Serialize)]
pub(crate) struct DumpResult {
    bucket: String,
    key: String,
    database: String,
    /// Where the dump was downloaded to, when `--file` was given.
    file: Option<String>,
}

pub(crate) async fn dump(ctx: &Ctx, file: Option<&Path>) -> Result<DumpResult> {
    ctx.require_approval()?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let bucket = output_str(&base, "seed_bucket_name")?.to_owned();
    let database = ctx.settings.database_name().to_owned();
    // Whole seconds, as 2026-01-31T120000Z: colons are awkward in shells.
    let taken = DateTime::from_secs(DateTime::from(SystemTime::now()).secs())
        .fmt(DateTimeFormat::DateTime)
        .map_err(|error| Error::state_failed(error.to_string()))?
        .replace(':', "");
    let key = format!("backups/{}/{taken}.dump", ctx.settings.namespace);
    let set = |container: &str, name: &str, value: &str| {
        let pair = KeyValuePair::builder().name(name).value(value).build();
        ContainerOverride::builder()
            .name(container)
            .environment(pair)
            .build()
    };
    let overrides = vec![
        set("dump", "DB_NAME", &database),
        set("upload", "BACKUP_KEY", &key),
    ];
    db::run_task(ctx, &base, "db_admin_dump_task_definition_arn", overrides).await?;
    if let Some(file) = file {
        download(ctx, &bucket, &key, file).await?;
    }
    Ok(DumpResult {
        bucket,
        key,
        database,
        file: file.map(|file| file.display().to_string()),
    })
}

/// Streams the object to a new file only its owner can read: a dump holds
/// everything in the database. An existing file is never overwritten.
async fn download(ctx: &Ctx, bucket: &str, key: &str, file: &Path) -> Result<()> {
    let failed =
        |error: std::io::Error| Error::invalid_input(format!("{}: {error}", file.display()));
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)
        .await
        .map_err(failed)?;
    let object = ctx
        .s3
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|error| Error::aws_failed("s3", "GetObject", error.to_string()))?;
    let mut body = object.body.into_async_read();
    tokio::io::copy(&mut body, &mut target)
        .await
        .map_err(failed)?;
    target.sync_all().await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_ecs::{
        operation::{describe_tasks::DescribeTasksOutput, run_task::RunTaskOutput},
        types::{Container, Task},
    };
    use aws_sdk_s3::{operation::get_object::GetObjectOutput, primitives::ByteStream};
    use aws_smithy_mocks::{RuleMode, mock, mock_client};

    const BASE_STATE: &str = r#"{"outputs":{
        "ecs_cluster_name":{"value":"portal-cluster"},
        "seed_bucket_name":{"value":"portal-seed"},
        "db_admin_dump_task_definition_arn":{"value":"arn:task-definition/portal-db-admin-dump:1"},
        "db_admin_network_configuration":{"value":{"awsvpcConfiguration":{
            "subnets":["subnet-a"],"securityGroups":["sg-admin"],"assignPublicIp":"ENABLED"}}}}}"#;

    fn object(body: &'static str) -> GetObjectOutput {
        GetObjectOutput::builder()
            .body(ByteStream::from_static(body.as_bytes()))
            .build()
    }

    #[tokio::test]
    async fn a_dump_names_the_namespace_database_and_downloads_what_the_task_uploaded()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        let base = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| request.key() == Some("base/terraform.tfstate"))
            .then_output(|| object(BASE_STATE));
        let uploaded = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| {
                let key = request.key().unwrap_or_default();
                request.bucket() == Some("portal-seed") && key.starts_with("backups/pr-38/")
            })
            .then_output(|| object("PGDMP"));
        ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&base, &uploaded]);
        let started = mock!(aws_sdk_ecs::Client::run_task)
            .match_requests(|request| {
                let overrides = request.overrides().map(|all| all.container_overrides());
                let set: Vec<_> = overrides
                    .into_iter()
                    .flatten()
                    .map(|one| (one.name(), one.environment().first()))
                    .map(|(container, pair)| (container, pair.and_then(KeyValuePair::name)))
                    .collect();
                request.task_definition() == Some("arn:task-definition/portal-db-admin-dump:1")
                    && set
                        == [
                            (Some("dump"), Some("DB_NAME")),
                            (Some("upload"), Some("BACKUP_KEY")),
                        ]
            })
            .then_output(|| {
                let task = Task::builder().task_arn("arn:task/abc").build();
                RunTaskOutput::builder().tasks(task).build()
            });
        let stopped = mock!(aws_sdk_ecs::Client::describe_tasks).then_output(|| {
            let containers = [0, 0].map(|code| Container::builder().exit_code(code).build());
            let task = Task::builder()
                .last_status("STOPPED")
                .set_containers(Some(containers.into()));
            DescribeTasksOutput::builder().tasks(task.build()).build()
        });
        ctx.ecs = mock_client!(aws_sdk_ecs, RuleMode::MatchAny, [&started, &stopped]);
        let file = dir.path().join("production.dump");
        let result = dump(&ctx, Some(&file)).await?;
        assert_eq!(result.database, "pr_38");
        assert!(result.key.starts_with("backups/pr-38/20"), "{}", result.key);
        assert!(result.key.ends_with("Z.dump"), "{}", result.key);
        assert_eq!(std::fs::read_to_string(&file)?, "PGDMP");
        // A second dump to the same path must not replace the first.
        let again = dump(&ctx, Some(&file)).await;
        assert_eq!(
            again.map(drop).map_err(|error| error.code()),
            Err("INVALID_INPUT")
        );
        Ok(())
    }
}
