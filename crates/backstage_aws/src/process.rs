//! Bounded child processes.
//!
//! This program's stdout carries exactly one JSON document, so a child's
//! stdout is either captured or redirected to stderr, never inherited.
use crate::error::{Error, Result};
#[cfg(unix)]
use process_wrap::tokio::ProcessSession;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{
    ffi::OsStr,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{ChildStdout, Command},
    time::{Instant, sleep_until, timeout_at},
};

const CAPTURE_LIMIT: usize = 16 * 1024 * 1024;

enum OutputMode {
    Capture(usize),
    ForwardToStderr,
}

struct ProcessGroupGuard {
    #[cfg(unix)]
    pgid: Option<i32>,
    #[cfg(unix)]
    active: bool,
}

impl ProcessGroupGuard {
    fn new(child: &dyn ChildWrapper) -> Self {
        #[cfg(unix)]
        {
            Self {
                pgid: child.id().and_then(|pid| i32::try_from(pid).ok()),
                active: true,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = child;
            Self {}
        }
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        if self.active {
            if let Some(pgid) = self.pgid
                && let Err(error) = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pgid),
                    nix::sys::signal::Signal::SIGKILL,
                )
                && error != nix::errno::Errno::ESRCH
            {
                // Drop cannot return errors; report an unexpected cleanup failure
                // without panicking if the diagnostic stream is also unavailable.
                drop(std::io::Write::write_fmt(
                    &mut std::io::stderr(),
                    format_args!("process group cleanup failed: {error}\n"),
                ));
            }
            self.active = false;
        }
    }

    const fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.active = false;
        }
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

async fn read_stdout(stdout: ChildStdout, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(limit.min(8192));
    let bounded_limit = u64::try_from(limit.saturating_add(1)).map_err(std::io::Error::other)?;
    let mut bounded = stdout.take(bounded_limit);
    bounded.read_to_end(&mut output).await?;
    if output.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("stdout exceeded the {limit} byte capture limit"),
        ));
    }
    Ok(output)
}

async fn write_stdin(
    stdin: Option<tokio::process::ChildStdin>,
    input: Option<&[u8]>,
) -> std::io::Result<()> {
    match (stdin, input) {
        (Some(mut stdin), Some(bytes)) => stdin.write_all(bytes).await,
        _ => Ok(()),
    }
}

async fn collect_io(
    child: &mut dyn ChildWrapper,
    input: Option<&[u8]>,
    output_mode: &OutputMode,
) -> std::io::Result<Vec<u8>> {
    let stdin = child.stdin().take();
    match output_mode {
        OutputMode::Capture(limit) => {
            let stdout = child.stdout().take().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "child stdout was not piped")
            })?;
            let ((), output) =
                tokio::try_join!(write_stdin(stdin, input), read_stdout(stdout, *limit))?;
            Ok(output)
        }
        OutputMode::ForwardToStderr => {
            write_stdin(stdin, input).await?;
            Ok(Vec::new())
        }
    }
}

pub(crate) struct OwnedChild {
    child: Box<dyn ChildWrapper>,
    group: ProcessGroupGuard,
    finished: bool,
}

impl OwnedChild {
    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.group.terminate();
            self.finished = true;
        }
        Ok(status)
    }
    pub(crate) async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let status = self.child.wait().await?;
        self.group.terminate();
        self.finished = true;
        Ok(status)
    }

    pub(crate) async fn shutdown(&mut self) -> std::io::Result<()> {
        if self.finished || self.try_wait()?.is_some() {
            return Ok(());
        }
        stop_child(self).await.map(drop)
    }
}

pub(crate) fn spawn_owned(command: Command) -> std::io::Result<OwnedChild> {
    let mut wrapped = CommandWrap::from(command);
    wrapped.wrap(KillOnDrop);
    #[cfg(unix)]
    wrapped.wrap(ProcessSession);
    let child = wrapped.spawn()?;
    let group = ProcessGroupGuard::new(child.as_ref());
    Ok(OwnedChild {
        child,
        group,
        finished: false,
    })
}

async fn stop_child(child: &mut OwnedChild) -> std::io::Result<ExitStatus> {
    let kill_result = child.child.start_kill();
    #[cfg(unix)]
    if kill_result.is_err() {
        child.group.terminate();
    } else {
        child.group.disarm();
    }
    #[cfg(not(unix))]
    kill_result?;
    let status = child.child.wait().await?;
    child.group.disarm();
    child.finished = true;
    Ok(status)
}

