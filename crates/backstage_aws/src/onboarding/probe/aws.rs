use super::{Check, Stage};
use super::{action_required, aws_base, blocked, command_error, invoke, parse_output, verified};
use crate::{
    error::{Error, Result},
    settings::Settings,
};
use serde_json::Value;
use std::path::Path;

pub(super) async fn version(program: &Path) -> Result<std::result::Result<Check, Check>> {
    let output = match invoke(program, &["--version".into()], Path::new(".")).await {
        Ok(output) => output,
        Err(Error::ToolFailed { detail, .. })
            if detail.starts_with("could not start executable:") =>
        {
            return Ok(Err(action_required(
                Stage::AwsCli,
                format!(
                    "AWS CLI is unavailable at {}; install AWS CLI v2 and retry.",
                    program.display()
                ),
            )));
        }
        Err(error) => return Err(error),
    };
    let version = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && version.starts_with("aws-cli/2.") {
        Ok(Ok(verified(Stage::AwsCli, "AWS CLI v2 is available.")))
    } else {
        Ok(Err(action_required(
            Stage::AwsCli,
            "AWS CLI v2 is required for IAM Identity Center support.",
        )))
    }
}

pub(super) async fn identity(
    program: &Path,
    profile: Option<&str>,
    settings: &Settings,
) -> Result<std::result::Result<(Check, std::result::Result<Check, Check>), Check>> {
    let value = match aws_json(
        program,
        profile,
        settings,
        "sts",
        "get-caller-identity",
        &[],
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return credential_or_access(error, Stage::Credentials, "sts get-caller-identity").map(
                |result| match result {
                    Ok(check) | Err(check) => Err(check),
                },
            );
        }
    };
    let identity = parse_identity(&value)?;
    let credential_check = verified(
        Stage::Credentials,
        "AWS credentials successfully resolved an active caller identity.",
    );
    let account_check = if settings.aws_account_id.trim().is_empty() {
        Err(action_required(
            Stage::Account,
            "Set the target AWS account ID in deployment settings before onboarding.",
        ))
    } else if identity.account != settings.aws_account_id {
        Err(action_required(
            Stage::Account,
            format!(
                "The active AWS identity belongs to account {}, not configured account {}.",
                identity.account, settings.aws_account_id
            ),
        ))
    } else {
        match identity.principal {
            Principal::User => Err(action_required(
                Stage::Account,
                "An IAM long-lived user identity is not allowed; sign in with an assumed role.",
            )),
            Principal::Root | Principal::AssumedRole => Ok(verified(
                Stage::Account,
                format!(
                    "Caller identity belongs to configured account {}.",
                    identity.account
                ),
            )),
            Principal::Other => Err(action_required(
                Stage::Account,
                "The active AWS identity is not an assumed role; sign in with an assumed role.",
            )),
        }
    };
    Ok(Ok((credential_check, account_check)))
}

struct Identity {
    account: String,
    principal: Principal,
}

enum Principal {
    Root,
    User,
    AssumedRole,
    Other,
}

fn parse_identity(value: &Value) -> Result<Identity> {
    let object = super::parse::object(
        value,
        &["UserId", "Account", "Arn"],
        "STS GetCallerIdentity",
    )?;
    let account = super::parse::string(object, "Account", "STS GetCallerIdentity")?;
    let arn = super::parse::string(object, "Arn", "STS GetCallerIdentity")?;
    let user_id = super::parse::string(object, "UserId", "STS GetCallerIdentity")?;
    if account.len() != 12
        || !account.bytes().all(|byte| byte.is_ascii_digit())
        || user_id.is_empty()
    {
        return Err(Error::state_failed(
            "STS GetCallerIdentity returned invalid identity fields",
        ));
    }
    let principal = parse_principal_arn(arn, account)?;
    Ok(Identity {
        account: account.to_owned(),
        principal,
    })
}

