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
use tokio::fs::File;

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
    let key = key_for_operation(
        &ctx.settings.namespace,
        &taken,
        &uuid::Uuid::new_v4().to_string(),
    );
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

/// Streams an object to a private same-directory file, then publishes it
/// atomically without replacing an existing destination.
async fn download(ctx: &Ctx, bucket: &str, key: &str, file: &Path) -> Result<()> {
    ensure_destination_absent(file).await?;
    let object = ctx
        .s3
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|error| Error::aws_failed("s3", "GetObject", error.to_string()))?;
    let mut body = object.body.into_async_read();
    publish_body(&mut body, file).await
}

async fn ensure_destination_absent(file: &Path) -> Result<()> {
    match tokio::fs::symlink_metadata(file).await {
        Ok(_) => Err(Error::invalid_input(format!(
            "{} already exists",
            file.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::tool_failed("dump download", error.to_string())),
    }
}

async fn publish_body(body: &mut (impl tokio::io::AsyncRead + Unpin), file: &Path) -> Result<()> {
    let parent = match file.parent() {
        Some(path) if !path.as_os_str().is_empty() => path,
        _ => Path::new("."),
    };
    let temporary = tempfile::Builder::new()
        .prefix(".backstage-dump-")
        .tempfile_in(parent)
        .map_err(|error| Error::tool_failed("dump download", error.to_string()))?;
    let (temporary_file, temporary_path) = temporary.into_parts();
    let mut target = File::from_std(temporary_file);
    tokio::io::copy(body, &mut target)
        .await
        .map_err(|error| Error::tool_failed("dump download", error.to_string()))?;
    target
        .sync_all()
        .await
        .map_err(|error| Error::tool_failed("dump download", error.to_string()))?;
    drop(target);
    temporary_path
        .persist_noclobber(file)
        .map_err(|error| match error.error.kind() {
            std::io::ErrorKind::AlreadyExists => {
                Error::invalid_input(format!("{} already exists", file.display()))
            }
            _ => Error::tool_failed("dump download", error.error.to_string()),
        })
}

fn key_for_operation(namespace: &str, taken: &str, operation_id: &str) -> String {
    format!("backups/{namespace}/{taken}-{operation_id}.dump")
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{
        io,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll},
    };
    use tokio::io::ReadBuf;

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

    struct FailingBody {
        first: bool,
    }

    impl tokio::io::AsyncRead for FailingBody {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.first {
                self.first = false;
                buffer.put_slice(b"partial");
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(Err(io::Error::other("injected body failure")))
        }
    }

    struct PendingBody {
        first: bool,
        started: Option<tokio::sync::oneshot::Sender<()>>,
    }

    impl tokio::io::AsyncRead for PendingBody {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.first {
                self.first = false;
                if let Some(started) = self.started.take()
                    && started.send(()).is_err()
                {
                    return Poll::Ready(Err(io::Error::other("cancellation observer dropped")));
                }
                buffer.put_slice(b"partial");
                return Poll::Ready(Ok(()));
            }
            Poll::Pending
        }
    }

    #[test]
    fn operation_keys_differ_for_same_namespace_and_fixed_timestamp() {
        let first = key_for_operation("pr-38", "20261115T120000Z", "operation-a");
        let second = key_for_operation("pr-38", "20261115T120000Z", "operation-b");
        assert_eq!(first, "backups/pr-38/20261115T120000Z-operation-a.dump");
        assert_eq!(second, "backups/pr-38/20261115T120000Z-operation-b.dump");
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn get_object_failure_leaves_destination_absent()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        let failed = mock!(aws_sdk_s3::Client::get_object)
            .then_error(|| aws_sdk_s3::operation::get_object::GetObjectError::unhandled("offline"));
        ctx.s3 = mock_client!(aws_sdk_s3, [&failed]);
        let file = dir.path().join("production.dump");
        let result = download(&ctx, "portal-seed", "backups/pr-38/key.dump", &file).await;
        assert_eq!(
            result.map_err(|error| (error.code(), error.exit_code())),
            Err(("AWS_FAILED", 4))
        );
        assert!(!file.exists());
        Ok(())
    }

    #[tokio::test]
    async fn a_body_stream_error_removes_the_partial_file()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("production.dump");
        let mut body = FailingBody { first: true };
        let result = publish_body(&mut body, &file).await;
        assert_eq!(
            result.map_err(|error| (error.code(), error.exit_code())),
            Err(("TOOL_FAILED", 4))
        );
        assert!(!file.exists());
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn a_fallible_sdk_byte_stream_leaves_no_download_file()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("production.dump");
        let mut body = ByteStream::new(aws_smithy_types::body::SdkBody::taken()).into_async_read();
        let result = publish_body(&mut body, &file).await;
        assert_eq!(
            result.map_err(|error| (error.code(), error.exit_code())),
            Err(("TOOL_FAILED", 4))
        );
        assert!(!file.exists());
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn cancelling_a_pending_body_removes_the_temporary_file()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("production.dump");
        let (started, wait_started) = tokio::sync::oneshot::channel();
        let body = PendingBody {
            first: true,
            started: Some(started),
        };
        let task = tokio::spawn(async move {
            let mut body = body;
            publish_body(&mut body, &file).await
        });
        wait_started.await?;
        task.abort();
        let cancelled = task.await;
        assert!(cancelled.is_err_and(|error| tokio::task::JoinError::is_cancelled(&error)));
        assert!(!dir.path().join("production.dump").exists());
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn an_existing_user_file_is_not_overwritten_and_downloads_are_private()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let existing = dir.path().join("existing.dump");
        std::fs::write(&existing, "user file")?;
        let mut body = std::io::Cursor::new(b"replacement".to_vec());
        let result = publish_body(&mut body, &existing).await;
        assert_eq!(
            result.map_err(|error| (error.code(), error.exit_code())),
            Err(("INVALID_INPUT", 2))
        );
        assert_eq!(std::fs::read_to_string(&existing)?, "user file");
        let downloaded = dir.path().join("private.dump");
        let mut body = std::io::Cursor::new(b"PGDMP".to_vec());
        publish_body(&mut body, &downloaded).await?;
        assert_eq!(std::fs::read(&downloaded)?, b"PGDMP");
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&downloaded)?.permissions().mode() & 0o777,
            0o600
        );
        Ok(())
    }

    #[tokio::test]
    async fn dump_request_contains_exact_database_and_operation_key_values()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        let captured_key = Arc::new(Mutex::new(None::<String>));
        let key_for_match = Arc::clone(&captured_key);
        let base = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| request.key() == Some("base/terraform.tfstate"))
            .then_output(|| object(BASE_STATE));
        let uploaded = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| {
                request.bucket() == Some("portal-seed")
                    && request
                        .key()
                        .is_some_and(|key| key.starts_with("backups/pr-38/"))
            })
            .then_output(|| object("PGDMP"));
        ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&base, &uploaded]);
        let started = mock!(aws_sdk_ecs::Client::run_task)
            .match_requests(move |request| {
                let entries: Vec<_> = request
                    .overrides()
                    .into_iter()
                    .flat_map(aws_sdk_ecs::types::TaskOverride::container_overrides)
                    .flat_map(|container| {
                        container
                            .environment()
                            .iter()
                            .map(move |pair| (container.name(), pair.name(), pair.value()))
                    })
                    .collect();
                let database = entries.iter().find(|(_, name, _)| *name == Some("DB_NAME"));
                let backup = entries
                    .iter()
                    .find(|(_, name, _)| *name == Some("BACKUP_KEY"));
                let backup_key = backup.and_then(|(_, _, value)| *value);
                if let Some(key) = backup_key
                    && let Ok(mut captured) = key_for_match.lock()
                {
                    *captured = Some(key.to_owned());
                }
                request.task_definition() == Some("arn:task-definition/portal-db-admin-dump:1")
                    && database == Some(&(Some("dump"), Some("DB_NAME"), Some("pr_38")))
                    && backup.is_some_and(|(container, name, value)| {
                        *container == Some("upload")
                            && *name == Some("BACKUP_KEY")
                            && value.is_some_and(|key| {
                                key.starts_with("backups/pr-38/20")
                                    && std::path::Path::new(key)
                                        .extension()
                                        .is_some_and(|extension| extension == "dump")
                                    && key.contains('-')
                            })
                    })
            })
            .then_output(|| {
                let task = Task::builder()
                    .task_arn("arn:aws:ecs:us-east-1:123456789012:task/portal-cluster/abc")
                    .build();
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
        let captured = captured_key
            .lock()
            .map_err(|_| "captured backup key lock poisoned")?;
        assert_eq!(captured.as_deref(), Some(result.key.as_str()));
        assert_eq!(std::fs::read(&file)?, b"PGDMP");
        Ok(())
    }
}
