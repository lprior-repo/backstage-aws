//! Offline validation of Terraform stacks and, optionally, the Backstage app.
//! Needs no deployment settings, credentials, or AWS account access.
use crate::{
    error::{Error, Result},
    process,
    terraform::Stack,
};
use serde::Serialize;
use std::{ffi::OsStr, path::Path, time::Duration};

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
const APP_STEPS: [(&str, &[&str]); 4] = [
    ("install", &["yarn", "install", "--immutable"]),
    ("typecheck", &["yarn", "tsc"]),
    ("test", &["yarn", "test", "--watch=false", "--runInBand"]),
    ("build:backend", &["yarn", "build:backend"]),
];
const NODE_OPTIONS: &str = "--no-node-snapshot --max-old-space-size=4096";

#[derive(Serialize)]
pub(crate) struct CheckResult {
    /// Every check that ran and passed, as `<stack>.<step>` or `app.<step>`.
    passed: Vec<String>,
}

pub(crate) enum ApplicationCheck<'a> {
    None,
    Build,
    Runtime { postgres_bin: Option<&'a Path> },
}

pub(crate) enum CheckRequest<'a> {
    Project {
        application: ApplicationCheck<'a>,
        release_archive: Option<&'a Path>,
    },
    Release {
        archive: &'a Path,
    },
}

impl CheckRequest<'_> {
    pub(crate) const fn needs_project(&self) -> bool {
        matches!(self, Self::Project { .. })
    }
}

pub(crate) async fn check(
    project: &Path,
    terraform: &Path,
    request: CheckRequest<'_>,
) -> Result<CheckResult> {
    let mut passed = Vec::new();
    let (application, release_archive) = match request {
        CheckRequest::Release { archive } => {
            passed.extend(crate::release_check::check(archive).await?);
            return Ok(CheckResult { passed });
        }
        CheckRequest::Project {
            application,
            release_archive,
        } => (application, release_archive),
    };
    {
        for stack in std::iter::once("access").chain(Stack::ALL.map(Stack::name)) {
            let dir = project.join("infra").join(stack);
            if !dir.is_dir() {
                return Err(Error::precondition_failed(format!(
                    "{} is not a backstage-aws project: {} is missing",
                    project.display(),
                    dir.display()
                )));
            }
            for (step, args) in STEPS {
                let name = format!("{stack}.{step}");
                process::run(
                    terraform.as_os_str(),
                    args,
                    &dir,
                    None,
                    Duration::from_mins(15),
                )
                .await
                .map_err(|error| Error::tool_failed("terraform", format!("{name}: {error}")))?;
                passed.push(name);
            }
        }
    }
    if let Some(archive) = release_archive {
        passed.extend(crate::release_check::check(archive).await?);
    }
    match application {
        ApplicationCheck::None => {}
        ApplicationCheck::Build => {
            check_app(project, OsStr::new("corepack"), &mut passed).await?;
        }
        ApplicationCheck::Runtime { postgres_bin } => {
            check_app(project, OsStr::new("corepack"), &mut passed).await?;
            passed.extend(crate::local_check::run(project, postgres_bin).await?);
        }
    }
    Ok(CheckResult { passed })
}

async fn check_app(project: &Path, corepack: &OsStr, passed: &mut Vec<String>) -> Result<()> {
    let app_dir = project.join("app");
    if !app_dir.is_dir() {
        return Err(Error::precondition_failed(format!(
            "{} is not a backstage-aws project: {} is missing",
            project.display(),
            app_dir.display()
        )));
    }
    for (step, args) in APP_STEPS {
        let name = format!("app.{step}");
        process::run_with_env(
            corepack,
            args,
            &app_dir,
            None,
            Duration::from_mins(45),
            &[("NODE_OPTIONS", NODE_OPTIONS)],
        )
        .await
        .map_err(|error| Error::tool_failed("application", format!("{name}: {error}")))?;
        passed.push(name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    const FAKE_TERRAFORM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/terraform");

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn project_request() -> CheckRequest<'static> {
        CheckRequest::Project {
            application: ApplicationCheck::None,
            release_archive: None,
        }
    }

    #[tokio::test]
    async fn every_stack_is_checked_and_the_first_failure_stops_the_run() -> TestResult {
        let project = tempfile::tempdir()?;
        for stack in std::iter::once("access").chain(Stack::ALL.map(Stack::name)) {
            fs::create_dir_all(project.path().join("infra").join(stack))?;
        }
        let terraform = Path::new(FAKE_TERRAFORM);
        let passed = check(project.path(), terraform, project_request())
            .await?
            .passed;
        assert_eq!(
            passed,
            [
                "access.fmt",
                "access.init",
                "access.validate",
                "access.test",
                "bootstrap.fmt",
                "bootstrap.init",
                "bootstrap.validate",
                "bootstrap.test",
                "base.fmt",
                "base.init",
                "base.validate",
                "base.test",
                "env.fmt",
                "env.init",
                "env.validate",
                "env.test",
            ]
        );
        let calls = fs::read_to_string(project.path().join("calls.log"))?;
        let expected = ["access", "bootstrap", "base", "env"]
            .into_iter()
            .flat_map(|stack| {
                [
                    "fmt -check -recursive",
                    "init -backend=false -input=false -lockfile=readonly",
                    "validate -no-color",
                    "test -no-color",
                ]
                .map(move |command| format!("{stack}: {command}"))
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.lines().collect::<Vec<_>>(), expected);

        fs::remove_file(project.path().join("calls.log"))?;
        fs::write(project.path().join("fail-validate"), "")?;
        let failed = check(project.path(), terraform, project_request())
            .await
            .map(|result| result.passed);
        assert_eq!(failed.map_err(|error| error.code()), Err("TOOL_FAILED"));
        let calls = fs::read_to_string(project.path().join("calls.log"))?;
        assert_eq!(calls.lines().last(), Some("access: validate -no-color"));
        Ok(())
    }

    #[tokio::test]
    async fn app_checks_run_in_order_with_child_only_node_options_and_stop_on_failure() -> TestResult
    {
        let project = tempfile::tempdir()?;
        let app = project.path().join("app");
        fs::create_dir(&app)?;
        let log = project.path().join("app-calls");
        let corepack = project.path().join("corepack");
        fs::write(
            &corepack,
            format!(
                "#!/bin/sh\nprintf '%s|%s\\n' \"$*\" \"$NODE_OPTIONS\" >> '{}'\ncase \"$*\" in *'test '*) exit 1;; esac\n",
                log.display()
            ),
        )?;
        fs::set_permissions(&corepack, fs::Permissions::from_mode(0o755))?;
        let mut passed = Vec::new();
        let result = check_app(project.path(), corepack.as_os_str(), &mut passed).await;
        assert_eq!(result.map_err(|error| error.code()), Err("TOOL_FAILED"));
        assert_eq!(passed, ["app.install", "app.typecheck"]);
        assert_eq!(
            fs::read_to_string(log)?,
            format!(
                "yarn install --immutable|{NODE_OPTIONS}\nyarn tsc|{NODE_OPTIONS}\nyarn test --watch=false --runInBand|{NODE_OPTIONS}\n"
            )
        );
        Ok(())
    }

    #[tokio::test]
    async fn a_directory_without_the_stacks_is_not_a_project() -> TestResult {
        let empty = tempfile::tempdir()?;
        let refused = check(empty.path(), Path::new(FAKE_TERRAFORM), project_request())
            .await
            .map(|result| result.passed);
        assert_eq!(
            refused.map_err(|error| error.code()),
            Err("PRECONDITION_FAILED")
        );
        Ok(())
    }
}
