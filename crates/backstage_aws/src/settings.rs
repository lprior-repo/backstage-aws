use crate::container::ContainerEngine;
use clap::Args;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const ENV_PROJECT: &str = "BACKSTAGE_AWS_PROJECT";
const ENV_NAME: &str = "BACKSTAGE_AWS_NAME";
const ENV_ACCOUNT: &str = "BACKSTAGE_AWS_AWS_ACCOUNT_ID";
const ENV_REGION: &str = "BACKSTAGE_AWS_AWS_REGION";
const ENV_DOMAIN: &str = "BACKSTAGE_AWS_DOMAIN_NAME";
const ENV_ZONE: &str = "BACKSTAGE_AWS_ROUTE53_ZONE_ID";
const ENV_REPOSITORY: &str = "BACKSTAGE_AWS_GITHUB_REPOSITORY";
const ENV_OWNER: &str = "BACKSTAGE_AWS_OWNER_GITHUB_ID";
const ENV_BUDGET_EMAIL: &str = "BACKSTAGE_AWS_BUDGET_EMAIL";
const ENV_ALERT_EMAIL: &str = "BACKSTAGE_AWS_ALERT_EMAIL";
const ENV_CIDRS: &str = "BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS";
const ENV_ENVIRONMENT: &str = "BACKSTAGE_AWS_ENVIRONMENT";
const ENV_MONTHLY_BUDGET: &str = "BACKSTAGE_AWS_MONTHLY_BUDGET_USD";
const ENV_BASELINE: &str = "BACKSTAGE_AWS_ACCOUNT_BASELINE";
const ENV_NAT_GATEWAYS: &str = "BACKSTAGE_AWS_NAT_GATEWAYS";
const ENV_FCK_NAT: &str = "BACKSTAGE_AWS_FCK_NAT";
const ENV_FCK_NAT_AMI_ID: &str = "BACKSTAGE_AWS_FCK_NAT_AMI_ID";
const ENV_DATABASE_CLASS: &str = "BACKSTAGE_AWS_DATABASE_INSTANCE_CLASS";
const ENV_PULL_REQUEST: &str = "BACKSTAGE_AWS_PULL_REQUEST";
const ENV_MULTI_AZ: &str = "BACKSTAGE_AWS_DATABASE_MULTI_AZ";
const ENV_DESIRED_COUNT: &str = "BACKSTAGE_AWS_DESIRED_COUNT";
const ENV_MAX_HIGH: &str = "BACKSTAGE_AWS_MAX_HIGH_FINDINGS";
const ENV_MAX_CRITICAL: &str = "BACKSTAGE_AWS_MAX_CRITICAL_FINDINGS";
const ENV_PLUGIN_SECRETS: &str = "BACKSTAGE_AWS_PLUGIN_SECRETS";
const ENV_OIDC_PROVIDER: &str = "BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN";
const ENV_TERRAFORM: &str = "BACKSTAGE_AWS_TERRAFORM";
const ENV_CONTAINER_ENGINE: &str = "BACKSTAGE_AWS_CONTAINER_ENGINE";
const ENV_APPROVE: &str = "BACKSTAGE_AWS_APPROVE";
const ENV_HOSTNAME: &str = "BACKSTAGE_AWS_HOSTNAME";
const ENV_NAMESPACE: &str = "BACKSTAGE_AWS_NAMESPACE";
const ENV_AUTH_MODE: &str = "BACKSTAGE_AWS_AUTH_MODE";
const ENV_SPOT: &str = "BACKSTAGE_AWS_SPOT";
#[derive(Args, Clone, Debug)]
pub(crate) struct Settings {
    /// The project directory that `init` created.
    #[arg(long, env=ENV_PROJECT, default_value=".", global=true)]
    pub(crate) project: PathBuf,
    /// Container command used for local development and image builds.
    #[arg(long, env=ENV_CONTAINER_ENGINE, default_value_t=ContainerEngine::Docker, value_enum, global=true)]
    pub(crate) container_engine: ContainerEngine,
    // The deployment's identity. Commands except `setup` and `check` need these;
    // they default to blank so that validation, not argument parsing, reports
    // the missing ones.
    /// Required. Prefix of every AWS resource name: 3-20 lowercase letters, digits or hyphens.
    #[arg(long, env=ENV_NAME, default_value="", global=true)]
    pub(crate) name: String,
    /// Required. The 12-digit AWS account to deploy into; other accounts are refused.
    #[arg(long, env=ENV_ACCOUNT, default_value="", global=true)]
    pub(crate) aws_account_id: String,
    /// Required. The AWS region, such as `us-east-1`.
    #[arg(long, env=ENV_REGION, default_value="", global=true)]
    pub(crate) aws_region: String,
    /// Optional. The domain environments are served under, such as `preview.example.com`. Leave empty, together with the zone ID, for domain-free plain-HTTP mode.
    #[arg(long, env=ENV_DOMAIN, default_value="", global=true)]
    pub(crate) domain_name: String,
    /// Optional. The Route 53 public hosted zone that holds the domain's records; set it exactly when the domain is set.
    #[arg(long, env=ENV_ZONE, default_value="", global=true)]
    pub(crate) route53_zone_id: String,
    /// Required. The project's GitHub repository as `owner/name`.
    #[arg(long, env=ENV_REPOSITORY, default_value="", global=true)]
    pub(crate) github_repository: String,
    /// Required. Numeric GitHub user ID of the person who may sign in to production first.
    #[arg(long, env=ENV_OWNER, default_value="", global=true)]
    pub(crate) owner_github_id: String,
    /// Receives the monthly budget alert. No budget is created without it.
    #[arg(long, env=ENV_BUDGET_EMAIL, global=true)]
    pub(crate) budget_email: Option<String>,
    /// Receives database and load balancer alarms.
    #[arg(long, env=ENV_ALERT_EMAIL, global=true)]
    pub(crate) alert_email: Option<String>,
    /// IPv4 networks that may reach the load balancer, comma-separated, /8 to /32. `base` needs one.
    #[arg(long, env=ENV_CIDRS, value_delimiter=',', global=true)]
    pub(crate) allowed_ingress_cidrs: Vec<String>,
    /// `production` or any other name; one per AWS account.
    #[arg(long, env=ENV_ENVIRONMENT, default_value="preview", global=true)]
    pub(crate) environment: String,
    /// A pull request number: names the namespace `pr-<number>` unless
    /// `--namespace` is set, and is where `env up` and `env down` comment.
    #[arg(long, env=ENV_PULL_REQUEST, global=true)]
    pub(crate) pull_request: Option<u64>,
    /// Which deployment inside the environment. Default: `pr-<number>`, else the environment name.
    #[arg(long, env=ENV_NAMESPACE, global=true)]
    pub(crate) namespace: Option<String>,
    /// Default: the domain for production, `<namespace>.<domain>` otherwise.
    #[arg(long, env=ENV_HOSTNAME, global=true)]
    pub(crate) hostname: Option<String>,
    /// `guest` or `github`. Default: `github` for production, `guest` otherwise.
    #[arg(long, env=ENV_AUTH_MODE, global=true)]
    pub(crate) auth_mode: Option<String>,
    /// Run the environment on Fargate Spot. Unset: every environment except
    /// production does.
    #[arg(long, env=ENV_SPOT, global=true)]
    pub(crate) spot: Option<bool>,
    /// The monthly spend, in US dollars, at which the budget alert fires.
    #[arg(long, env=ENV_MONTHLY_BUDGET, default_value_t=100, global=true)]
    pub(crate) monthly_budget_usd: u32,
    /// Have `bootstrap` secure the account: `CloudTrail`, password policy, S3 public-access block.
    #[arg(long, env=ENV_BASELINE, default_value_t=true, action=clap::ArgAction::Set, global=true)]
    pub(crate) account_baseline: bool,
    #[command(flatten)]
    pub(crate) network: NetworkSettings,
    /// Tasks per environment, 1 to 4.
    #[arg(long, env=ENV_DESIRED_COUNT, default_value_t=1, global=true)]
    pub(crate) desired_count: u8,
    /// `env up --scan` fails when the image has more high-severity findings than this.
    #[arg(long, env=ENV_MAX_HIGH, default_value_t=0, global=true)]
    pub(crate) max_high_findings: u32,
    /// `env up --scan` fails when the image has more critical findings than this.
    #[arg(long, env=ENV_MAX_CRITICAL, default_value_t=0, global=true)]
    pub(crate) max_critical_findings: u32,
    /// Secrets for plugins, comma-separated, each `name=ENV_VAR` or `name=ENV_VAR:json_key`.
    #[arg(long, env=ENV_PLUGIN_SECRETS, value_delimiter=',', global=true)]
    pub(crate) plugin_secrets: Vec<String>,
    /// The account's existing GitHub OIDC provider; bootstrap creates one when unset.
    #[arg(long, env=ENV_OIDC_PROVIDER, global=true)]
    pub(crate) github_oidc_provider_arn: Option<String>,
    /// The Terraform 1.17.2 executable. Default: `terraform` on `PATH`.
    #[arg(long, env=ENV_TERRAFORM, global=true)]
    pub(crate) terraform: Option<PathBuf>,
    /// Allow the command to change something. Without it, a mutating command exits 3.
    #[arg(long, env=ENV_APPROVE, default_value_t=false, action=clap::ArgAction::SetTrue, global=true)]
    pub(crate) approve: bool,
}
impl Settings {
    /// The Terraform executable: `--terraform`, else `terraform` on `PATH`.
    /// Explicit relative paths are anchored at the invocation directory so
    /// Terraform can be launched after changing into a stack directory.
    pub(crate) fn terraform_path(&self) -> PathBuf {
        let configured = self
            .terraform
            .as_ref()
            .map_or_else(|| PathBuf::from("terraform"), Clone::clone);
        if configured.is_absolute() || configured.components().count() == 1 {
            return configured;
        }
        std::env::current_dir().map_or_else(|_| configured.clone(), |cwd| cwd.join(&configured))
    }
}
#[derive(Args, Clone, Debug)]
pub(crate) struct NetworkSettings {
    /// NAT gateways for the application subnets. With none, tasks get a public
    /// address and reach the internet directly; nothing but the load balancer
    /// can reach them.
    #[arg(long, env=ENV_NAT_GATEWAYS, default_value_t=0, global=true)]
    pub(crate) nat_gateways: u8,
    /// Route private application subnets through a shared fck-nat instance.
    #[arg(long, env=ENV_FCK_NAT, default_value_t=false, action=clap::ArgAction::Set, global=true)]
    pub(crate) fck_nat: bool,
    /// Pin the fck-nat AMI. Only usable when `--fck-nat` is enabled.
    #[arg(long, env=ENV_FCK_NAT_AMI_ID, global=true)]
    pub(crate) fck_nat_ami_id: Option<String>,
    /// The database size. One instance serves every environment in the account.
    #[arg(long, env=ENV_DATABASE_CLASS, default_value="db.t4g.micro", global=true)]
    pub(crate) database_instance_class: String,
    /// Keep a standby database in a second availability zone, at twice the price.
    #[arg(long, env=ENV_MULTI_AZ, default_value_t=false, action=clap::ArgAction::Set, global=true)]
    pub(crate) database_multi_az: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PluginSecret {
    pub(crate) name: String,
    pub(crate) environment_variable: String,
    pub(crate) json_key: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct OwnerGithubId(pub(crate) u64);
#[derive(Clone, Debug)]
pub(crate) struct ValidatedSettings {
    pub(crate) settings: Settings,
    pub(crate) owner_github_id: OwnerGithubId,
    pub(crate) namespace: String,
    pub(crate) hostname: String,
    pub(crate) database_name: String,
    pub(crate) auth_mode: String,
    pub(crate) plugin_secrets: Vec<PluginSecret>,
}
mod derived;
#[cfg(test)]
mod tests;
mod validation;
