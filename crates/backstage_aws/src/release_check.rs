//! Offline smoke checks for a locally-built native release archive.
//!
//! This checks archive shape and observable CLI behavior only. It does not
//! validate GitHub attestations or any other cryptographic provenance; the
//! installer remains responsible for `gh attestation verify`.
use crate::error::{Error, Result};
use flate2::read::GzDecoder;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tar::Archive;

const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: u64 = MAX_BINARY_BYTES + 64 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const EXECUTABLE_NAME: &str = "backstage-aws";

/// Validates and executes a local release archive without network or AWS access.
pub async fn check(archive: &Path) -> Result<Vec<String>> {
    if !cfg!(any(
        all(
            target_os = "linux",
            target_arch = "x86_64",
            target_env = "gnu"
        ),
        all(target_os = "macos", target_arch = "aarch64")
    )) {
        return Err(Error::precondition_failed(
            "release archive smoke checks currently support Linux x86_64 GNU and macOS arm64 only",
        ));
    }

    let metadata = fs::metadata(archive).map_err(|error| {
        Error::invalid_input(format!(
            "cannot read release archive {}: {error}",
            archive.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(Error::invalid_input(
            "release archive must be a regular file",
        ));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(Error::invalid_input(format!(
            "release archive exceeds the {MAX_ARCHIVE_BYTES}-byte limit"
        )));
    }

    let temp = tempfile::Builder::new()
        .prefix("backstage-aws-release-check-")
        .tempdir()
        .map_err(|error| {
            Error::state_failed(format!("create private release-check directory: {error}"))
        })?;
    let executable = temp.path().join(EXECUTABLE_NAME);
    extract_single_executable(archive, &executable)?;

    let version = run_command(&executable, &["--version"]).await?;
    let expected_version = format!("{EXECUTABLE_NAME} {}", env!("CARGO_PKG_VERSION"));
    let actual_version = String::from_utf8_lossy(&version.stdout);
    let expected_output = format!("{expected_version}\n");
    if !version.status.success() || actual_version.as_ref() != expected_output {
        return Err(Error::precondition_failed(format!(
            "release archive --version did not report {expected_version:?} successfully"
        )));
    }
    let help = run_command(&executable, &["--help"]).await?;
    let help_text = String::from_utf8_lossy(&help.stdout);
    if !help.status.success() || !help_text.contains("Usage: backstage-aws") {
        return Err(Error::precondition_failed(
            "release archive --help did not return the expected successful CLI help",
        ));
    }

    let invalid = run_command(&executable, &["__release-smoke-invalid-command__"]).await?;
    let invalid_json: serde_json::Value =
        serde_json::from_slice(&invalid.stdout).map_err(|error| {
            Error::precondition_failed(format!(
                "release archive invalid-command response is not JSON: {error}"
            ))
        })?;
    if invalid.status.code() != Some(2)
        || invalid_json.get("ok").and_then(serde_json::Value::as_bool) != Some(false)
        || invalid_json
            .pointer("/command")
            .and_then(serde_json::Value::as_str)
            != Some("unknown")
        || invalid_json
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str)
            != Some("INVALID_INPUT")
    {
        return Err(Error::precondition_failed(
            "release archive invalid-command response was not JSON unknown/INVALID_INPUT with exit code 2",
        ));
    }

    Ok(vec![
        "release-archive.version".to_owned(),
        "release-archive.help".to_owned(),
        "release-archive.invalid-command".to_owned(),
    ])
}

fn extract_single_executable(archive_path: &Path, destination: &Path) -> Result<()> {
    let archive_file = File::open(archive_path)
        .map_err(|error| Error::invalid_input(format!("cannot open release archive: {error}")))?;
    let decoded = GzDecoder::new(archive_file).take(MAX_DECOMPRESSED_BYTES);
    let mut archive = Archive::new(decoded);
    let mut entries = archive.entries().map_err(|error| {
        Error::invalid_input(format!("invalid gzip tar release archive: {error}"))
    })?;
    let first = entries
        .next()
        .ok_or_else(|| Error::invalid_input("release archive is empty"))?;
    let mut entry = first
        .map_err(|error| Error::invalid_input(format!("invalid release archive entry: {error}")))?;

    if !entry.header().entry_type().is_file() {
        return Err(Error::invalid_input(
            "release archive must contain only one regular root backstage-aws executable",
        ));
    }
    let path = entry.path_bytes();
    if path.as_ref() != EXECUTABLE_NAME.as_bytes() {
        return Err(Error::invalid_input(
            "release archive entry must be the root backstage-aws executable",
        ));
    }
    let size = entry.header().size().map_err(|error| {
        Error::invalid_input(format!("invalid release executable size: {error}"))
    })?;
    if size > MAX_BINARY_BYTES {
        return Err(Error::invalid_input(format!(
            "release executable exceeds the {MAX_BINARY_BYTES}-byte limit"
        )));
    }
    let mode = entry.header().mode().map_err(|error| {
        Error::invalid_input(format!("invalid release executable mode: {error}"))
    })?;
    if mode & 0o111 == 0 {
        return Err(Error::invalid_input(
            "release archive backstage-aws entry is not executable",
        ));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            Error::state_failed(format!("create extracted release executable: {error}"))
        })?;
    let copied = io::copy(&mut entry, &mut output)
        .map_err(|error| Error::invalid_input(format!("extract release executable: {error}")))?;
    if copied != size {
        return Err(Error::invalid_input(format!(
            "release executable was truncated: expected {size} bytes, extracted {copied}"
        )));
    }
    drop(output);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).map_err(|error| {
            Error::state_failed(format!("mark extracted release executable: {error}"))
        })?;
    }

    drop(entry);

    if entries.next().is_some() {
        return Err(Error::invalid_input(
            "release archive must not contain extra entries",
        ));
    }
    Ok(())
}

struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
}

async fn run_command(executable: &Path, args: &[&str]) -> Result<CommandOutput> {
    let cwd = executable.parent().ok_or_else(|| {
        Error::state_failed("extracted release executable has no parent directory")
    })?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let (status, stdout) =
        crate::process::capture_command(command, COMMAND_TIMEOUT, MAX_COMMAND_OUTPUT_BYTES).await?;
    Ok(CommandOutput { status, stdout })
}
