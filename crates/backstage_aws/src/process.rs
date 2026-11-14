//! Bounded child processes.
//!
//! This program's stdout carries exactly one JSON document, so a child's
//! stdout is either captured or redirected to stderr, never inherited.
use crate::error::{Error, Result};
use std::{ffi::OsStr, path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

/// Runs a child to completion with its stdout streamed to stderr.
pub(crate) async fn run<S: AsRef<OsStr> + Sync>(
    program: &OsStr,
    args: &[S],
    cwd: &Path,
    input: Option<&[u8]>,
    limit: Duration,
) -> Result<()> {
    let stdout = Stdio::from(std::io::stderr());
    finish(program, args, cwd, input, limit, stdout, &[])
        .await
        .map(drop)
}

/// Runs a child with additional environment values and streams stdout to stderr.
pub(crate) async fn run_with_env<S: AsRef<OsStr> + Sync>(
    program: &OsStr,
    args: &[S],
    cwd: &Path,
    input: Option<&[u8]>,
    limit: Duration,
    env: &[(&str, &str)],
) -> Result<()> {
    finish(
        program,
        args,
        cwd,
        input,
        limit,
        Stdio::from(std::io::stderr()),
        env,
    )
    .await
    .map(drop)
}

/// Runs a child to completion and returns its stdout.
pub(crate) async fn capture<S: AsRef<OsStr> + Sync>(
    program: &OsStr,
    args: &[S],
    cwd: &Path,
    limit: Duration,
) -> Result<Vec<u8>> {
    finish(program, args, cwd, None, limit, Stdio::piped(), &[]).await
}

async fn finish<S: AsRef<OsStr> + Sync>(
    program: &OsStr,
    args: &[S],
    cwd: &Path,
    input: Option<&[u8]>,
    limit: Duration,
    stdout: Stdio,
    env: &[(&str, &str)],
) -> Result<Vec<u8>> {
    let name = program.to_string_lossy().into_owned();
    let failed = |detail: String| Error::tool_failed(name.clone(), detail);
    let mut child_command = Command::new(program);
    child_command
        .args(args)
        .current_dir(cwd)
        .env("TF_IN_AUTOMATION", "1")
        .envs(env.iter().copied())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(stdout)
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut child = child_command
        .spawn()
        .map_err(|error| failed(error.to_string()))?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(bytes)
            .await
            .map_err(|error| failed(error.to_string()))?;
    }
    // Dropping the future on timeout drops the child, which kills it. Nothing
    // continues remotely, so this is a tool failure rather than a timeout.
    let output = timeout(limit, child.wait_with_output())
        .await
        .map_err(|_| {
            failed(format!(
                "stopped after exceeding its {} s limit",
                limit.as_secs()
            ))
        })?
        .map_err(|error| failed(error.to_string()))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(failed(format!("exited with {}", output.status)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn capture_returns_stdout_and_feeds_nothing_on_stdin() -> Result<()> {
        let out = capture(
            OsStr::new("sh"),
            &["-c", "cat; printf ok"],
            Path::new("."),
            LIMIT,
        )
        .await?;
        assert_eq!(out, b"ok");
        Ok(())
    }

    #[tokio::test]
    async fn run_passes_input_on_stdin_and_reports_failure() {
        let script = ["-c", "read line; test \"$line\" = secret"];
        let ok = run(
            OsStr::new("sh"),
            &script,
            Path::new("."),
            Some(b"secret\n"),
            LIMIT,
        )
        .await;
        assert!(ok.is_ok());
        let bad = run(
            OsStr::new("sh"),
            &script,
            Path::new("."),
            Some(b"other\n"),
            LIMIT,
        )
        .await;
        assert_eq!(bad.map_err(|error| error.code()), Err("TOOL_FAILED"));
    }

    #[tokio::test]
    async fn a_child_that_outlives_its_limit_is_stopped_and_reported_as_a_failure() {
        let limit = Duration::from_millis(50);
        let result = capture(OsStr::new("sleep"), &["5"], Path::new("."), limit).await;
        let error = result
            .map(drop)
            .map_err(|error| (error.exit_code(), error.to_string()));
        let stopped = "sleep: stopped after exceeding its 0 s limit".to_owned();
        assert_eq!(error, Err((4, stopped)));
    }
}
