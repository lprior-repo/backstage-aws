use super::{Action, Check, Stage};
use crate::settings::Settings;
pub(super) fn doctor_action(settings: &Settings) -> crate::error::Result<Action> {
    let mut command = vec![String::from("backstage-aws"), String::from("doctor")];
    append_settings(&mut command, settings)?;
    Ok(Action {
        title: String::from("Run doctor"),
        instructions: vec![
            String::from("Run the read-only doctor checks before deployment."),
            String::from(
                "Only after reviewing doctor results, use `backstage-aws up --approve` if you explicitly approve AWS resource changes and charges.",
            ),
        ],
        command: Some(command),
        url: None,
    })
}

pub(super) fn resume_command(
    settings: &Settings,
    args: &super::super::SetupArgs,
) -> crate::error::Result<Vec<String>> {
    let mut command = vec![String::from("backstage-aws"), String::from("setup")];
    append_settings(&mut command, settings)?;
    append_pair(&mut command, "--aws-cli", path_value(&args.aws_cli)?);
    if let Some(profile) = &args.profile {
        append_pair(&mut command, "--profile", profile);
    }
    Ok(command)
}

fn path_value(path: &std::path::Path) -> crate::error::Result<&str> {
    path.to_str()
        .ok_or_else(|| crate::error::Error::invalid_input("setup resume path is not valid UTF-8"))
}

fn append_settings(command: &mut Vec<String>, settings: &Settings) -> crate::error::Result<()> {
    append_pair(command, "--project", path_value(&settings.project)?);
    append_pair(command, "--name", &settings.name);
    append_pair(command, "--aws-account-id", &settings.aws_account_id);
    append_pair(command, "--aws-region", &settings.aws_region);
    append_pair(command, "--domain-name", &settings.domain_name);
    append_pair(command, "--route53-zone-id", &settings.route53_zone_id);
    append_pair(command, "--github-repository", &settings.github_repository);
    append_pair(command, "--owner-github-id", &settings.owner_github_id);
    append_optional(command, "--budget-email", settings.budget_email.as_deref());
    append_optional(command, "--alert-email", settings.alert_email.as_deref());
    settings.allowed_ingress_cidrs.iter().for_each(|value| {
        append_pair(command, "--allowed-ingress-cidrs", value);
    });
    append_pair(command, "--environment", &settings.environment);
    append_optional(
        command,
        "--pull-request",
        settings
            .pull_request
            .map(|value| value.to_string())
            .as_deref(),
    );
    append_optional(command, "--namespace", settings.namespace.as_deref());
    append_optional(command, "--hostname", settings.hostname.as_deref());
    append_optional(command, "--auth-mode", settings.auth_mode.as_deref());
    append_optional(
        command,
        "--spot",
        settings.spot.map(|value| value.to_string()).as_deref(),
    );
    append_pair(command, "--monthly-budget-usd", settings.monthly_budget_usd);
    append_pair(command, "--account-baseline", settings.account_baseline);
    append_network(command, settings);
    append_pair(command, "--desired-count", settings.desired_count);
    append_pair(command, "--max-high-findings", settings.max_high_findings);
    append_pair(
        command,
        "--max-critical-findings",
        settings.max_critical_findings,
    );
    settings
        .plugin_secrets
        .iter()
        .for_each(|value| append_pair(command, "--plugin-secrets", value));
    append_optional(
        command,
        "--github-oidc-provider-arn",
        settings.github_oidc_provider_arn.as_deref(),
    );
    if let Some(terraform) = &settings.terraform {
        append_pair(command, "--terraform", path_value(terraform)?);
    }
    Ok(())
}

fn append_network(command: &mut Vec<String>, settings: &Settings) {
    append_pair(command, "--nat-gateways", settings.network.nat_gateways);
    append_pair(command, "--fck-nat", settings.network.fck_nat);
    append_optional(
        command,
        "--fck-nat-ami-id",
        settings.network.fck_nat_ami_id.as_deref(),
    );
    append_pair(
        command,
        "--database-instance-class",
        &settings.network.database_instance_class,
    );
    append_pair(
        command,
        "--database-multi-az",
        settings.network.database_multi_az,
    );
}

fn append_optional(command: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        append_pair(command, flag, value);
    }
}

fn append_pair(command: &mut Vec<String>, flag: &str, value: impl std::fmt::Display) {
    command.push(format!("{flag}={value}"));
}

pub(super) fn action(settings: &Settings, args: &super::super::SetupArgs, check: &Check) -> Action {
    if check.status == super::Status::Blocked {
        return Action {
            title: String::from("Resolve blocked account discovery"),
            instructions: vec![
                check.detail.clone(),
                String::from(
                    "Resolve the reported identity, permission or ambiguous-state blocker, then rerun setup. Unreadable state is not evidence of a missing resource.",
                ),
            ],
            command: None,
            url: None,
        };
    }
    let mut instructions = vec![check.detail.clone()];
    let (title, command, url) = match check.stage {
        Stage::AwsCli => (
            "Install AWS CLI v2",
            None,
            Some("https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html"),
        ),
        Stage::Credentials => credentials_action(args, &mut instructions),
        Stage::Account => {
            instructions.push(format!(
                "Set --aws-account-id to the intended 12-digit target ({}); setup never adopts the caller's discovered account.",
                settings.aws_account_id
            ));
            ("Set the target AWS account", None, None)
        }
        Stage::Organization => {
            instructions.push(String::from("Create or select the intended AWS Organization in the AWS console, then rerun setup."));
            instructions.push(String::from("Creating an organization may change Free Tier billing or expire credits; review that impact first."));
            (
                "Set up AWS Organizations",
                None,
                Some("https://console.aws.amazon.com/organizations/"),
            )
        }
        Stage::DeploymentSettings => ("Fix deployment settings", None, None),
        Stage::Project => {
            instructions.push(format!(
                "Use the configured project directory: {}.",
                settings.project.display()
            ));
            ("Initialize or select a project directory", None, None)
        }
        Stage::LocalTools => ("Install required local tools", None, None),
    };
    Action {
        title: String::from(title),
        instructions,
        command,
        url: url.map(String::from),
    }
}

fn credentials_action(
    args: &super::super::SetupArgs,
    instructions: &mut Vec<String>,
) -> (&'static str, Option<Vec<String>>, Option<&'static str>) {
    let mut command = vec![
        args.aws_cli.display().to_string(),
        String::from("configure"),
        String::from("sso"),
    ];
    if let Some(profile) = &args.profile {
        command.extend([String::from("--profile"), profile.clone()]);
    }
    instructions.push(String::from("Sign in with an assumed role through AWS IAM Identity Center; do not use long-lived IAM user keys."));
    (
        "Configure AWS sign-in",
        Some(command),
        Some("https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sso.html"),
    )
}
