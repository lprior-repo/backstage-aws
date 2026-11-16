//! Offline release-archive behavior, exercised through the real CLI.
use flate2::{Compression, write::GzEncoder};
use serde_json::Value;
use std::{
    error::Error,
    fs::{self, File},
    io::{self, Cursor, Write},
    path::Path,
    process::{Command, Output},
};
use tar::{Builder, EntryType, Header};

const BINARY: &str = env!("CARGO_BIN_EXE_backstage-aws");
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;

type TestResult = Result<(), Box<dyn Error>>;

struct Harness {
    directory: tempfile::TempDir,
}

impl Harness {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            directory: tempfile::tempdir()?,
        })
    }

    fn run(&self, archive: &Path) -> Result<Output, Box<dyn Error>> {
        let archive = format!("--release-archive={}", archive.display());
        // The CLI extracts into its temporary directory; keep that inside
        // this test's own directory rather than wherever the host puts /tmp.
        Ok(Command::new(BINARY)
            .env_clear()
            .env("TMPDIR", self.directory.path())
            .args(["check", archive.as_str()])
            .output()?)
    }
}

fn append_file(
    builder: &mut Builder<GzEncoder<Vec<u8>>>,
    path: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<(), Box<dyn Error>> {
    let mut header = Header::new_gnu();
    header.set_size(u64::try_from(bytes.len())?);
    header.set_mode(mode);
    header.set_cksum();
    builder.append_data(&mut header, path, Cursor::new(bytes))?;
    Ok(())
}

fn archive_with_files(files: &[(&str, &[u8], u32)]) -> Result<Vec<u8>, Box<dyn Error>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::fast());
    let mut builder = Builder::new(encoder);
    for (path, bytes, mode) in files {
        append_file(&mut builder, path, bytes, *mode)?;
    }
    let encoder = builder.into_inner()?;
    Ok(encoder.finish()?)
}

fn write_archive(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    fs::write(path, bytes)?;
    Ok(())
}

fn document(output: &Output) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn assert_invalid_input(output: &Output, message: &str) -> Result<(), Box<dyn Error>> {
    let json = document(output)?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        json.pointer("/error/code").and_then(Value::as_str),
        Some("INVALID_INPUT")
    );
    let actual = json
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map_or(String::new(), std::string::ToString::to_string);
    assert!(
        actual.contains(message),
        "expected error containing {message:?}, got {actual:?}"
    );
    Ok(())
}

fn assert_precondition_rejection(output: &Output, message: &str) -> Result<(), Box<dyn Error>> {
    let json = document(output)?;
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(json.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        json.pointer("/error/code").and_then(Value::as_str),
        Some("PRECONDITION_FAILED")
    );
    let actual = json
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map_or(String::new(), std::string::ToString::to_string);
    assert!(
        actual.contains(message),
        "expected error containing {message:?}, got {actual:?}"
    );
    Ok(())
}

fn executable_script() -> Vec<u8> {
    format!(
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'backstage-aws {PACKAGE_VERSION}';;\n  --help) printf '%s\\n' 'Backstage on AWS' 'Usage: backstage-aws <COMMAND>';;\n  *) printf '%s\\n' '{{\"ok\":false,\"command\":\"unknown\",\"error\":{{\"code\":\"INVALID_INPUT\"}}}}'; exit 2;;\nesac\n"
    )
    .into_bytes()
}

#[test]
fn release_archive_passes_smoke_checks_when_it_contains_the_current_cli() -> TestResult {
    let harness = Harness::new()?;
    let archive_path = harness.directory.path().join("release.tar.gz");
    let executable = harness.directory.path().join("backstage-aws");
    fs::copy(BINARY, &executable)?;
    let stripped = Command::new("strip").arg("-S").arg(&executable).status()?;
    assert!(stripped.success(), "strip Cargo debug information");
    let archive = archive_with_files(&[("backstage-aws", &fs::read(executable)?, 0o755)])?;
    write_archive(&archive_path, &archive)?;

    let output = harness.run(&archive_path)?;
    let json = document(&output)?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        json.pointer("/command").and_then(Value::as_str),
        Some("check")
    );
    assert_eq!(
        json.pointer("/result/passed")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        json.pointer("/result/passed/0").and_then(Value::as_str),
        Some("release-archive.version")
    );
    assert_eq!(
        json.pointer("/result/passed/1").and_then(Value::as_str),
        Some("release-archive.help")
    );
    assert_eq!(
        json.pointer("/result/passed/2").and_then(Value::as_str),
        Some("release-archive.invalid-command")
    );
    Ok(())
}

#[test]
fn release_archive_rejects_an_empty_archive() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("empty.tar.gz");
    write_archive(&path, &archive_with_files(&[])?)?;
    assert_invalid_input(&harness.run(&path)?, "empty")?;
    Ok(())
}

#[test]
fn release_archive_rejects_an_archive_without_the_executable() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("missing-executable.tar.gz");
    let archive = archive_with_files(&[("other-file", b"not the executable", 0o755)])?;
    write_archive(&path, &archive)?;
    assert_invalid_input(&harness.run(&path)?, "root backstage-aws")?;
    Ok(())
}

