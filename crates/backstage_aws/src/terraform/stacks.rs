//! Plan-then-apply for the three stacks. Terraform only ever applies a plan
//! file it has just written, and only when the invocation was approved.
use super::{Stack, bucket, generate, remote_state, state_outputs};
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    process,
};
use serde_json::Value;
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

struct Workspace {
    temp: Option<tempfile::TempDir>,
    /// Root of the `.terraform-workspace-*` directory; survives `keep()`.
    root: PathBuf,
    path: PathBuf,
}

impl Workspace {
    fn path(&self) -> &Path {
        &self.path
    }

    /// Retain the directory so cancellation or process termination preserves
    /// Terraform's errored.tfstate for post-mortem inspection.
    fn retain(&mut self) {
        if let Some(temp) = self.temp.take() {
            let _ = temp.keep();
        }
        eprintln!("Terraform workspace retained: {}", self.path.display());
    }

    /// Remove the workspace after a successful apply; failures keep it for inspection.
    fn cleanup(&self) -> Result<()> {
        fs::remove_dir_all(&self.root)
            .map_err(|error| Error::state_failed(format!("{}: {error}", self.root.display())))
    }
}

const PLAN: &str = "backstage-aws.tfplan";
const VARS: &str = "terraform.tfvars";
/// Git-ignored; points the bootstrap stack at the bucket once it exists.
const BACKEND_FILE: &str = "backend.local.tf";
const LOCAL_STATE: &str = "terraform.tfstate";
const LIMIT: Duration = Duration::from_mins(60);
const MAX_STATE_BYTES: u64 = 16 * 1024 * 1024;

pub async fn apply(ctx: &Ctx, stack: Stack, vars: &Value) -> Result<()> {
    remote(ctx, stack, vars, false).await
}

pub async fn destroy(ctx: &Ctx, stack: Stack, vars: &Value) -> Result<()> {
    remote(ctx, stack, vars, true).await
}

