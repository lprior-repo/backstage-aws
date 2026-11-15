//! Given account evidence, setup reports one truthful next action without mutation.
use serde_json::Value;
use std::{error::Error, path::Path, process::Command};

type TestResult = Result<(), Box<dyn Error>>;
const BINARY: &str = env!("CARGO_BIN_EXE_backstage-aws");

fn setup(aws: &Path, extra: &[&str]) -> Result<(Option<i32>, Value), Box<dyn Error>> {
    let output = Command::new(BINARY)
        .env_clear()
        .args(extra)
        .args(["setup", "--profile", "sandbox", "--aws-cli"])
        .arg(aws)
        .output()?;
    Ok((
        output.status.code(),
        serde_json::from_slice(&output.stdout)?,
    ))
}

fn fake_aws(path: &Path, response: &str, exit: u8) -> TestResult {
    let body = format!(
        "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo 'aws-cli/2.40.3 Python/3 Linux'; exit 0; fi\nif [ {exit} -eq 0 ]; then printf '%s\\n' '{response}'; else printf '%s\\n' '{response}' >&2; fi\nexit {exit}\n"
    );
    std::fs::write(path.with_file_name("aws-response.sh"), body)?;
    if !path.exists() {
        std::os::unix::fs::symlink(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/probe-aws"),
            path,
        )?;
    }
    Ok(())
}

#[test]
fn missing_aws_needs_no_settings_or_terraform() -> TestResult {
    // Given no tools, credentials, project or deployment settings.
    let directory = tempfile::tempdir()?;
    // When discovery runs through the real binary.
    let (exit, result) = setup(&directory.path().join("missing-aws"), &[])?;
    // Then the actionable first blocker is AWS CLI, not invalid deployment settings.
    assert_eq!(exit, Some(3));
    assert_eq!(
        result.pointer("/error/code"),
        Some(&Value::from("SETUP_BLOCKED"))
    );
    assert_eq!(
        result.pointer("/error/details/checks/0/stage"),
        Some(&Value::from("aws_cli"))
    );
    assert_eq!(result.get("ok"), Some(&Value::Bool(false)));
    Ok(())
}

#[test]
fn resume_rediscovers_credentials_and_rejects_a_changed_account() -> TestResult {
    let directory = tempfile::tempdir()?;
    let aws = directory.path().join("aws");
    // Given missing credentials, preserve the chosen profile in the resume command.
    fake_aws(
        &aws,
        "Unable to locate credentials. You can configure credentials by running aws configure.",
        253,
    )?;
    let (exit, result) = setup(&aws, &["--aws-account-id", "123456789012"])?;
    assert_eq!(exit, Some(3));
    let resume = result
        .pointer("/error/details/resume")
        .and_then(Value::as_array)
        .ok_or("missing resume argv")?;
    assert!(resume.iter().any(|value| value == "sandbox"));
    assert_eq!(
        result.pointer("/error/details/checks/1/stage"),
        Some(&Value::from("credentials"))
    );
    // When the same executable now reports a different signed-in account.
    fake_aws(
        &aws,
        r#"{"Account":"999999999999","Arn":"arn:aws:sts::999999999999:assumed-role/Developer/session","UserId":"AROA:session"}"#,
        0,
    )?;
    let argv = resume
        .iter()
        .skip(1)
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
        .ok_or("resume contains non-string argv")?;
    let replay = Command::new(BINARY).env_clear().args(argv).output()?;
    let exit = replay.status.code();
    let result: Value = serde_json::from_slice(&replay.stdout)?;
    // Then discovery advances past credentials, but never accepts the wrong account.
    assert_eq!(exit, Some(3));
    assert_eq!(
        result.pointer("/error/details/checks/1/status"),
        Some(&Value::from("verified"))
    );
    assert_ne!(
        result.pointer("/error/details/checks/2/status"),
        Some(&Value::from("verified"))
    );
    assert_eq!(
        result.pointer("/error/details/status"),
        Some(&Value::from("blocked"))
    );
    Ok(())
}

#[test]
fn malformed_success_is_an_external_failure_not_a_setup_instruction() -> TestResult {
    // Given a broken AWS response that exits successfully.
    let directory = tempfile::tempdir()?;
    let aws = directory.path().join("aws");
    fake_aws(&aws, "not JSON", 0)?;
    // When the real binary parses identity evidence.
    let (exit, result) = setup(&aws, &[])?;
    // Then it fails closed, without inventing missing credentials or account state.
    assert_eq!(exit, Some(4));
    assert_ne!(
        result.pointer("/error/code"),
        Some(&Value::from("SETUP_BLOCKED"))
    );
    assert_eq!(result.get("ok"), Some(&Value::Bool(false)));
    Ok(())
}

