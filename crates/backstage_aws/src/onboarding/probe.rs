use super::{
    SetupArgs,
    model::{Check, ManualCheck, Stage, Status},
};
use crate::{
    error::{Error, Result},
    settings::Settings,
};
use command::{Output, invoke};
use serde_json::Value;
use std::time::Duration;

mod account;
mod aws;
mod command;
mod dns;
mod local;
mod parse;
#[cfg(test)]
#[path = "probe/tests.rs"]
mod tests;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const OUTPUT_LIMIT: usize = 1_048_576;

pub(super) async fn collect(settings: &Settings, args: &SetupArgs) -> Result<Vec<Check>> {
    let mut checks = Vec::new();
    let version = match aws::version(&args.aws_cli).await? {
        Ok(version) => version,
        Err(error) => return Ok(vec![error]),
    };
    checks.push(version);
    let (credentials, account) =
        match aws::identity(&args.aws_cli, args.profile.as_deref(), settings).await? {
            Ok(result) => result,
            Err(check) => return Ok(append(checks, check)),
        };
    checks.push(credentials);
    let account = match account {
        Ok(check) => check,
        Err(check) => return Ok(append(checks, check)),
    };
    checks.push(account);
    let summary =
        account::account_summary(&args.aws_cli, args.profile.as_deref(), settings).await?;
    let stopped = summary
        .iter()
        .any(|check| matches!(&check.status, Status::ActionRequired | Status::Blocked));
    checks.extend(summary);
    if stopped {
        return Ok(checks);
    }
    let organization =
        match account::organization(&args.aws_cli, args.profile.as_deref(), settings).await? {
            Ok(check) => check,
            Err(check) => return Ok(append(checks, check)),
        };
    checks.push(organization);
    if !args.confirmed.contains(&ManualCheck::OrganizationEmail) {
        return Ok(append(
            checks,
            action_required(
                Stage::OrganizationEmail,
                "Confirm that the AWS Organizations management account email is verified by the account owner.",
            ),
        ));
    }
    checks.push(acknowledged(
        Stage::OrganizationEmail,
        "Organization management-account email verification is acknowledged by the owner.",
    ));
    collect_identity_center(settings, args, checks).await
}

async fn collect_identity_center(
    settings: &Settings,
    args: &SetupArgs,
    mut checks: Vec<Check>,
) -> Result<Vec<Check>> {
    if settings.aws_region.trim().is_empty() {
        return Ok(append(
            checks,
            action_required(
                Stage::IdentityCenter,
                "Set a non-empty AWS region with `--aws-region` before regional IAM Identity Center discovery.",
            ),
        ));
    }
    let instance =
        match account::identity_center(&args.aws_cli, args.profile.as_deref(), settings).await? {
            Ok(check) => check,
            Err(check) => return Ok(append(checks, check)),
        };
    checks.push(instance);
    if !args.confirmed.contains(&ManualCheck::IdentityCenterMfa) {
        return Ok(append(
            checks,
            action_required(
                Stage::IdentityCenterMfa,
                "Confirm in the IAM Identity Center console that MFA is required by its configured authentication policy.",
            ),
        ));
    }
    checks.push(acknowledged(
        Stage::IdentityCenterMfa,
        "Identity Center MFA policy is acknowledged by the owner; no AWS API proof is available.",
    ));
    collect_deployment(settings, args, checks).await
}

async fn collect_deployment(
    settings: &Settings,
    args: &SetupArgs,
    mut checks: Vec<Check>,
) -> Result<Vec<Check>> {
    let validated = match settings.clone().validate() {
        Ok(validated) => validated,
        Err(error) => {
            return Ok(append(
                checks,
                action_required(
                    Stage::DeploymentSettings,
                    format!("Deployment settings are invalid: {error}"),
                ),
            ));
        }
    };
    if validated.settings.allowed_ingress_cidrs.is_empty() {
        return Ok(append(
            checks,
            action_required(
                Stage::DeploymentSettings,
                "At least one allowed ingress CIDR is required for the base deployment.",
            ),
        ));
    }
    checks.push(verified(
        Stage::DeploymentSettings,
        "Deployment settings validated.",
    ));

    let zone = match aws::dns(&args.aws_cli, args.profile.as_deref(), settings).await? {
        Ok(check) => check,
        Err(check) => return Ok(append(checks, check)),
    };
    checks.push(zone);
    let project = match local::project(&settings.project)? {
        Ok(check) => check,
        Err(check) => return Ok(append(checks, check)),
    };
    checks.push(project);
    match local::tools(settings).await? {
        Ok(check) => checks.push(check),
        Err(check) => return Ok(append(checks, check)),
    }
    Ok(checks)
}

