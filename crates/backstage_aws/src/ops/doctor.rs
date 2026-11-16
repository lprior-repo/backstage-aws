//! Preflight for an operator workstation: tools, AWS identity, GitHub
//! permission. Changes nothing.
use crate::{
    container::ContainerEngine,
    ctx::Ctx,
    error::{Error, Result},
    github, process, terraform,
};
use serde::Serialize;
use std::{ffi::OsStr, path::Path, time::Duration};

#[derive(Serialize)]
pub(crate) struct DoctorResult {
    account: String,
    arn: String,
    /// False when `publish` still has to create the repository.
    repository_exists: bool,
}

pub(crate) async fn doctor(ctx: &Ctx) -> Result<DoctorResult> {
    terraform::verify_version(&ctx.terraform).await?;
    let engine = ctx.settings.settings.container_engine;
    let engine_args: &[&str] = match engine {
        ContainerEngine::Docker => &["buildx", "version"],
        ContainerEngine::Podman => &["info"],
    };
    for (tool, args) in [
        (engine.program(), engine_args),
        ("git", &["version", "--build-options"]),
    ] {
        let limit = Duration::from_secs(20);
        process::capture(OsStr::new(tool), args, Path::new("."), limit)
            .await
            .map_err(|_| {
                Error::precondition_failed(format!("`{tool} {}` does not run", args.join(" ")))
            })?;
    }
    let identity = ctx
        .sts
        .get_caller_identity()
        .send()
        .await
        .map_err(|error| Error::aws_failed("sts", "GetCallerIdentity", error.to_string()))?;
    let account = identity.account().unwrap_or_default().to_owned();
    let arn = identity.arn().unwrap_or_default().to_owned();
    let expected = &ctx.settings.settings.aws_account_id;
    if &account != expected {
        return Err(Error::precondition_failed(format!(
            "credentials are for account {account}, but this project targets {expected}"
        )));
    }
    if arn.ends_with(":root") {
        return Err(Error::precondition_failed(
            "root credentials are refused; use short-lived role credentials",
        ));
    }
    let repository = &ctx.settings.settings.github_repository;
    let administers = github::administers(&github::client()?, repository).await?;
    if administers == Some(false) {
        return Err(Error::precondition_failed(format!(
            "the GitHub token lacks admin permission on {repository}"
        )));
    }
    Ok(DoctorResult {
        account,
        arn,
        repository_exists: administers.is_some(),
    })
}