async fn remote(ctx: &Ctx, stack: Stack, vars: &Value, destroy: bool) -> Result<()> {
    ctx.require_approval()?;
    let mut workspace = workspace(ctx, stack)?;
    write(&workspace.path().join(VARS), &generate::variables(vars)?)?;
    init(ctx, stack, workspace.path(), Some("-reconfigure")).await?;
    // Arm retention before remote apply so cancellation preserves errored.tfstate.
    workspace.retain();
    let dir = workspace.path();
    match plan_and_apply(ctx, dir, destroy).await {
        Ok(()) => {
            workspace.cleanup()?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Bootstraps the account from whichever state exists: remote state is adopted,
/// state on disk moves into the bucket before any apply, and with no state the
/// first apply runs locally before its state moves in. Every apply retains its
/// workspace on failure so errored state survives. Returns true when this run
/// created the bootstrap state.
pub async fn bootstrap(ctx: &Ctx) -> Result<bool> {
    ctx.require_approval()?;
    let stack = Stack::Bootstrap;
    let source_state = stack.dir(ctx).join(LOCAL_STATE);
    let local = local_state(ctx, &source_state)?;
    bucket::ensure(ctx).await?;
    let remote = remote_state(ctx, stack).await?;
    if let Some(state) = &remote
        && !state
            .get("resources")
            .and_then(Value::as_array)
            .is_some_and(|resources| belongs_to(ctx, resources))
    {
        return Err(Error::precondition_failed(
            "remote bootstrap state does not contain this account's bootstrap resources and account identity",
        ));
    }
    if let (Some(local), Some(remote)) = (&local, &remote)
        && !same_state(local, remote)
    {
        return Err(Error::precondition_failed(
            "local and remote bootstrap states differ; refusing to overwrite either state",
        ));
    }
    let mut workspace = workspace(ctx, stack)?;
    write(
        &workspace.path().join(VARS),
        &generate::variables(&ctx.settings.bootstrap_tfvars())?,
    )?;
    let created = match (remote, local) {
        (Some(state), _) => {
            adopt_remote(ctx, stack, &mut workspace, &source_state, &state).await?;
            false
        }
        (None, Some(state)) => {
            migrate_then_apply(ctx, stack, &mut workspace, &source_state, &state).await?;
            false
        }
        (None, None) => {
            apply_then_migrate(ctx, stack, &mut workspace, &source_state).await?;
            true
        }
    };
    workspace.cleanup()?;
    Ok(created)
}

/// Re-applies bootstrap state already in the bucket.
async fn adopt_remote(
    ctx: &Ctx,
    stack: Stack,
    workspace: &mut Workspace,
    source_state: &Path,
    state: &Value,
) -> Result<()> {
    encrypt_state_bucket(ctx, state).await?;
    write(&workspace.path().join(BACKEND_FILE), &generate::backend()?)?;
    init(ctx, stack, workspace.path(), Some("-reconfigure")).await?;
    // A previous migration may have succeeded before its cleanup completed.
    remove(source_state)?;
    workspace.retain();
    plan_and_apply(ctx, workspace.path(), false).await
}

/// Moves state found on disk into the bucket, then applies from the bucket.
async fn migrate_then_apply(
    ctx: &Ctx,
    stack: Stack,
    workspace: &mut Workspace,
    source_state: &Path,
    state: &Value,
) -> Result<()> {
    encrypt_state_bucket(ctx, state).await?;
    init_local(ctx, stack, workspace.path()).await?;
    move_to_bucket(ctx, stack, workspace.path(), source_state).await?;
    workspace.retain();
    plan_and_apply(ctx, workspace.path(), false).await
}

/// Applies with no state anywhere, then moves the new state into the bucket.
async fn apply_then_migrate(
    ctx: &Ctx,
    stack: Stack,
    workspace: &mut Workspace,
    source_state: &Path,
) -> Result<()> {
    init_local(ctx, stack, workspace.path()).await?;
    workspace.retain();
    plan_and_apply(ctx, workspace.path(), false).await?;
    let state = local_state(ctx, source_state)?.ok_or_else(|| {
        Error::state_failed("bootstrap apply produced no recoverable local state")
    })?;
    encrypt_state_bucket(ctx, &state).await?;
    // Never force-copy over a state written since this run started.
    if remote_state(ctx, stack).await?.is_some() {
        return Err(Error::precondition_failed(
            "remote bootstrap state appeared during local apply; local state retained",
        ));
    }
    move_to_bucket(ctx, stack, workspace.path(), source_state).await
}

/// Points the workspace at local state, the source of any migration. Written
/// directly so the recovery backend survives cancellation.
async fn init_local(ctx: &Ctx, stack: Stack, dir: &Path) -> Result<()> {
    write(&dir.join(BACKEND_FILE), &generate::local_backend()?)?;
    init(ctx, stack, dir, None).await
}

/// Migrates the workspace's local state into the bucket, then removes the source copy.
async fn move_to_bucket(ctx: &Ctx, stack: Stack, dir: &Path, source_state: &Path) -> Result<()> {
    write(&dir.join(BACKEND_FILE), &generate::backend()?)?;
    init(ctx, stack, dir, Some("-migrate-state")).await?;
    remove(source_state)
}

async fn encrypt_state_bucket(ctx: &Ctx, state: &Value) -> Result<()> {
    let outputs = state_outputs(state);
    let key = super::output_str(&outputs, "state_kms_key_arn")?;
    bucket::encrypt(ctx, key).await
}

fn same_state(local: &Value, remote: &Value) -> bool {
    ["lineage", "serial", "resources", "outputs"]
        .iter()
        .all(|field| local.get(field).is_some() && local.get(field) == remote.get(field))
}

fn local_state(ctx: &Ctx, local_state: &Path) -> Result<Option<Value>> {
    let file = match File::open(local_state) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(state_read_error(local_state, error)),
    };
    let mut bounded = file.take(MAX_STATE_BYTES + 1);
    let mut bytes = Vec::new();
    bounded
        .read_to_end(&mut bytes)
        .map_err(|error| state_read_error(local_state, error))?;
    if u64::try_from(bytes.len()).map_or(true, |size| size > MAX_STATE_BYTES) {
        return Err(Error::state_failed(format!(
            "{} exceeds the {MAX_STATE_BYTES}-byte local state limit",
            local_state.display()
        )));
    }
    let state: Value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::state_failed(format!("{}: {error}", local_state.display())))?;
    let resources = state
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::state_failed(format!("{} has no resources array", local_state.display()))
        })?;
    if resources.is_empty() {
        return Ok(None);
    }
    if belongs_to(ctx, resources) {
        return Ok(Some(state));
    }
    Err(Error::precondition_failed(format!(
        "{} does not contain this account's bootstrap resources and account identity",
        local_state.display()
    )))
}

fn state_read_error(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::state_failed(format!("{}: {error}", path.display()))
}

