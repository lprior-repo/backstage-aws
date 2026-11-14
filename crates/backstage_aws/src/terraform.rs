//! Terraform 1.17.2: version check and stack outputs. Every stack's state
//! lives in the account's state bucket, whose name follows from the settings.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    process,
};
use aws_sdk_s3::{error::ProvideErrorMetadata, operation::get_object::GetObjectError};
use serde_json::{Map, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

mod stacks;
pub(crate) use stacks::{apply, bootstrap, destroy};

const VERSION: &str = "1.17.2";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stack {
    Bootstrap,
    Base,
    Env,
}

impl Stack {
    pub(crate) const ALL: [Self; 3] = [Self::Bootstrap, Self::Base, Self::Env];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Base => "base",
            Self::Env => "env",
        }
    }

    pub(crate) fn dir(self, ctx: &Ctx) -> PathBuf {
        ctx.project_dir.join("infra").join(self.name())
    }

    fn state_key(self, ctx: &Ctx) -> String {
        match self {
            Self::Bootstrap => "bootstrap/terraform.tfstate".to_owned(),
            Self::Base => "base/terraform.tfstate".to_owned(),
            Self::Env => ctx.settings.env_state_key(),
        }
    }
}

pub(crate) async fn verify_version(terraform: &Path) -> Result<()> {
    let args = ["version", "-json"];
    let limit = Duration::from_secs(20);
    let stdout = process::capture(terraform.as_os_str(), &args, Path::new("."), limit).await?;
    let found = serde_json::from_slice::<Value>(&stdout)
        .ok()
        .and_then(|value| Some(value.get("terraform_version")?.as_str()?.to_owned()));
    if found.as_deref() == Some(VERSION) {
        return Ok(());
    }
    Err(Error::precondition_failed(format!(
        "Terraform {VERSION} is required, found {}; install it (hashicorp/setup-terraform in CI) or pass --terraform",
        found.as_deref().unwrap_or("an unreadable version")
    )))
}

/// Outputs of an applied stack, read from its state object. `None` when the
/// stack has never been applied, or the account was never bootstrapped.
pub(crate) async fn outputs(ctx: &Ctx, stack: Stack) -> Result<Option<Value>> {
    let failed = |detail: String| Error::aws_failed("s3", "GetObject", detail);
    let request = ctx
        .s3
        .get_object()
        .bucket(ctx.settings.state_bucket())
        .key(stack.state_key(ctx));
    let missing =
        |error: &GetObjectError| error.is_no_such_key() || error.code() == Some("NoSuchBucket");
    let object = match request.send().await {
        Ok(object) => object,
        Err(error) if error.as_service_error().is_some_and(missing) => return Ok(None),
        Err(error) => return Err(failed(error.to_string())),
    };
    let bytes = object
        .body
        .collect()
        .await
        .map_err(|error| failed(error.to_string()))?
        .into_bytes();
    let state: Value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::state_failed(format!("{} state: {error}", stack.name())))?;
    let values = state
        .get("outputs")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, output)| Some((name.clone(), output.get("value")?.clone())))
        .collect::<Map<_, _>>();
    Ok(Some(Value::Object(values)))
}

/// Outputs of a stack that must already be applied.
pub(crate) async fn required_outputs(ctx: &Ctx, stack: Stack) -> Result<Value> {
    outputs(ctx, stack).await?.ok_or_else(|| {
        Error::precondition_failed(format!(
            "the {} stack has not been applied in this account",
            stack.name()
        ))
    })
}

/// A string output, or a precondition failure naming it.
pub(crate) fn output_str<'a>(outputs: &'a Value, name: &str) -> Result<&'a str> {
    outputs
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::precondition_failed(format!("Terraform output `{name}` is missing")))
}

#[cfg(test)]
mod tests;
