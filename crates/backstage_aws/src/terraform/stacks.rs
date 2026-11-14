//! Plan-then-apply for the three stacks. Terraform only ever applies a plan
//! file it has just written, and only when the invocation was approved.
use super::Stack;
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    process,
};
use aws_sdk_s3::operation::head_bucket::HeadBucketError;
use serde_json::Value;
use std::{ffi::OsStr, fs, path::Path, time::Duration};

const PLAN: &str = "backstage-aws.tfplan";
const VARS: &str = "terraform.tfvars.json";
/// Git-ignored; points the bootstrap stack at the bucket once it exists.
const BACKEND_FILE: &str = "backend.local.tf";
const BACKEND_BLOCK: &str = "terraform {\n  backend \"s3\" {}\n}\n";
const LOCAL_STATE: &str = "terraform.tfstate";
const LIMIT: Duration = Duration::from_mins(60);

pub async fn apply(ctx: &Ctx, stack: Stack, vars: &Value) -> Result<()> {
    remote(ctx, stack, vars, false).await
}

pub async fn destroy(ctx: &Ctx, stack: Stack, vars: &Value) -> Result<()> {
    remote(ctx, stack, vars, true).await
}

async fn remote(ctx: &Ctx, stack: Stack, vars: &Value, destroy: bool) -> Result<()> {
    ctx.require_approval()?;
    write(&stack.dir(ctx).join(VARS), &to_json(vars)?)?;
    init(ctx, stack, Some("-reconfigure")).await?;
    plan_and_apply(ctx, stack, destroy).await
}

/// Applies the bootstrap stack. The account, not this checkout, says whether
/// it is bootstrapped: until its state bucket exists the stack is applied with
/// state on disk. State found on disk is then moved into the bucket, which
/// also completes a run that failed between the two. Returns whether state
/// was moved.
pub async fn bootstrap(ctx: &Ctx) -> Result<bool> {
    ctx.require_approval()?;
    let stack = Stack::Bootstrap;
    let dir = stack.dir(ctx);
    let (backend_file, local_state) = (dir.join(BACKEND_FILE), dir.join(LOCAL_STATE));
    held_locally(ctx, &local_state)?;
    let vars = to_json(&ctx.settings.bootstrap_tfvars())?;
    write(&dir.join(VARS), &vars)?;
    let bootstrapped = bucket_exists(ctx).await?;
    if !bootstrapped {
        remove(&backend_file)?;
        init(ctx, stack, None).await?;
        plan_and_apply(ctx, stack, false).await?;
    }
    write(&backend_file, BACKEND_BLOCK.as_bytes())?;
    let moved = held_locally(ctx, &local_state)?;
    if moved {
        init(ctx, stack, Some("-migrate-state")).await?;
        remove(&local_state)?;
    } else {
        init(ctx, stack, Some("-reconfigure")).await?;
    }
    if bootstrapped {
        plan_and_apply(ctx, stack, false).await?;
    }
    Ok(moved)
}

/// Whether the state on disk holds this account's resources. State for any
/// other account is refused, so it can never be moved into this one's bucket.
fn held_locally(ctx: &Ctx, local_state: &Path) -> Result<bool> {
    let text = fs::read_to_string(local_state).unwrap_or_default();
    let state = serde_json::from_str::<Value>(&text).unwrap_or_default();
    let resources = state.get("resources").and_then(Value::as_array);
    if resources.is_none_or(Vec::is_empty) {
        return Ok(false);
    }
    if text.contains(&ctx.settings.settings.aws_account_id) {
        return Ok(true);
    }
    Err(Error::precondition_failed(format!(
        "{} holds bootstrap state for another account; finish that bootstrap or move the file away",
        local_state.display()
    )))
}

async fn bucket_exists(ctx: &Ctx) -> Result<bool> {
    let request = ctx.s3.head_bucket().bucket(ctx.settings.state_bucket());
    match request.send().await {
        Ok(_) => Ok(true),
        Err(error)
            if error
                .as_service_error()
                .is_some_and(HeadBucketError::is_not_found) =>
        {
            Ok(false)
        }
        Err(error) => Err(Error::aws_failed("s3", "HeadBucket", error.to_string())),
    }
}

/// Initializes against the state bucket in the given mode, or against state
/// on disk when there is no mode. Encryption is the bucket's default key.
async fn init(ctx: &Ctx, stack: Stack, remote: Option<&str>) -> Result<()> {
    let mut args = ["init", "-input=false", "-lockfile=readonly"]
        .map(str::to_owned)
        .to_vec();
    let Some(mode) = remote else {
        args.push("-reconfigure".to_owned());
        return terraform(ctx, stack, &args).await;
    };
    args.push(mode.to_owned());
    if mode == "-migrate-state" {
        args.push("-force-copy".to_owned());
    }
    let pairs = [
        format!("bucket={}", ctx.settings.state_bucket()),
        format!("key={}", stack.state_key(ctx)),
        format!("region={}", ctx.settings.settings.aws_region),
        "use_lockfile=true".to_owned(),
    ];
    args.extend(pairs.map(|pair| format!("-backend-config={pair}")));
    terraform(ctx, stack, &args).await
}

async fn plan_and_apply(ctx: &Ctx, stack: Stack, destroy: bool) -> Result<()> {
    let mut plan = vec!["plan", "-input=false", "-out", PLAN];
    if destroy {
        plan.push("-destroy");
    }
    terraform(ctx, stack, &plan).await?;
    terraform(ctx, stack, &["apply", "-input=false", PLAN]).await
}

async fn terraform<S: AsRef<OsStr> + Sync>(ctx: &Ctx, stack: Stack, args: &[S]) -> Result<()> {
    process::run(
        ctx.terraform.as_os_str(),
        args,
        &stack.dir(ctx),
        None,
        LIMIT,
    )
    .await
}

fn to_json(value: &Value) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(|error| Error::state_failed(error.to_string()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes)
        .map_err(|error| Error::state_failed(format!("{}: {error}", path.display())))
}

fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(Error::state_failed(format!("{}: {error}", path.display())))
        }
        _ => Ok(()),
    }
}
