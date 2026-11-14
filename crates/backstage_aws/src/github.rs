//! The GitHub repository, and one comment per environment on its pull request.
//!
//! `publish` creates the repository when it is missing and pushes the project
//! to `main`; Git authenticates with whatever credential helper the machine has.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    process,
};
use octocrab::Octocrab;
use serde::Serialize;
use serde_json::{Value, json};
use std::{env, ffi::OsStr, time::Duration};

const ACTIONS_BOT_ID: u64 = 41_898_282;

#[derive(Clone, Copy)]
enum CommentIdentity {
    AuthenticatedLogin,
    ActionsBot,
}

fn comment_identity() -> CommentIdentity {
    match (
        env::var("GITHUB_ACTIONS").as_deref(),
        env::var("GITHUB_TOKEN"),
    ) {
        (Ok("true"), Ok(token)) if !token.is_empty() => CommentIdentity::ActionsBot,
        _ => CommentIdentity::AuthenticatedLogin,
    }
}

const COMMENT_PAGE_SIZE: usize = 100;
const COMMENT_PAGE_LIMIT: usize = 100;

#[cfg(test)]
mod tests;

const GIT_LIMIT: Duration = Duration::from_mins(5);

#[derive(Serialize)]
pub(crate) struct PublishResult {
    repository: String,
    created: bool,
    commit: String,
}

fn token() -> Option<String> {
    let token = env::var("GITHUB_TOKEN").or_else(|_| env::var("GH_TOKEN"));
    token.ok().filter(|token| !token.is_empty())
}

pub(crate) fn client() -> Result<Octocrab> {
    let token = token().ok_or_else(|| Error::github_failed("set GITHUB_TOKEN or GH_TOKEN"))?;
    Octocrab::builder()
        .personal_token(token)
        .build()
        .map_err(failed)
}

/// Writes this namespace's comment on the pull request, replacing the one
/// from an earlier run. Returns the comment's URL, or `None` when no pull
/// request or token is configured: deploying does not depend on GitHub.
pub(crate) async fn comment(ctx: &Ctx, text: &str) -> Result<Option<String>> {
    let settings = &ctx.settings;
    let (Some(number), Some(_)) = (settings.settings.pull_request, token()) else {
        return Ok(None);
    };
    comment_with_client(
        &client()?,
        comment_identity(),
        &settings.settings.github_repository,
        number,
        &settings.namespace,
        text,
    )
    .await
}

async fn comment_with_client(
    octo: &Octocrab,
    identity: CommentIdentity,
    repository: &str,
    number: u64,
    namespace: &str,
    text: &str,
) -> Result<Option<String>> {
    let user: Option<Value> = match identity {
        CommentIdentity::AuthenticatedLogin => {
            Some(octo.get("/user", None::<&()>).await.map_err(failed)?)
        }
        CommentIdentity::ActionsBot => None,
    };
    let caller = match user.as_ref() {
        Some(user) => CommentOwner::Login(
            user.get("login")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::github_failed("GitHub /user response has no login"))?,
        ),
        None => CommentOwner::Id(ACTIONS_BOT_ID),
    };
    let marker = format!("<!-- backstage-aws:{namespace} -->");
    let body = json!({"body": format!("{marker}\n{text}")});
    let comments = format!("/repos/{repository}/issues/{number}/comments");
    let previous = find_comment(octo, &comments, &marker, caller).await?;
    let saved: Value = match previous {
        Some(id) => {
            let route = format!("/repos/{repository}/issues/comments/{id}");
            octo.patch(route, Some(&body)).await
        }
        None => octo.post(comments, Some(&body)).await,
    }
    .map_err(failed)?;
    Ok(saved
        .get("html_url")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

#[derive(Clone, Copy)]
enum CommentOwner<'a> {
    Login(&'a str),
    Id(u64),
}
async fn find_comment(
    octo: &Octocrab,
    comments: &str,
    marker: &str,
    caller: CommentOwner<'_>,
) -> Result<Option<u64>> {
    let mut page: usize = 1;
    while page <= COMMENT_PAGE_LIMIT {
        let route = format!("{comments}?per_page={COMMENT_PAGE_SIZE}&page={page}");
        let results: Vec<Value> = octo.get(route, None::<&()>).await.map_err(failed)?;
        if let Some(id) = results.iter().find_map(|comment| {
            let body = comment.get("body")?.as_str()?;
            let user = comment.get("user")?;
            let is_owner = match caller {
                CommentOwner::Login(login) => {
                    user.get("login")?.as_str()?.eq_ignore_ascii_case(login)
                }
                CommentOwner::Id(id) => user.get("id")?.as_u64()? == id,
            };
            (body.lines().next() == Some(marker) && is_owner)
                .then(|| comment.get("id")?.as_u64())?
        }) {
            return Ok(Some(id));
        }
        if results.len() < COMMENT_PAGE_SIZE {
            return Ok(None);
        }
        if page == COMMENT_PAGE_LIMIT {
            return Err(Error::github_failed(format!(
                "refusing to create a duplicate comment after {COMMENT_PAGE_LIMIT} full pages"
            )));
        }
        page = page.saturating_add(1);
    }
    Ok(None)
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