fn append(mut checks: Vec<Check>, check: Check) -> Vec<Check> {
    checks.push(check);
    checks
}

fn verified(stage: Stage, detail: impl Into<String>) -> Check {
    Check {
        stage,
        status: Status::Verified,
        detail: detail.into(),
    }
}

fn blocked(stage: Stage, detail: impl Into<String>) -> Check {
    Check {
        stage,
        status: Status::Blocked,
        detail: detail.into(),
    }
}
fn action_required(stage: Stage, detail: impl Into<String>) -> Check {
    Check {
        stage,
        status: Status::ActionRequired,
        detail: detail.into(),
    }
}

fn acknowledged(stage: Stage, detail: impl Into<String>) -> Check {
    Check {
        stage,
        status: Status::Acknowledged,
        detail: detail.into(),
    }
}

fn aws_base(profile: Option<&str>, region: &str) -> Vec<String> {
    let mut argv = vec![
        "--output".into(),
        "json".into(),
        "--no-cli-pager".into(),
        "--no-paginate".into(),
    ];
    if !region.trim().is_empty() {
        argv.extend(["--region".into(), region.into()]);
    }
    if let Some(profile) = profile {
        argv.extend(["--profile".into(), profile.into()]);
    }
    argv
}

fn command_error(
    service: &'static str,
    operation: &'static str,
    stderr: &[u8],
    status: std::process::ExitStatus,
) -> Error {
    let diagnostic = String::from_utf8_lossy(stderr);
    let code = parse::error_code(&diagnostic);
    if diagnostic.contains("Unable to locate credentials") {
        return Error::precondition_failed(format!(
            "AWS credentials are unavailable during {service} {operation}; sign in with `aws sso login` and retry."
        ));
    }
    let expired_sso = diagnostic.contains("SSO session associated with this profile has expired")
        || diagnostic.contains("The SSO session has expired");
    if expired_sso {
        return Error::precondition_failed(format!(
            "AWS IAM Identity Center session expired during {service} {operation}; sign in again with `aws sso login` and retry."
        ));
    }
    match code.as_deref() {
        Some(
            "ExpiredToken"
            | "ExpiredTokenException"
            | "UnauthorizedException"
            | "InvalidGrantException"
            | "TokenRefreshRequired",
        ) => Error::precondition_failed(format!(
            "AWS credentials or IAM Identity Center session expired during {service} {operation}; sign in again with `aws sso login` and retry."
        )),
        Some("UnableToLocateCredentials" | "NoCredentialsError" | "CredentialsNotFound") => {
            Error::precondition_failed(format!(
                "AWS credentials are unavailable during {service} {operation}; configure an assumed role or sign in with `aws sso login`."
            ))
        }
        Some("AccessDenied" | "AccessDeniedException" | "UnauthorizedOperation") => {
            Error::aws_failed(service, operation, "AccessDenied: read access is required")
        }
        Some("AWSOrganizationsNotInUseException") => {
            Error::aws_failed(service, operation, "AWSOrganizationsNotInUseException")
        }
        _ => Error::aws_failed(
            service,
            operation,
            format!("AWS CLI exited with {status}; diagnostic redacted"),
        ),
    }
}

fn parse_output(output: &Output, service: &'static str, operation: &'static str) -> Result<Value> {
    if !output.status.success() {
        return Err(command_error(
            service,
            operation,
            &output.stderr,
            output.status,
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| Error::state_failed(format!("{service} {operation} returned malformed JSON")))
}
