//! Account-level operations run from an operator workstation: `bootstrap`,
//! `base`, and `up`, which chains a first install end to end.
use crate::{
    ctx::Ctx,
    db,
    error::{Error, Result},
    template,
    terraform::{self, Stack},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Serialize)]
pub(crate) struct BootstrapResult {
    /// True on the first run in an account, when local state moved to S3.
    state_migrated: bool,
    /// The committed record of this environment's settings, which CI loads.
    settings_file: String,
    deploy_role_arn: String,
}

#[derive(Serialize)]
pub(crate) struct BaseResult {
    outputs: Value,
}

pub(crate) async fn bootstrap(ctx: &Ctx) -> Result<BootstrapResult> {
    ctx.require_approval()?;
    let settings = &ctx.settings;
    Ok(BootstrapResult {
        state_migrated: terraform::bootstrap(ctx).await?,
        settings_file: template::write_settings_file(settings, &ctx.project_dir)?,
        deploy_role_arn: settings.deploy_role_arn(),
    })
}

pub(crate) async fn base(ctx: &Ctx) -> Result<BaseResult> {
    ctx.require_approval()?;
    if ctx.settings.settings.allowed_ingress_cidrs.is_empty() {
        return Err(Error::invalid_input(
            "set --allowed-ingress-cidrs: the load balancer admits only the IPv4 networks listed there",
        ));
    }
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