fn parse_principal_arn(arn: &str, account: &str) -> Result<Principal> {
    let mut fields = arn.splitn(6, ':');
    let prefix = fields
        .next()
        .ok_or_else(|| Error::state_failed("STS GetCallerIdentity returned a malformed ARN"))?;
    let partition = fields
        .next()
        .ok_or_else(|| Error::state_failed("STS GetCallerIdentity returned a malformed ARN"))?;
    let service = fields
        .next()
        .ok_or_else(|| Error::state_failed("STS GetCallerIdentity returned a malformed ARN"))?;
    let region = fields
        .next()
        .ok_or_else(|| Error::state_failed("STS GetCallerIdentity returned a malformed ARN"))?;
    let arn_account = fields
        .next()
        .ok_or_else(|| Error::state_failed("STS GetCallerIdentity returned a malformed ARN"))?;
    let resource = fields
        .next()
        .ok_or_else(|| Error::state_failed("STS GetCallerIdentity returned a malformed ARN"))?;
    if prefix != "arn"
        || !(partition == "aws" || partition.starts_with("aws-"))
        || !partition
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !region.is_empty()
        || arn_account != account
        || resource.is_empty()
    {
        return Err(Error::state_failed(
            "STS GetCallerIdentity returned an ARN inconsistent with its account",
        ));
    }
    match (service, resource) {
        ("iam", "root") => Ok(Principal::Root),
        ("iam", value) if value.starts_with("user/") => {
            if valid_user_resource(value) {
                Ok(Principal::User)
            } else {
                Err(Error::state_failed(
                    "STS GetCallerIdentity returned a malformed IAM user ARN",
                ))
            }
        }
        ("sts", value) if value.starts_with("assumed-role/") => {
            if valid_assumed_role_resource(value) {
                Ok(Principal::AssumedRole)
            } else {
                Err(Error::state_failed(
                    "STS GetCallerIdentity returned a malformed assumed-role ARN",
                ))
            }
        }
        ("iam" | "sts", _) => Ok(Principal::Other),
        _ => Err(Error::state_failed(
            "STS GetCallerIdentity returned an unsupported ARN service",
        )),
    }
}

fn valid_user_resource(resource: &str) -> bool {
    resource
        .strip_prefix("user/")
        .is_some_and(|path| !path.is_empty() && path.split('/').all(|part| !part.is_empty()))
}

fn valid_assumed_role_resource(resource: &str) -> bool {
    let Some(path) = resource.strip_prefix("assumed-role/") else {
        return false;
    };
    let parts = path.split('/').collect::<Vec<_>>();
    parts.len() >= 2 && parts.iter().all(|part| !part.is_empty())
}

pub(super) async fn aws_json(
    program: &Path,
    profile: Option<&str>,
    settings: &Settings,
    service: &str,
    operation: &str,
    extra: &[&str],
) -> Result<Value> {
    let mut argv = vec![service.to_owned()];
    argv.extend(operation.split_whitespace().map(str::to_owned));
    argv.extend(extra.iter().map(|value| (*value).to_owned()));
    argv.extend(aws_base(profile, &settings.aws_region));
    let output = invoke(program, &argv, Path::new(".")).await?;
    if !output.status.success() {
        return Err(command_error(
            "AWS CLI",
            "read-only operation",
            &output.stderr,
            output.status,
        ));
    }
    parse_output(&output, "AWS CLI", "read-only operation")
}

pub(super) fn credential_or_access(
    error: Error,
    stage: Stage,
    operation: &str,
) -> Result<std::result::Result<Check, Check>> {
    match error {
        Error::PreconditionFailed(_) => Ok(Err(action_required(
            Stage::Credentials,
            format!(
                "AWS credentials are unavailable or expired during {operation}; sign in with `aws sso login` and retry."
            ),
        ))),
        Error::AwsFailed { detail, .. }
            if detail.contains("AccessDenied")
                || detail.contains("AccessDeniedException")
                || detail.contains("UnauthorizedOperation") =>
        {
            Ok(Err(blocked(
                stage,
                format!(
                    "AWS denied {operation}; grant the active assumed role the read-only permission required for this operation."
                ),
            )))
        }
        other => Err(other),
    }
}
