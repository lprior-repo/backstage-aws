//! Creates the GitHub repository when it is missing and pushes the project to
//! `main`. Git authenticates with whatever credential helper the machine has.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    process,
};
use octocrab::Octocrab;
use serde::Serialize;
use std::{env, ffi::OsStr, time::Duration};

const GIT_LIMIT: Duration = Duration::from_mins(5);

#[derive(Serialize)]
pub(crate) struct PublishResult {
    repository: String,
    created: bool,
    commit: String,
}

pub(crate) fn client() -> Result<Octocrab> {
    let token = env::var("GITHUB_TOKEN")
        .or_else(|_| env::var("GH_TOKEN"))
        .map_err(|_| Error::github_failed("set GITHUB_TOKEN or GH_TOKEN"))?;
    Octocrab::builder()
        .personal_token(token)
        .build()
        .map_err(failed)
}

fn failed(error: impl std::fmt::Display) -> Error {
    Error::github_failed(error.to_string())
}

/// Whether the token administers `owner/name`; `None` when no such repository
/// is visible to it.
pub(crate) async fn administers(octo: &Octocrab, repository: &str) -> Result<Option<bool>> {
    let (owner, name) = split(repository)?;
    match octo.repos(owner, name).get().await {
        Ok(found) => Ok(Some(found.permissions.is_some_and(|granted| granted.admin))),
        Err(octocrab::Error::GitHub { source, .. }) if source.status_code.as_u16() == 404 => {
            Ok(None)
        }
        Err(error) => Err(failed(error)),
    }
}

fn split(repository: &str) -> Result<(&str, &str)> {
    repository
        .split_once('/')
        .ok_or_else(|| Error::invalid_input("the GitHub repository must be `owner/name`"))
}

async fn create(octo: &Octocrab, repository: &str) -> Result<()> {
    let (owner, name) = split(repository)?;
    let caller = octo.current().user().await.map_err(failed)?.login;
    let route = if caller.eq_ignore_ascii_case(owner) {
        "/user/repos".to_owned()
    } else {
        format!("/orgs/{owner}/repos")
    };
    let body = serde_json::json!({"name": name, "private": true});
    octo.post::<_, serde_json::Value>(route, Some(&body))
        .await
        .map(drop)
        .map_err(failed)
}

/// Creates the repository when the token cannot see one. Returns whether it
/// was created.
pub(crate) async fn ensure_repository(ctx: &Ctx) -> Result<bool> {
    ctx.require_approval()?;
    let repository = &ctx.settings.settings.github_repository;
    let octo = client()?;
    match administers(&octo, repository).await? {
        Some(true) => Ok(false),
        Some(false) => Err(Error::precondition_failed(
            "the GitHub token must have admin permission on the repository",
        )),
        None => create(&octo, repository).await.map(|()| true),
    }
}

pub(crate) async fn publish(ctx: &Ctx) -> Result<PublishResult> {
    let created = ensure_repository(ctx).await?;
    let repository = ctx.settings.settings.github_repository.clone();
    let dir = &ctx.project_dir;
    let git = OsStr::new("git");
    let run = async |args: &[&str]| process::run(git, args, dir, None, GIT_LIMIT).await;
    let capture = async |args: &[&str]| process::capture(git, args, dir, GIT_LIMIT).await;
    run(&["init", "--initial-branch=main"]).await?;
    run(&["add", "-A"]).await?;
    if !capture(&["status", "--porcelain"]).await?.is_empty() {
        run(&["commit", "-m", "Publish Backstage project"]).await?;
    }
    let has_origin = capture(&["remote"])
        .await?
        .split(|byte| *byte == b'\n')
        .any(|remote| remote == b"origin");
    let verb = if has_origin { "set-url" } else { "add" };
    let url = format!("https://github.com/{repository}.git");
    run(&["remote", verb, "origin", &url]).await?;
    // Publishes the checked-out commit without renaming or moving any local branch.
    run(&["push", "origin", "HEAD:refs/heads/main"]).await?;
    let commit = String::from_utf8_lossy(&capture(&["rev-parse", "HEAD"]).await?)
        .trim()
        .to_owned();
    Ok(PublishResult {
        repository,
        created,
        commit,
    })
}
