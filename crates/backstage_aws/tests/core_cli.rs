use serde_json::Value;
use std::{error::Error, process::Command};

const BINARY: &str = env!("CARGO_BIN_EXE_backstage-aws");

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn setup_parser_defaults_are_observed_in_an_environment_cleared_subprocess() -> TestResult {
    let directory = tempfile::tempdir()?;
    let missing_aws = directory.path().join("missing-aws");
    let output = Command::new(BINARY)
        .env_clear()
        .args(["setup", "--aws-cli"])
        .arg(missing_aws)
        .output()?;
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        document.pointer("/error/code"),
        Some(&Value::from("SETUP_BLOCKED"))
    );
    let resume = document
        .pointer("/error/details/resume")
        .and_then(Value::as_array)
        .ok_or("setup response omitted resume arguments")?;
    assert!(resume.iter().any(|argument| argument == "--fck-nat=false"));
    assert!(!resume.iter().any(|argument| argument == "--fck-nat-ami-id"));
    Ok(())
}