#[test]
fn release_archive_rejects_a_traversal_entry() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("traversal.tar.gz");
    let mut builder = Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = Header::new_gnu();
    header.set_size(0);
    header.set_mode(0o755);
    for (target, byte) in header.as_mut_bytes().iter_mut().zip(b"../backstage-aws") {
        *target = *byte;
    }
    header.set_cksum();
    builder.append(&header, io::empty())?;
    let archive = builder.into_inner()?.finish()?;
    write_archive(&path, &archive)?;
    assert_invalid_input(&harness.run(&path)?, "root backstage-aws")?;
    assert!(!harness.directory.path().join("backstage-aws").exists());
    Ok(())
}

#[test]
fn release_archive_rejects_a_symlink_entry() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("symlink.tar.gz");
    let mut builder = Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_link_name("../../outside")?;
    header.set_cksum();
    builder.append_data(&mut header, "backstage-aws", io::empty())?;
    let archive = builder.into_inner()?.finish()?;
    write_archive(&path, &archive)?;
    assert_invalid_input(&harness.run(&path)?, "regular root backstage-aws")?;
    Ok(())
}

#[test]
fn release_archive_rejects_extra_entries() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("extra.tar.gz");
    let archive = archive_with_files(&[
        ("backstage-aws", b"not a binary", 0o755),
        ("unexpected.txt", b"unexpected", 0o644),
    ])?;
    write_archive(&path, &archive)?;
    assert_invalid_input(&harness.run(&path)?, "extra entries")?;
    Ok(())
}

#[test]
fn release_archive_rejects_a_non_executable_file() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("not-executable.tar.gz");
    let archive = archive_with_files(&[("backstage-aws", b"not a binary", 0o644)])?;
    write_archive(&path, &archive)?;
    let output = harness.run(&path)?;
    let json = document(&output)?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json.pointer("/error/code").and_then(Value::as_str),
        Some("INVALID_INPUT")
    );
    assert!(
        json.pointer("/error/message")
            .and_then(Value::as_str)
            .map_or(String::new(), std::string::ToString::to_string)
            .contains("not executable")
    );
    Ok(())
}

#[test]
fn release_archive_rejects_a_wrong_version() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("wrong-version.tar.gz");
    let script = "#!/bin/sh\nprintf '%s\\n' 'backstage-aws 0.0.0'\n";
    let archive = archive_with_files(&[("backstage-aws", script.as_bytes(), 0o755)])?;
    write_archive(&path, &archive)?;
    assert_precondition_rejection(&harness.run(&path)?, "--version did not report")?;
    Ok(())
}

#[test]
fn release_archive_rejects_non_json_invalid_command_output() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("bad-json.tar.gz");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'backstage-aws {PACKAGE_VERSION}';;\n  --help) printf '%s\\n' 'Backstage on AWS' 'Usage: backstage-aws <COMMAND>';;\n  *) printf '%s\\n' 'not-json'; exit 2;;\nesac\n"
    );
    let archive = archive_with_files(&[("backstage-aws", script.as_bytes(), 0o755)])?;
    write_archive(&path, &archive)?;
    assert_precondition_rejection(&harness.run(&path)?, "not JSON")?;
    Ok(())
}

#[test]
fn release_archive_rejects_invalid_command_with_the_wrong_exit_code() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("wrong-exit.tar.gz");
    let script = String::from_utf8(executable_script())?
        .replace("exit 2", "exit 0")
        .into_bytes();
    let archive = archive_with_files(&[("backstage-aws", &script, 0o755)])?;
    write_archive(&path, &archive)?;
    assert_precondition_rejection(&harness.run(&path)?, "exit code 2")?;
    Ok(())
}

#[test]
fn release_archive_rejects_an_oversized_compressed_file() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("oversized.tar.gz");
    let file = File::create(&path)?;
    file.set_len(MAX_ARCHIVE_BYTES.saturating_add(1))?;
    let output = harness.run(&path)?;
    let json = document(&output)?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json.pointer("/error/code").and_then(Value::as_str),
        Some("INVALID_INPUT")
    );
    assert!(
        json.pointer("/error/message")
            .and_then(Value::as_str)
            .map_or(String::new(), std::string::ToString::to_string)
            .contains("archive exceeds")
    );
    Ok(())
}

#[test]
fn release_archive_rejects_an_oversized_declared_binary() -> TestResult {
    let harness = Harness::new()?;
    let path = harness.directory.path().join("oversized-entry.tar.gz");
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut encoder = encoder;
    let mut header = Header::new_gnu();
    header.set_path("backstage-aws")?;
    header.set_size(MAX_BINARY_BYTES.saturating_add(1));
    header.set_mode(0o755);
    header.set_cksum();
    encoder.write_all(header.as_bytes())?;
    let archive = encoder.finish()?;
    write_archive(&path, &archive)?;
    let output = harness.run(&path)?;
    let json = document(&output)?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json.pointer("/error/code").and_then(Value::as_str),
        Some("INVALID_INPUT")
    );
    assert!(
        json.pointer("/error/message")
            .and_then(Value::as_str)
            .map_or(String::new(), std::string::ToString::to_string)
            .contains("exceeds")
    );
    Ok(())
}