/// Runs a child with a shared deadline for stdin, stdout, and process completion.
async fn execute(
    mut command: Command,
    input: Option<&[u8]>,
    limit: Duration,
    output_mode: OutputMode,
) -> Result<(ExitStatus, Vec<u8>)> {
    let name = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let failed = |detail: String| Error::tool_failed(name.clone(), detail);
    let deadline = Instant::now()
        .checked_add(limit)
        .ok_or_else(|| failed("process deadline overflow".to_owned()))?;
    let stdout = match &output_mode {
        OutputMode::Capture(_) => Stdio::piped(),
        OutputMode::ForwardToStderr => Stdio::from(std::io::stderr()),
    };
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(stdout);
    let mut child = spawn_owned(command).map_err(|error| failed(error.to_string()))?;
    let io_result = timeout_at(
        deadline,
        collect_io(child.child.as_mut(), input, &output_mode),
    )
    .await;
    let output = match io_result {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let cleanup = stop_child(&mut child).await;
            let detail = cleanup.map_or_else(
                |cleanup| format!("{error}; process cleanup failed: {cleanup}"),
                |_| error.to_string(),
            );
            return Err(failed(detail));
        }
        Err(_) => {
            let cleanup = stop_child(&mut child).await;
            if let Err(error) = cleanup {
                return Err(failed(format!(
                    "timed out and process cleanup failed: {error}"
                )));
            }
            return Err(timeout_error(&failed, limit));
        }
    };
    let mut wait = child.child.wait();
    tokio::select! {
        result = &mut wait => {
            let status = result.map_err(|error| failed(error.to_string()))?;
            child.group.terminate();
            Ok((status, output))
        }
        () = sleep_until(deadline) => {
            child.group.terminate();
            let cleanup = wait.await;
            if let Err(error) = cleanup {
                return Err(failed(format!(
                    "timed out and process cleanup failed: {error}"
                )));
            }
            child.group.disarm();
            Err(timeout_error(&failed, limit))
        }
    }
}

fn timeout_error(failed: &impl Fn(String) -> Error, limit: Duration) -> Error {
    failed(format!(
        "stopped after exceeding its {} s limit",
        limit.as_secs()
    ))
}

/// Runs a child to completion with its stdout streamed to stderr.
pub(crate) async fn run<S: AsRef<OsStr> + Sync>(
    program: &OsStr,
    args: &[S],
    cwd: &Path,
    input: Option<&[u8]>,
    limit: Duration,
) -> Result<()> {
    run_with_env(program, args, cwd, input, limit, &[]).await
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
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .env("TF_IN_AUTOMATION", "1")
        .envs(env.iter().copied());
    let (status, _) = execute(command, input, limit, OutputMode::ForwardToStderr).await?;
    status.success().then_some(()).ok_or_else(|| {
        Error::tool_failed(program.to_string_lossy(), format!("exited with {status}"))
    })
}

/// Captures bounded stdout and returns the raw status, including nonzero exits.
pub(crate) async fn capture_command(
    command: Command,
    limit: Duration,
    stdout_limit: usize,
) -> Result<(ExitStatus, Vec<u8>)> {
    execute(command, None, limit, OutputMode::Capture(stdout_limit)).await
}

