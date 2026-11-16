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
        version_id: stored
            .version_id()
            .map_or_else(String::new, std::borrow::ToOwned::to_owned),
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
    use aws_sdk_s3::{operation::get_object::GetObjectOutput, primitives::ByteStream};
    use aws_sdk_secretsmanager::operation::put_secret_value::{
        PutSecretValueError, PutSecretValueOutput,
    };
    use aws_smithy_mocks::{RuleMode, mock, mock_client};

    const BASE_STATE: &str = r#"{"outputs":{"identity_secret_arn":{"value":"arn:aws:secretsmanager:us-east-1:123456789012:secret:portal/identity"}}}"#;

    #[tokio::test]
    async fn set_identity_stores_exact_arn_camel_case_credentials_and_version() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|error| Error::state_failed(error.to_string()))?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        let base = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| request.key() == Some("base/terraform.tfstate"))
            .then_output(|| {
                GetObjectOutput::builder()
                    .body(ByteStream::from_static(BASE_STATE.as_bytes()))
                    .build()
            });
        ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&base]);
        let path = dir.path().join("identity.json");
        fs::write(
            &path,
            r#"{"clientId":"app-id","clientSecret":"private-value"}"#,
        )
        .map_err(|error| Error::state_failed(error.to_string()))?;
        let stored = mock!(aws_sdk_secretsmanager::Client::put_secret_value)
            .match_requests(|request| {
                request.secret_id()
                    == Some("arn:aws:secretsmanager:us-east-1:123456789012:secret:portal/identity")
                    && request.secret_string()
                        == Some(r#"{"clientId":"app-id","clientSecret":"private-value"}"#)
            })
            .then_output(|| {
                PutSecretValueOutput::builder()
                    .version_id("version-7")
                    .build()
            });
        ctx.secrets = mock_client!(aws_sdk_secretsmanager, RuleMode::MatchAny, [&stored]);

        let result = set_identity(&ctx, Some(&path)).await?;

        assert_eq!(
            serde_json::to_value(result).map_err(|error| Error::state_failed(error.to_string()))?,
            serde_json::json!({
                "secret_arn": "arn:aws:secretsmanager:us-east-1:123456789012:secret:portal/identity",
                "version_id": "version-7"
            })
        );
        assert_eq!(stored.num_calls(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn set_identity_sanitizes_the_service_error() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|error| Error::state_failed(error.to_string()))?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        let base = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| request.key() == Some("base/terraform.tfstate"))
            .then_output(|| {
                GetObjectOutput::builder()
                    .body(ByteStream::from_static(BASE_STATE.as_bytes()))
                    .build()
            });
        ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&base]);
        let path = dir.path().join("identity.json");
        fs::write(
            &path,
            r#"{"clientId":"app-id","clientSecret":"private-value"}"#,
        )
        .map_err(|error| Error::state_failed(error.to_string()))?;
        let failed = mock!(aws_sdk_secretsmanager::Client::put_secret_value)
            .then_error(|| PutSecretValueError::unhandled("private diagnostic"));
        ctx.secrets = mock_client!(aws_sdk_secretsmanager, RuleMode::MatchAny, [&failed]);

        let Err(error) = set_identity(&ctx, Some(&path)).await else {
            return Err(Error::state_failed("expected a storage failure"));
        };
        assert_eq!(error.code(), "AWS_FAILED");
        assert!(!error.to_string().contains("private diagnostic"));
        assert!(error.to_string().contains("the secret was not stored"));
        assert_eq!(failed.num_calls(), 1);
        Ok(())
    }

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
