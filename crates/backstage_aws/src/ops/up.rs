//! Account-level operations run from an operator workstation: `bootstrap`,
//! `base`, and `up`, which chains a first install end to end.
use crate::{
    ctx::Ctx,
    db,
    error::Result,
    github,
    terraform::{self, Stack},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Serialize)]
pub(crate) struct BootstrapResult {
    repository_created: bool,
    /// True on the first run in an account, when local state moved to S3.
    state_migrated: bool,
}

#[derive(Serialize)]
pub(crate) struct BaseResult {
    outputs: Value,
}

/// The bootstrap stack manages the repository's GitHub environment, so the
/// repository has to exist before Terraform runs.
pub(crate) async fn bootstrap(ctx: &Ctx) -> Result<BootstrapResult> {
    ctx.require_approval()?;
    Ok(BootstrapResult {
        repository_created: github::ensure_repository(ctx).await?,
        state_migrated: terraform::bootstrap(ctx).await?,
    })
}

pub(crate) async fn base(ctx: &Ctx) -> Result<BaseResult> {
    terraform::apply(ctx, Stack::Base, &ctx.settings.base_tfvars()).await?;
    db::initialize_database(ctx).await?;
    Ok(BaseResult {
        outputs: terraform::required_outputs(ctx, Stack::Base).await?,
    })
}

/// A first install. Sign-in credentials can only be stored once the base
/// stack has created their secret, and must be there before the first
/// deployment that uses GitHub sign-in. The project is pushed last, so the
/// workflows it triggers find the infrastructure they deploy to.
pub(crate) async fn up(ctx: &Ctx, seed: Option<&Path>, identity: Option<&Path>) -> Result<Value> {
    ctx.require_approval()?;
    let doctor = super::doctor(ctx).await?;
    let bootstrap = bootstrap(ctx).await?;
    let base = base(ctx).await?;
    let seed = match seed {
        Some(file) => Some(db::seed(ctx, file).await?),
        None => None,
    };
    if identity.is_some() {
        super::set_identity(ctx, identity).await?;
    }
    let environment = Box::pin(super::env_up(ctx, None, false)).await?;
    let publish = super::publish(ctx).await?;
    Ok(json!({
        "doctor": doctor,
        "bootstrap": bootstrap,
        "base": base,
        "seed": seed,
        "environment": environment,
        "publish": publish,
    }))
}