/// Runs a child and captures at most 16 MiB of stdout; larger output is rejected.
pub(crate) async fn capture<S: AsRef<OsStr> + Sync>(
    program: &OsStr,
    args: &[S],
    cwd: &Path,
    limit: Duration,
) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .env("TF_IN_AUTOMATION", "1");
    let (status, output) = capture_command(command, limit, CAPTURE_LIMIT).await?;
    status.success().then_some(output).ok_or_else(|| {
        Error::tool_failed(program.to_string_lossy(), format!("exited with {status}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
    type Elapsed = tokio::time::error::Elapsed;

    const LIMIT: Duration = Duration::from_secs(10);

    #[cfg(unix)]
    #[tokio::test]
    async fn owned_children_cannot_inherit_the_callers_terminal_session()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut command = Command::new("sleep");
        command.arg("10");
        let mut child = spawn_owned(command)?;
        let pid = child.child.id().ok_or("child exited before inspection")?;
        let pid = nix::unistd::Pid::from_raw(i32::try_from(pid)?);
        assert_eq!(nix::unistd::getsid(Some(pid))?, pid);
        child.shutdown().await?;
        Ok(())
    }

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
    #[tokio::test]
    async fn capture_accepts_exact_limit_and_rejects_one_extra_byte() {
        let exact = capture(
            OsStr::new("sh"),
            &["-c", "dd if=/dev/zero bs=16384 count=1024 2>/dev/null"],
            Path::new("."),
            LIMIT,
        )
        .await;
        assert_eq!(
            exact.map(|bytes| bytes.len()).map_err(|error| error.code()),
            Ok(CAPTURE_LIMIT)
        );

        let oversized = capture(
            OsStr::new("sh"),
            &[
                "-c",
                "dd if=/dev/zero bs=16384 count=1024 2>/dev/null; printf x",
            ],
            Path::new("."),
            LIMIT,
        )
        .await;
        assert_eq!(oversized.map_err(|error| error.code()), Err("TOOL_FAILED"));
    }

    #[tokio::test]
    async fn raw_capture_preserves_nonzero_status_and_stdout_with_a_clean_environment() -> Result<()>
    {
        let mut command = Command::new("sh");
        command
            .args(["-c", "printf '%s' \"${TF_IN_AUTOMATION-unset}\"; exit 2"])
            .current_dir(".")
            .env_clear()
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        let (status, output) = capture_command(command, LIMIT, 64).await?;
        assert_eq!(status.code(), Some(2));
        assert_eq!(output, b"unset");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn wait_for_pid_file(path: &Path) -> std::result::Result<u32, Elapsed> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(pid) = tokio::fs::read_to_string(path).await
                    && let Ok(pid) = pid.trim().parse::<u32>()
                {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
    }

    #[cfg(target_os = "linux")]
    async fn wait_until_stopped(pid: u32) -> std::result::Result<(), Elapsed> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .and_then(|stat| stat.rsplit_once(')').map(|(_, rest)| rest.to_owned()))
                    .and_then(|rest| rest.split_whitespace().next().map(str::to_owned));
                if matches!(state.as_deref(), None | Some("Z" | "X")) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
    }

    #[cfg(target_os = "linux")]
    fn pid_args(leader: &Path, descendant: &Path, script: &str) -> [String; 5] {
        [
            "-c".to_owned(),
            script.to_owned(),
            "sh".to_owned(),
            leader.to_string_lossy().into_owned(),
            descendant.to_string_lossy().into_owned(),
        ]
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn blocked_stdin_timeout_terminates_direct_child() -> TestResult {
        let dir = tempfile::tempdir()?;
        let pid_path = dir.path().join("pid");
        let args = [
            "-c".to_owned(),
            "echo $$ > \"$1\"; exec sleep 30".to_owned(),
            "sh".to_owned(),
            pid_path.to_string_lossy().into_owned(),
        ];
        let input = vec![b'x'; 4 * 1024 * 1024];
        let task = tokio::spawn(async move {
            run(
                OsStr::new("sh"),
                &args,
                Path::new("."),
                Some(&input),
                Duration::from_secs(1),
            )
            .await
        });
        let pid = wait_for_pid_file(&pid_path).await?;
        let result = tokio::time::timeout(Duration::from_secs(4), task).await??;
        assert_eq!(result.map_err(|error| error.code()), Err("TOOL_FAILED"));
        wait_until_stopped(pid).await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn timeout_kills_descendant_even_after_leader_exits_and_stdout_stays_open() -> TestResult
    {
        let dir = tempfile::tempdir()?;
        let leader_path = dir.path().join("leader");
        let descendant_path = dir.path().join("descendant");
        let args = pid_args(
            &leader_path,
            &descendant_path,
            "echo $$ > \"$1\"; sleep 30 & echo $! > \"$2\"; exit 0",
        );
        let task = tokio::spawn(async move {
            capture(
                OsStr::new("sh"),
                &args,
                Path::new("."),
                Duration::from_millis(150),
            )
            .await
        });
        let leader = wait_for_pid_file(&leader_path).await?;
        let descendant = wait_for_pid_file(&descendant_path).await?;
        let result = tokio::time::timeout(Duration::from_secs(4), task).await??;
        assert_eq!(result.map_err(|error| error.code()), Err("TOOL_FAILED"));
        wait_until_stopped(leader).await?;
        wait_until_stopped(descendant).await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn timeout_terminates_live_leader_and_descendant() -> TestResult {
        let dir = tempfile::tempdir()?;
        let leader_path = dir.path().join("leader");
        let descendant_path = dir.path().join("descendant");
        let args = pid_args(
            &leader_path,
            &descendant_path,
            "echo $$ > \"$1\"; sleep 30 & echo $! > \"$2\"; wait",
        );
        let task = tokio::spawn(async move {
            capture(
                OsStr::new("sh"),
                &args,
                Path::new("."),
                Duration::from_millis(150),
            )
            .await
        });
        let leader = wait_for_pid_file(&leader_path).await?;
        let descendant = wait_for_pid_file(&descendant_path).await?;
        let result = tokio::time::timeout(Duration::from_secs(4), task).await??;
        assert_eq!(result.map_err(|error| error.code()), Err("TOOL_FAILED"));
        wait_until_stopped(leader).await?;
        wait_until_stopped(descendant).await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn caller_cancellation_kills_the_entire_process_group() -> TestResult {
        let dir = tempfile::tempdir()?;
        let leader_path = dir.path().join("leader");
        let descendant_path = dir.path().join("descendant");
        let args = pid_args(
            &leader_path,
            &descendant_path,
            "echo $$ > \"$1\"; sleep 30 & echo $! > \"$2\"; wait",
        );
        let task = tokio::spawn(async move {
            capture(
                OsStr::new("sh"),
                &args,
                Path::new("."),
                Duration::from_secs(30),
            )
            .await
        });
        let leader = wait_for_pid_file(&leader_path).await?;
        let descendant = wait_for_pid_file(&descendant_path).await?;
        task.abort();
        let joined = task
            .await
            .err()
            .ok_or("aborted process task reports cancellation")?;
        assert!(joined.is_cancelled());
        wait_until_stopped(leader).await?;
        wait_until_stopped(descendant).await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn successful_capture_stops_detached_output_descendant() -> TestResult {
        let dir = tempfile::tempdir()?;
        let leader_path = dir.path().join("leader");
        let descendant_path = dir.path().join("descendant");
        let args = pid_args(
            &leader_path,
            &descendant_path,
            "echo $$ > \"$1\"; sleep 30 >/dev/null 2>&1 & echo $! > \"$2\"; printf done",
        );
        let output = capture(OsStr::new("sh"), &args, Path::new("."), LIMIT).await?;
        let descendant = wait_for_pid_file(&descendant_path).await?;
        assert_eq!(output, b"done");
        wait_until_stopped(descendant).await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn owned_child_shutdown_terminates_the_process_group() -> TestResult {
        let dir = tempfile::tempdir()?;
        let leader_path = dir.path().join("leader");
        let descendant_path = dir.path().join("descendant");
        let args = pid_args(
            &leader_path,
            &descendant_path,
            "echo $$ > \"$1\"; sleep 30 & echo $! > \"$2\"; wait",
        );
        let mut command = Command::new("sh");
        command.args(args);
        let mut child = spawn_owned(command)?;
        let leader = wait_for_pid_file(&leader_path).await?;
        let descendant = wait_for_pid_file(&descendant_path).await?;

        tokio::time::timeout(Duration::from_secs(4), child.shutdown()).await??;
        wait_until_stopped(leader).await?;
        wait_until_stopped(descendant).await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn owned_child_try_wait_cleans_descendants_when_leader_exits() -> TestResult {
        let dir = tempfile::tempdir()?;
        let leader_path = dir.path().join("leader");
        let descendant_path = dir.path().join("descendant");
        let args = pid_args(
            &leader_path,
            &descendant_path,
            "echo $$ > \"$1\"; sleep 30 & echo $! > \"$2\"; exit 0",
        );
        let mut command = Command::new("sh");
        command.args(args);
        let mut child = spawn_owned(command)?;
        let leader = wait_for_pid_file(&leader_path).await?;
        let descendant = wait_for_pid_file(&descendant_path).await?;

        let status = tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    return Ok::<_, Box<dyn std::error::Error>>(status);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await??;
        assert!(status.success());
        wait_until_stopped(leader).await?;
        wait_until_stopped(descendant).await?;
        child.shutdown().await?;
        Ok(())
    }
}