fn belongs_to(ctx: &Ctx, resources: &[Value]) -> bool {
    let expected_bucket = resources.iter().any(|resource| {
        resource.get("mode").and_then(Value::as_str) == Some("managed")
            && resource.get("type").and_then(Value::as_str) == Some("aws_s3_bucket")
            && resource.get("name").and_then(Value::as_str) == Some("state")
            && instances_have(resource, "bucket", &ctx.settings.state_bucket())
    });
    let expected_alias = resources.iter().any(|resource| {
        resource.get("mode").and_then(Value::as_str) == Some("managed")
            && resource.get("type").and_then(Value::as_str) == Some("aws_kms_alias")
            && resource.get("name").and_then(Value::as_str) == Some("state")
            && instances_have(
                resource,
                "name",
                &format!("alias/{}-terraform-state", ctx.settings.settings.name),
            )
    });
    // The key can be recorded before Terraform has created its alias.
    let expected_key = resources.iter().any(|resource| {
        resource.get("mode").and_then(Value::as_str) == Some("managed")
            && resource.get("type").and_then(Value::as_str) == Some("aws_kms_key")
            && resource.get("name").and_then(Value::as_str) == Some("state")
            && instances_have(
                resource,
                "description",
                &format!(
                    "Encrypt {} Terraform remote state",
                    ctx.settings.settings.name
                ),
            )
    });
    let expected_account = resources.iter().any(|resource| {
        resource.get("mode").and_then(Value::as_str) == Some("data")
            && resource.get("type").and_then(Value::as_str) == Some("aws_caller_identity")
            && resource.get("name").and_then(Value::as_str) == Some("current")
            && instances_have(
                resource,
                "account_id",
                &ctx.settings.settings.aws_account_id,
            )
    });
    (expected_bucket || expected_alias || expected_key) && expected_account
}

fn instances_have(resource: &Value, key: &str, expected: &str) -> bool {
    resource
        .get("instances")
        .and_then(Value::as_array)
        .is_some_and(|instances| {
            instances.iter().any(|instance| {
                instance
                    .get("attributes")
                    .and_then(|attributes| attributes.get(key))
                    .and_then(Value::as_str)
                    == Some(expected)
            })
        })
}

/// Initializes against the state bucket in the given mode, or against state
/// on disk when there is no mode. Encryption is the bucket's default key.
async fn init(ctx: &Ctx, stack: Stack, dir: &Path, remote: Option<&str>) -> Result<()> {
    let mut args = ["init", "-input=false", "-lockfile=readonly"]
        .map(str::to_owned)
        .to_vec();
    let Some(mode) = remote else {
        let state_path = std::path::absolute(stack.dir(ctx).join(LOCAL_STATE))
            .map_err(|error| Error::state_failed(format!("local state path: {error}")))?;
        args.push("-reconfigure".to_owned());
        args.push(format!("-backend-config=path={}", state_path.display()));
        return terraform(ctx, dir, &args).await;
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
    terraform(ctx, dir, &args).await
}

async fn plan_and_apply(ctx: &Ctx, dir: &Path, destroy: bool) -> Result<()> {
    let mut plan = vec!["plan", "-input=false", "-out", PLAN];
    if destroy {
        plan.push("-destroy");
    }
    terraform(ctx, dir, &plan).await?;
    terraform(ctx, dir, &["apply", "-input=false", PLAN]).await
}

async fn terraform<S: AsRef<OsStr> + Sync>(ctx: &Ctx, dir: &Path, args: &[S]) -> Result<()> {
    process::run(ctx.terraform.as_os_str(), args, dir, None, LIMIT).await
}

fn workspace(ctx: &Ctx, stack: Stack) -> Result<Workspace> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::Builder::new()
        .prefix(".terraform-workspace-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&ctx.project_dir)
        .map_err(|error| Error::state_failed(error.to_string()))?;
    let path = temp.path().join(stack.name());
    fs::create_dir(&path).map_err(|error| state_read_error(&path, error))?;
    copy_tree(&stack.dir(ctx), &path)?;
    let root = temp.path().to_path_buf();
    Ok(Workspace {
        temp: Some(temp),
        root,
        path,
    })
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::read_dir(source)
        .map_err(|error| state_read_error(source, error))?
        .try_for_each(|entry| {
            let entry = entry.map_err(|error| state_read_error(source, error))?;
            let name = entry.file_name();
            if name == ".terraform" {
                return Ok(());
            }
            let from = entry.path();
            let to = destination.join(name);
            let kind = entry
                .file_type()
                .map_err(|error| state_read_error(&from, error))?;
            if kind.is_dir() {
                fs::create_dir_all(&to).map_err(|error| state_read_error(&to, error))?;
                copy_tree(&from, &to)
            } else if kind.is_file() {
                fs::copy(&from, &to)
                    .map(|_| ())
                    .map_err(|error| state_read_error(&to, error))
            } else {
                Err(Error::state_failed(format!(
                    "unsupported Terraform source entry {}",
                    from.display()
                )))
            }
        })
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
