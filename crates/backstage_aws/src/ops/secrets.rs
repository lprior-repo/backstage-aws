//! Stores the GitHub OAuth application credentials in the secret the base
//! stack owns. The values come from a private file or the environment, never
//! from flags.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::Path};

const CLIENT_ID_ENV: &str = "BACKSTAGE_AWS_OAUTH_CLIENT_ID";
const CLIENT_SECRET_ENV: &str = "BACKSTAGE_AWS_OAUTH_CLIENT_SECRET";

#[derive(Serialize)]
pub(crate) struct IdentityResult {
    secret_arn: String,
    version_id: String,
}

/// Serialized with the JSON keys the `env` stack's task definition reads.
#[derive(Deserialize, Serialize)]
struct Identity {
    #[serde(rename = "clientId", alias = "client_id")]
    client_id: String,
    #[serde(rename = "clientSecret", alias = "client_secret")]
    client_secret: String,
}

pub(crate) async fn set_identity(ctx: &Ctx, file: Option<&Path>) -> Result<IdentityResult> {
    ctx.require_approval()?;
    let identity = read_identity(file)?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let secret_arn = output_str(&base, "identity_secret_arn")?.to_owned();
    let document = serde_json::to_string(&identity)
        .map_err(|_| Error::state_failed("the credentials could not be encoded"))?;
    // Error text from the service is not echoed here: it can quote the request.
    let failed = || {
        Error::aws_failed(
            "secretsmanager",
            "PutSecretValue",
            "the secret was not stored",
        )
    };
    let stored = ctx
        .secrets
        .put_secret_value()
        .secret_id(&secret_arn)
        .secret_string(document)
        .send()
        .await
        .map_err(|_| failed())?;
    Ok(IdentityResult {
        secret_arn,
        version_id: stored.version_id().unwrap_or_default().to_owned(),
    })
}

fn read_identity(file: Option<&Path>) -> Result<Identity> {
    let identity = if let Some(path) = file {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Identity>(&bytes).ok())
            .ok_or_else(|| {
                Error::invalid_input(format!(
                    "{} must be a JSON file with `clientId` and `clientSecret`",
                    path.display()
                ))
            })?
    } else {
        let read = |name: &str| {
            env::var(name).map_err(|_| Error::invalid_input(format!("set {name} or pass --file")))
        };
        Identity {
            client_id: read(CLIENT_ID_ENV)?,
            client_secret: read(CLIENT_SECRET_ENV)?,
        }
    };
    if identity.client_id.trim().is_empty() || identity.client_secret.trim().is_empty() {
        return Err(Error::invalid_input(
            "the OAuth client ID and secret must not be empty",
        ));
    }
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credentials_file_is_stored_under_the_keys_the_task_definition_reads() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|error| Error::state_failed(error.to_string()))?;
        let path = dir.path().join("identity.json");
        fs::write(&path, r#"{"client_id":"abc","client_secret":"shh"}"#)
            .map_err(|error| Error::state_failed(error.to_string()))?;
        let stored = serde_json::to_value(read_identity(Some(&path))?)
            .map_err(|error| Error::state_failed(error.to_string()))?;
        assert_eq!(
            stored,
            serde_json::json!({"clientId": "abc", "clientSecret": "shh"})
        );
        Ok(())
    }

    #[test]
    fn empty_or_malformed_credentials_are_rejected() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|error| Error::state_failed(error.to_string()))?;
        for body in [
            r#"{"clientId":" ","clientSecret":"x"}"#,
            r#"{"clientId":"x"}"#,
            "not json",
        ] {
            let path = dir.path().join("identity.json");
            fs::write(&path, body).map_err(|error| Error::state_failed(error.to_string()))?;
            let code = read_identity(Some(&path))
                .map(drop)
                .map_err(|error| error.code());
            assert_eq!(code, Err("INVALID_INPUT"), "{body}");
        }
        Ok(())
    }
}
