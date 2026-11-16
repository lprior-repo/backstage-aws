//! The wire contract, exercised through the real binary without AWS: one JSON
//! document on stdout, and the documented exit code for each kind of outcome.
use serde_json::Value;
use std::{error::Error, path::Path, process::Command};

const BINARY: &str = env!("CARGO_BIN_EXE_backstage-aws");
const FAKE_TERRAFORM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/terraform");
const SETTINGS: [&str; 7] = [
    "--name=portal",
    "--aws-account-id=123456789012",
    "--aws-region=us-east-1",
    "--domain-name=preview.example.com",
    "--route53-zone-id=Z123456789ABC",
    "--github-repository=example/portal",
    "--owner-github-id=123456",
];

type TestResult = Result<(), Box<dyn Error>>;

struct Outcome {
    exit: Option<i32>,
    ok: Option<bool>,
    command: Option<String>,
    code: Option<String>,
    result: Value,
}

/// Runs the binary with a clean environment. Parsing fails unless stdout is
/// exactly one JSON document.
fn run(args: &[&str]) -> Result<Outcome, Box<dyn Error>> {
    let output = Command::new(BINARY).args(args).env_clear().output()?;
    let document: Value = serde_json::from_slice(&output.stdout)?;
    let text = |pointer: &str| {
        document
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Ok(Outcome {
        exit: output.status.code(),
        ok: document.get("ok").and_then(Value::as_bool),
        command: text("/command"),
        code: text("/error/code"),
        result: document.get("result").cloned().unwrap_or(Value::Null),
    })
}

/// A directory holding a Terraform stand-in that reports `version`.
fn terraform(version: &str) -> Result<(tempfile::TempDir, String), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let link = dir.path().join(format!("terraform-{version}"));
    std::os::unix::fs::symlink(FAKE_TERRAFORM, &link)?;
    let flag = format!("--terraform={}", link.display());
    Ok((dir, flag))
}

fn failure(outcome: &Outcome) -> (Option<i32>, Option<bool>, Option<&str>) {
    (outcome.exit, outcome.ok, outcome.code.as_deref())
}

#[test]
fn bad_input_exits_2_with_a_json_error() -> TestResult {
    let unknown = run(&["frobnicate"])?;
    assert_eq!(
        failure(&unknown),
        (Some(2), Some(false), Some("INVALID_INPUT"))
    );
    assert_eq!(unknown.command.as_deref(), Some("unknown"));

    let missing_settings = run(&["env", "status"])?;
    assert_eq!(
        failure(&missing_settings),
        (Some(2), Some(false), Some("INVALID_INPUT"))
    );
    assert_eq!(missing_settings.command.as_deref(), Some("env status"));

    let mut reserved = SETTINGS.to_vec();
    reserved.extend(["--namespace=postgres", "env", "up", "--approve"]);
    assert_eq!(
        failure(&run(&reserved)?),
        (Some(2), Some(false), Some("INVALID_INPUT"))
    );
    Ok(())
}

#[test]
fn every_remote_mutation_without_approval_makes_no_changes() -> TestResult {
    let (_dir, terraform) = terraform("1.17.2")?;
    let scratch = tempfile::tempdir()?;
    let project_flag = format!("--project={}", scratch.path().display());
    let dump = scratch.path().join("dump");
    let dump_file = format!("--file={}", dump.display());
    let mutations: [&[&str]; 11] = [
        &["bootstrap"],
        &["base"],
        &["seed", "--file=/nonexistent"],
        &["build"],
        &["env", "up"],
        &["env", "down"],
        &["secrets", "set-identity"],
        &["db", "rotate-password"],
        &["db", "dump", dump_file.as_str()],
        &["publish"],
        &["up"],
    ];
    for mutation in mutations {
        let mut args = SETTINGS.to_vec();
        args.push(&terraform);
        args.push(&project_flag);
        args.extend(mutation);
        let refused = run(&args)?;
        assert_eq!(
            failure(&refused),
            (Some(3), Some(false), Some("APPROVAL_REQUIRED")),
            "{mutation:?}"
        );
        assert!(!dump.exists(), "{mutation:?}");
        assert_eq!(
            std::fs::read_dir(scratch.path())?.count(),
            0,
            "{mutation:?}"
        );
    }
    Ok(())
}

#[test]
fn any_other_terraform_version_is_a_failed_precondition() -> TestResult {
    let (_dir, terraform) = terraform("1.9.0")?;
    let mut args = SETTINGS.to_vec();
    args.extend([terraform.as_str(), "env", "up", "--approve"]);
    assert_eq!(
        failure(&run(&args)?),
        (Some(3), Some(false), Some("PRECONDITION_FAILED"))
    );
    Ok(())
}

#[test]
fn check_needs_no_settings_and_init_writes_a_project_once() -> TestResult {
    let (_dir, terraform) = terraform("1.17.2")?;
    let scratch = tempfile::tempdir()?;
    let project = scratch.path().join("project");
    let output = format!("--output={}", project.display());

    let mut init = SETTINGS.to_vec();
    init.extend(["init", output.as_str()]);
    let created = run(&init)?;
    assert_eq!((created.exit, created.ok), (Some(0), Some(true)));
    assert!(created.result.get("files").and_then(Value::as_u64) > Some(50));
    assert!(Path::new(&project).join("infra/env/main.tf").is_file());
    assert_eq!(
        failure(&run(&init)?),
        (Some(2), Some(false), Some("INVALID_INPUT"))
    );

    let project_flag = format!("--project={}", project.display());
    let checked = run(&["check", project_flag.as_str(), terraform.as_str()])?;
    assert_eq!((checked.exit, checked.ok), (Some(0), Some(true)));
    assert_eq!(
        checked.result.pointer("/passed/15"),
        Some(&Value::from("env.test"))
    );

    let elsewhere = format!("--project={}", scratch.path().display());
    let not_a_project = run(&["check", elsewhere.as_str(), terraform.as_str()])?;
    assert_eq!(
        failure(&not_a_project),
        (Some(3), Some(false), Some("PRECONDITION_FAILED"))
    );
    Ok(())
}
