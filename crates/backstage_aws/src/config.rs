use serde::{Deserialize, Serialize};

use crate::{Error, validation};

pub const MAX_CONFIG_BYTES: u64 = 65_536;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all(serialize = "camelCase"))]
struct Fields {
    name: String,
    aws_region: String,
    aws_account_id: String,
    #[serde(rename(serialize = "domainName"))]
    hostname: String,
    route53_zone_id: String,
    github_repository: String,
    owner_github_id: String,
    budget_email: String,
    alert_email: String,
    #[serde(rename(serialize = "allowedIngressCidrs"))]
    allowed_cidrs: Vec<String>,
    #[serde(default = "production")]
    environment: String,
    #[serde(default = "monthly_budget")]
    monthly_budget_usd: u32,
    #[serde(default = "enabled")]
    account_baseline: bool,
    #[serde(default)]
    single_nat_gateway: bool,
    #[serde(default = "enabled")]
    database_multi_az: bool,
    #[serde(default = "replicas")]
    portal_desired_count: u8,
    #[serde(default)]
    maximum_high_findings: u32,
    #[serde(default)]
    maximum_critical_findings: u32,
}

fn production() -> String {
    "production".to_owned()
}
const fn monthly_budget() -> u32 {
    300
}
const fn enabled() -> bool {
    true
}
const fn replicas() -> u8 {
    2
}

/// Validated non-secret configuration for the full automation template.
pub struct Config(Fields);

impl Config {
    /// Parse bounded deployment inputs without echoing rejected contents.
    ///
    /// # Errors
    /// Rejects oversized, unknown, missing or invalid fields.
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.len() > 65_536 {
            return Err(Error::InvalidConfig("file exceeds 64 KiB"));
        }
        let fields: Fields = toml::from_str(input).map_err(|_| {
            Error::InvalidConfig(
                "TOML syntax, required fields or unknown fields; see generator.example.toml",
            )
        })?;
        fields.validate()?;
        Ok(Self(fields))
    }

    pub(crate) fn render(&self, input: &str) -> String {
        input
            .replace("__BACKSTAGE_AWS_NAME__", &self.0.name)
            .replace("__BACKSTAGE_AWS_OWNER_ID__", &self.0.owner_github_id)
    }

    pub(crate) fn deployment_json(&self) -> Result<Vec<u8>, Error> {
        let json = serde_json::to_string_pretty(&self.0)
            .map_err(|_| Error::InvalidConfig("cannot encode deployment configuration"))?;
        Ok(format!("{json}\n").into_bytes())
    }
}

impl Fields {
    fn validate(&self) -> Result<(), Error> {
        self.identity_checks()
            .into_iter()
            .chain(self.deployment_checks())
            .try_for_each(|(valid, field)| {
                if valid {
                    Ok(())
                } else {
                    Err(Error::InvalidConfig(field))
                }
            })
    }

    fn identity_checks(&self) -> [(bool, &'static str); 7] {
        [
            (
                validation::label(&self.name, 3, 20)
                    && self.name.starts_with(|ch: char| ch.is_ascii_lowercase()),
                "name",
            ),
            (validation::hostname(&self.hostname), "hostname"),
            (
                validation::repository(&self.github_repository),
                "github_repository (lowercase owner/repository)",
            ),
            (
                self.owner_github_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit())
                    && !self.owner_github_id.starts_with('0')
                    && self
                        .owner_github_id
                        .parse::<u64>()
                        .is_ok_and(|id| id <= 9_007_199_254_740_991),
                "owner_github_id (positive GitHub numeric ID)",
            ),
            (
                validation::region(&self.aws_region),
                "aws_region (commercial AWS regions only)",
            ),
            (
                self.aws_account_id.len() == 12
                    && self
                        .aws_account_id
                        .bytes()
                        .all(|byte| byte.is_ascii_digit()),
                "aws_account_id",
            ),
            (
                self.route53_zone_id.starts_with('Z')
                    && (6..=33).contains(&self.route53_zone_id.len())
                    && self
                        .route53_zone_id
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()),
                "route53_zone_id",
            ),
        ]
    }

    fn deployment_checks(&self) -> [(bool, &'static str); 6] {
        [
            (
                (1..=32).contains(&self.allowed_cidrs.len())
                    && self.allowed_cidrs.iter().all(|cidr| validation::cidr(cidr)),
                "allowed_cidrs (1-32 IPv4 networks, /8 through /32)",
            ),
            (validation::email(&self.budget_email), "budget_email"),
            (validation::email(&self.alert_email), "alert_email"),
            (
                validation::label(&self.environment, 2, 20)
                    && self
                        .environment
                        .starts_with(|ch: char| ch.is_ascii_lowercase()),
                "environment",
            ),
            (
                (1..=100_000).contains(&self.monthly_budget_usd),
                "monthly_budget_usd",
            ),
            (
                (1..=4).contains(&self.portal_desired_count),
                "portal_desired_count",
            ),
        ]
    }
}