#[test]
fn unknown_aws_failure_does_not_expose_raw_diagnostics() -> TestResult {
    // Given an unexpected provider error containing private diagnostic data.
    let directory = tempfile::tempdir()?;
    let aws = directory.path().join("aws");
    fake_aws(
        &aws,
        "An error occurred (PRIVATE_DIAGNOSTIC_123) when calling GetCallerIdentity: private detail",
        254,
    )?;
    // When discovery fails, neither stdout nor stderr may disclose that body.
    let output = Command::new(BINARY)
        .env_clear()
        .args(["setup", "--aws-cli"])
        .arg(&aws)
        .output()?;
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(document.get("ok"), Some(&Value::Bool(false)));
    assert!(!String::from_utf8(output.stdout)?.contains("PRIVATE_DIAGNOSTIC_123"));
    assert!(!String::from_utf8(output.stderr)?.contains("PRIVATE_DIAGNOSTIC_123"));
    Ok(())
}

#[test]
fn complete_evidence_is_ready_but_manual_checks_remain_attestations() -> TestResult {
    // Given complete external evidence and a generated-project layout.
    let directory = tempfile::tempdir()?;
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin)?;
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    ["terraform", "dig", "docker", "git"]
        .into_iter()
        .try_for_each(|tool| {
            std::os::unix::fs::symlink(fixtures.join("setup-tool"), bin.join(tool))
        })?;
    ["infra/bootstrap", "infra/base", "infra/env"]
        .into_iter()
        .try_for_each(|part| std::fs::create_dir_all(directory.path().join(part)))?;
    // When the owner explicitly attests both console-only checks.
    let output = Command::new(BINARY)
        .env_clear()
        .env("PATH", &bin)
        .args([
            "--aws-account-id",
            "123456789012",
            "--domain-name",
            "preview.example.com",
            "--name",
            "portal",
            "--aws-region",
            "us-east-1",
            "--route53-zone-id",
            "Z123456789ABC",
            "--github-repository",
            "example/portal",
            "--owner-github-id",
            "123456",
            "--allowed-ingress-cidrs",
            "203.0.113.1/32",
            "--project",
        ])
        .arg(directory.path())
        .arg("--terraform")
        .arg(bin.join("terraform"))
        .args([
            "setup",
            "--confirm",
            "organization-email",
            "--confirm",
            "identity-center-mfa",
            "--aws-cli",
        ])
        .arg(fixtures.join("setup-aws"))
        .output()?;
    let document: Value = serde_json::from_slice(&output.stdout)?;
    // Then readiness is only for doctor; manual gates are not API-verified.
    assert_eq!(output.status.code(), Some(0), "{document}");
    assert_eq!(
        document.pointer("/result/status"),
        Some(&Value::from("ready_for_doctor"))
    );
    let checks = document
        .pointer("/result/checks")
        .and_then(Value::as_array)
        .ok_or("missing checks")?;
    assert_eq!(checks.len(), 13);
    let manual = checks
        .iter()
        .filter(|check| check.get("status") == Some(&Value::from("acknowledged")))
        .count();
    assert_eq!(manual, 2);
    assert!(checks.iter().all(|check| matches!(
        check.get("status").and_then(Value::as_str),
        Some("verified" | "acknowledged")
    )));
    Ok(())
}

#[test]
fn invalid_network_settings_fail_before_aws_discovery() -> TestResult {
    // Given no AWS executable and an explicitly conflicting network choice.
    let directory = tempfile::tempdir()?;
    let absent = directory.path().join("aws-must-not-run");
    // When setup receives these inputs, reject them without needing credentials.
    let (exit, document) = setup(&absent, &["--fck-nat", "true", "--nat-gateways", "1"])?;
    assert_eq!(exit, Some(2));
    assert_eq!(
        document.pointer("/error/code"),
        Some(&Value::from("INVALID_INPUT"))
    );
    assert!(
        document
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("cannot be combined"))
    );
    let (exit, document) = setup(&absent, &["--fck-nat", "true", "--fck-nat-ami-id", " "])?;
    assert_eq!(exit, Some(2));
    assert_eq!(
        document.pointer("/error/code"),
        Some(&Value::from("INVALID_INPUT"))
    );
    Ok(())
}
