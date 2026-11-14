//! Offline validation of the project's three Terraform stacks. Needs no
//! deployment settings, no credentials, and reaches no AWS account.
use crate::{
    error::{Error, Result},
    process,
    terraform::Stack,
};
use serde::Serialize;
use std::{path::Path, time::Duration};

const STEPS: [(&str, &[&str]); 4] = [
    ("fmt", &["fmt", "-check", "-recursive"]),
    (
        "init",
        &[
            "init",
            "-backend=false",
            "-input=false",
            "-lockfile=readonly",
        ],
    ),
    ("validate", &["validate", "-no-color"]),
    ("test", &["test", "-no-color"]),
];

#[derive(Serialize)]
pub(crate) struct CheckResult {
    /// Every check that ran and passed, as `<stack>.<step>`.
    passed: Vec<String>,
}

pub(crate) async fn check(project: &Path, terraform: &Path) -> Result<CheckResult> {
    let mut passed = Vec::new();
    for stack in Stack::ALL {
        let dir = project.join("infra").join(stack.name());
        if !dir.is_dir() {
            return Err(Error::precondition_failed(format!(
                "{} is not a backstage-aws project: {} is missing",
                project.display(),
                dir.display()
            )));
        }
        for (step, args) in STEPS {
            let name = format!("{}.{step}", stack.name());
            let limit = Duration::from_mins(15);
            process::run(terraform.as_os_str(), args, &dir, None, limit)
                .await
                .map_err(|error| Error::tool_failed("terraform", format!("{name}: {error}")))?;
            passed.push(name);
        }
    }
    Ok(CheckResult { passed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const FAKE_TERRAFORM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/terraform");

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn every_stack_is_checked_and_the_first_failure_stops_the_run() -> TestResult {
        let project = tempfile::tempdir()?;
        for stack in Stack::ALL {
            fs::create_dir_all(project.path().join("infra").join(stack.name()))?;
        }
        let terraform = Path::new(FAKE_TERRAFORM);
        let passed = check(project.path(), terraform).await?.passed;
        assert_eq!(passed.len(), 12);
        assert_eq!(passed.first().map(String::as_str), Some("bootstrap.fmt"));
        assert_eq!(passed.last().map(String::as_str), Some("env.test"));

        fs::remove_file(project.path().join("calls.log"))?;
        fs::write(project.path().join("fail-validate"), "")?;
        let failed = check(project.path(), terraform)
            .await
            .map(|result| result.passed);
        assert_eq!(failed.map_err(|error| error.code()), Err("TOOL_FAILED"));
        let calls = fs::read_to_string(project.path().join("calls.log"))?;
        assert_eq!(calls.lines().last(), Some("bootstrap: validate -no-color"));
        Ok(())
    }

    #[tokio::test]
    async fn a_directory_without_the_stacks_is_not_a_project() -> TestResult {
        let empty = tempfile::tempdir()?;
        let refused = check(empty.path(), Path::new(FAKE_TERRAFORM))
            .await
            .map(|result| result.passed);
        assert_eq!(
            refused.map_err(|error| error.code()),
            Err("PRECONDITION_FAILED")
        );
        Ok(())
    }
}
