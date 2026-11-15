use super::{COMMAND_TIMEOUT, OUTPUT_LIMIT};
use crate::error::{Error, Result};
use std::{path::Path, process::Stdio};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

pub(super) struct Output {
    pub(super) status: std::process::ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

pub(super) async fn invoke(program: &Path, argv: &[String], cwd: &Path) -> Result<Output> {
    let mut command = Command::new(program);
    command
        .args(argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .env("AWS_MAX_ATTEMPTS", "1")
        .env("AWS_RETRY_MODE", "standard")
        .env("AWS_PAGER", "")
        .env("AWS_CLI_AUTO_PROMPT", "off");
    let mut child = command.spawn().map_err(|error| {
        Error::tool_failed(
            program.display().to_string(),
            format!("could not start executable: {error}"),
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::state_failed("child stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::state_failed("child stderr pipe unavailable"))?;
    let read = async {
        let (stdout, stderr, status) =
            tokio::try_join!(read_limited(stdout), read_limited(stderr), child.wait())?;
        Ok::<_, std::io::Error>(Output {
            status,
            stdout,
            stderr,
        })
    };
    timeout(COMMAND_TIMEOUT, read)
        .await
        .map_err(|_| {
            Error::tool_failed(
                program.display().to_string(),
                "command timed out after 20 seconds",
            )
        })?
        .map_err(|error| {
            Error::tool_failed(
                program.display().to_string(),
                format!("command I/O failed: {error}"),
            )
        })
}

async fn read_limited<R: tokio::io::AsyncRead + Unpin>(stream: R) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut bounded = stream.take(1_048_577);
    bounded.read_to_end(&mut bytes).await?;
    if bytes.len() > OUTPUT_LIMIT {
        return Err(std::io::Error::other("child output exceeded 1 MiB limit"));
    }
    Ok(bytes)
}
