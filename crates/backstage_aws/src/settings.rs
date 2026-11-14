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
const ENV_SINGLE_NAT: &str = "BACKSTAGE_AWS_SINGLE_NAT_GATEWAY";
const ENV_MULTI_AZ: &str = "BACKSTAGE_AWS_DATABASE_MULTI_AZ";
const ENV_DESIRED_COUNT: &str = "BACKSTAGE_AWS_DESIRED_COUNT";
const ENV_MAX_HIGH: &str = "BACKSTAGE_AWS_MAX_HIGH_FINDINGS";
const ENV_MAX_CRITICAL: &str = "BACKSTAGE_AWS_MAX_CRITICAL_FINDINGS";
const ENV_PLUGIN_SECRETS: &str = "BACKSTAGE_AWS_PLUGIN_SECRETS";
const ENV_OIDC_PROVIDER: &str = "BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN";
const ENV_TERRAFORM: &str = "BACKSTAGE_AWS_TERRAFORM";
const ENV_APPROVE: &str = "BACKSTAGE_AWS_APPROVE";
const ENV_HOSTNAME: &str = "BACKSTAGE_AWS_HOSTNAME";
const ENV_NAMESPACE: &str = "BACKSTAGE_AWS_NAMESPACE";
const ENV_AUTH_MODE: &str = "BACKSTAGE_AWS_AUTH_MODE";
#[derive(Args, Clone, Debug)]
pub(crate) struct Settings {
    #[arg(long, env=ENV_PROJECT, default_value=".", global=true)]
    pub(crate) project: PathBuf,
    // The deployment's identity. Every command except `check` needs these;
    // they default to blank so that validation, not argument parsing, reports
    // the missing ones.
    #[arg(long, env=ENV_NAME, default_value="", global=true)]
    pub(crate) name: String,
    #[arg(long, env=ENV_ACCOUNT, default_value="", global=true)]
    pub(crate) aws_account_id: String,
    #[arg(long, env=ENV_REGION, default_value="", global=true)]
    pub(crate) aws_region: String,
    #[arg(long, env=ENV_DOMAIN, default_value="", global=true)]
    pub(crate) domain_name: String,
    #[arg(long, env=ENV_ZONE, default_value="", global=true)]
    pub(crate) route53_zone_id: String,
    #[arg(long, env=ENV_REPOSITORY, default_value="", global=true)]
    pub(crate) github_repository: String,
    #[arg(long, env=ENV_OWNER, default_value="", global=true)]
    pub(crate) owner_github_id: String,
    #[arg(long, env=ENV_BUDGET_EMAIL, global=true)]
    pub(crate) budget_email: Option<String>,
    #[arg(long, env=ENV_ALERT_EMAIL, global=true)]
    pub(crate) alert_email: Option<String>,
    #[arg(long, env=ENV_CIDRS, value_delimiter=',', global=true)]
    pub(crate) allowed_ingress_cidrs: Vec<String>,
    #[arg(long, env=ENV_ENVIRONMENT, default_value="preview", global=true)]
    pub(crate) environment: String,
    #[arg(long, env=ENV_NAMESPACE, global=true)]
    pub(crate) namespace: Option<String>,
    #[arg(long, env=ENV_HOSTNAME, global=true)]
    pub(crate) hostname: Option<String>,
    #[arg(long, env=ENV_AUTH_MODE, global=true)]
    pub(crate) auth_mode: Option<String>,
    #[arg(long, env=ENV_MONTHLY_BUDGET, default_value_t=300, global=true)]
    pub(crate) monthly_budget_usd: u32,
    #[arg(long, env=ENV_BASELINE, default_value_t=true, action=clap::ArgAction::Set, global=true)]
    pub(crate) account_baseline: bool,
    #[command(flatten)]
    pub(crate) network: NetworkSettings,
    #[arg(long, env=ENV_DESIRED_COUNT, default_value_t=1, global=true)]
    pub(crate) desired_count: u8,
    #[arg(long, env=ENV_MAX_HIGH, default_value_t=0, global=true)]
    pub(crate) max_high_findings: u32,
    #[arg(long, env=ENV_MAX_CRITICAL, default_value_t=0, global=true)]
    pub(crate) max_critical_findings: u32,
    #[arg(long, env=ENV_PLUGIN_SECRETS, value_delimiter=',', global=true)]
    pub(crate) plugin_secrets: Vec<String>,
    /// The account's existing GitHub OIDC provider; bootstrap creates one when unset.
    #[arg(long, env=ENV_OIDC_PROVIDER, global=true)]
    pub(crate) github_oidc_provider_arn: Option<String>,
    #[arg(long, env=ENV_TERRAFORM, global=true)]
    pub(crate) terraform: Option<PathBuf>,
    #[arg(long, env=ENV_APPROVE, default_value_t=false, action=clap::ArgAction::SetTrue, global=true)]
    pub(crate) approve: bool,
}
impl Settings {
    /// The Terraform executable: `--terraform`, else `terraform` on `PATH`.
    pub(crate) fn terraform_path(&self) -> PathBuf {
        self.terraform
            .clone()
            .unwrap_or_else(|| PathBuf::from("terraform"))
    }
}
#[derive(Args, Clone, Debug)]
pub(crate) struct NetworkSettings {
    #[arg(long, env=ENV_SINGLE_NAT, default_value_t=true, action=clap::ArgAction::Set, global=true)]
    pub(crate) single_nat_gateway: bool,
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
