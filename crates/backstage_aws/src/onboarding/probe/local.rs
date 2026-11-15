use super::{action_required, invoke, verified};
use crate::{
    error::{Error, Result},
    settings::Settings,
};
use serde_json::Value;
use std::path::Path;

pub(super) fn project(project: &Path) -> Result<std::result::Result<super::Check, super::Check>> {
    match std::fs::metadata(project) {
        Ok(metadata) if metadata.is_dir() => (),
        Ok(_) => {
            return Ok(Err(action_required(
                super::Stage::Project,
                format!(
                    "Project path {} is not a directory; choose a new empty project directory.",
                    project.display()
                ),
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Err(action_required(
                super::Stage::Project,
                format!(
                    "Project directory {} does not exist; run `backstage-aws init --output {}` before setup.",
                    project.display(),
                    project.display()
                ),
            )));
        }
        Err(error) => {
            return Err(Error::tool_failed(
                project.display().to_string(),
                format!("could not inspect project directory: {error}"),
            ));
        }
    }
    let missing = ["infra/bootstrap", "infra/base", "infra/env"]
        .into_iter()
        .map(|part| directory_exists(&project.join(part)).map(|exists| (part, exists)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|(part, exists)| if exists { None } else { Some(part) })
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Ok(Err(action_required(
            super::Stage::Project,
            format!(
                "Project is missing required generated directories: {}. Choose a new empty directory and initialize the project; setup will not overwrite files in the configured directory.",
                missing.join(", ")
            ),
        )));
    }
    Ok(Ok(verified(
        super::Stage::Project,
        "Generated Terraform directories are present.",
    )))
}

fn directory_exists(path: &Path) -> Result<bool> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::tool_failed(
            path.display().to_string(),
            format!("could not inspect generated directory: {error}"),
        )),
    }
}

pub(super) async fn tools(
    settings: &Settings,
) -> Result<std::result::Result<super::Check, super::Check>> {
    let terraform = settings.terraform_path();
    let terraform_version = match run(&terraform, &["version", "-json"], Path::new(".")).await {
        Ok(bytes) => parse_version(&bytes)?,
        Err(Error::ToolFailed { detail, .. }) if unavailable_tool(&detail) => {
            return Ok(Err(action_required(
                super::Stage::LocalTools,
                format!(
                    "Terraform 1.17.2 is required; install it or pass --terraform (configured executable: {}).",
                    terraform.display()
                ),
            )));
        }
        Err(error) => return Err(error),
    };
    if terraform_version != "1.17.2" {
        return Ok(Err(action_required(
            super::Stage::LocalTools,
            format!("Terraform 1.17.2 is required; found {terraform_version}."),
        )));
    }
    if let Some(check) =
        check_tool("docker", &["buildx", "version"], "Docker Buildx", settings).await?
    {
        return Ok(Err(check));
    }
    if let Some(check) = check_tool(
        "docker",
        &["info", "--format", "{{.ServerVersion}}"],
        "Docker engine",
        settings,
    )
    .await?
    {
        return Ok(Err(check));
    }
    if let Some(check) = check_tool("git", &["--version"], "Git", settings).await? {
        return Ok(Err(check));
    }
    Ok(Ok(verified(
        super::Stage::LocalTools,
        "Terraform 1.17.2, Docker Buildx, Docker engine, and Git are available.",
    )))
}

async fn check_tool(
    program: &str,
    argv: &[&str],
    label: &str,
    settings: &Settings,
) -> Result<Option<super::Check>> {
    match run(Path::new(program), argv, &settings.project).await {
        Ok(_) => Ok(None),
        Err(Error::ToolFailed { detail, .. }) if unavailable_tool(&detail) => {
            Ok(Some(action_required(
                super::Stage::LocalTools,
                format!("{label} is required and must be available in this environment."),
            )))
        }
        Err(error) => Err(error),
    }
}

fn unavailable_tool(detail: &str) -> bool {
    detail.starts_with("could not start executable:")
        || detail == "local prerequisite command failed"
}

async fn run(program: &Path, argv: &[&str], cwd: &Path) -> Result<Vec<u8>> {
    let arguments = argv
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    let output = invoke(program, &arguments, cwd).await?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(Error::tool_failed(
            program.display().to_string(),
            "local prerequisite command failed",
        ))
    }
}

fn parse_version(bytes: &[u8]) -> Result<String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::state_failed("Terraform returned malformed version JSON"))?;
    let object = super::parse::object(
        &value,
        &[
            "terraform_version",
            "platform",
            "provider_selections",
            "format_version",
            "development",
            "target",
        ],
        "Terraform version",
    )?;
    let version = super::parse::string(object, "terraform_version", "Terraform version")?;
    Ok(version.to_owned())
}
