use super::{OwnerGithubId, PluginSecret, Settings, ValidatedSettings};
use crate::error::{Error, Result};
use regex::Regex;
use std::net::Ipv4Addr;

fn matches(value: &str, pattern: &str) -> bool {
    Regex::new(pattern).is_ok_and(|regex| regex.is_match(value))
}

fn require(value: &str, pattern: &str, label: &str) -> Result<()> {
    if !matches(value, pattern) {
        return Err(Error::invalid_input(format!(
            "{label} is missing or invalid: {value:?} does not match {pattern}"
        )));
    }
    Ok(())
}

fn valid_domain(value: &str) -> bool {
    value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn validate_derived_names(settings: &Settings) -> Result<(String, String, String)> {
    // An explicit namespace wins, then the pull request, then the environment.
    let namespace = match (settings.namespace.as_deref(), settings.pull_request) {
        (Some(namespace), _) => namespace.to_owned(),
        (None, Some(number)) => format!("pr-{number}"),
        (None, None) => settings.environment.clone(),
    };
    require(&namespace, r"^[a-z][a-z0-9-]{0,30}$", "namespace")?;
    // Names of system databases and of the base stack's log groups and task families.
    let reserved = [
        "seed",
        "postgres",
        "template0",
        "template1",
        "portal",
        "vpc-flow",
    ];
    if namespace.ends_with('-')
        || namespace.starts_with("db-admin")
        || reserved.contains(&namespace.as_str())
    {
        return Err(Error::invalid_input(format!(
            "namespace {namespace:?} is reserved or ends with a hyphen"
        )));
    }
    let hostname = settings.hostname.clone().unwrap_or_else(|| {
        if namespace == "production" {
            settings.domain_name.clone()
        } else {
            format!("{namespace}.{}", settings.domain_name)
        }
    });
    let suffix = format!(".{}", settings.domain_name);
    if !valid_domain(&hostname)
        || (hostname != settings.domain_name
            && hostname
                .strip_suffix(&suffix)
                .is_none_or(|prefix| prefix.contains('.')))
    {
        return Err(Error::invalid_input(
            "hostname must be the domain or exactly one label below it",
        ));
    }
    let auth_mode = settings.auth_mode.clone().unwrap_or_else(|| {
        if settings.environment == "production" {
            "github"
        } else {
            "guest"
        }
        .to_owned()
    });
    if !["guest", "github"].contains(&auth_mode.as_str()) {
        return Err(Error::invalid_input("auth mode must be guest or github"));
    }
    Ok((namespace, hostname, auth_mode))
}

impl Settings {
    /// Validates the settings. A blank value counts as unset, because a
    /// workflow maps every variable into the environment whether it is
    /// defined or not.
    pub(crate) fn validate(&self) -> Result<ValidatedSettings> {
        let mut settings = self.clone();
        let blank =
            |value: &Option<String>| value.as_deref().is_some_and(|text| text.trim().is_empty());
        for value in [
            &mut settings.namespace,
            &mut settings.hostname,
            &mut settings.auth_mode,
            &mut settings.budget_email,
            &mut settings.alert_email,
            &mut settings.github_oidc_provider_arn,
        ] {
            if blank(value) {
                *value = None;
            }
        }
        settings
            .allowed_ingress_cidrs
            .retain(|cidr| !cidr.is_empty());
        settings.plugin_secrets.retain(|secret| !secret.is_empty());
        settings.validated()
    }

    fn validated(&self) -> Result<ValidatedSettings> {
        require(&self.name, r"^[a-z][a-z0-9-]{2,19}$", "name")?;
        require(&self.environment, r"^[a-z][a-z0-9-]{1,19}$", "environment")?;
        require(&self.aws_account_id, r"^[0-9]{12}$", "AWS account ID")?;
        require(&self.aws_region, r"^[a-z]{2}-[a-z]+-[0-9]+$", "AWS region")?;
        if !valid_domain(&self.domain_name) {
            return Err(Error::invalid_input("invalid domain name"));
        }
        require(
            &self.route53_zone_id,
            r"^Z[A-Z0-9]{5,32}$",
            "Route 53 zone ID",
        )?;
        validate_repository(&self.github_repository)?;
        require(
            &self.owner_github_id,
            r"^[1-9][0-9]{0,15}$",
            "owner GitHub ID",
        )?;
        let owner = self
            .owner_github_id
            .parse::<u64>()
            .map_err(|_| Error::invalid_input("invalid owner GitHub ID"))?;
        for (email, label) in [
            (self.budget_email.as_deref(), "budget email"),
            (self.alert_email.as_deref(), "alert email"),
        ] {
            if email.is_some_and(|value| !matches(value, r"^[^\s@]+@[^\s@]+\.[^\s@]+$")) {
                return Err(Error::invalid_input(format!("invalid {label}")));
            }
        }
        validate_cidrs(&self.allowed_ingress_cidrs)?;
        self.validate_sizes()?;
        let (namespace, hostname, auth_mode) = validate_derived_names(self)?;
        let plugin_secrets = self
            .plugin_secrets
            .iter()
            .map(|raw| parse_plugin(raw))
            .collect::<Result<Vec<_>>>()?;
        for (index, secret) in plugin_secrets.iter().enumerate() {
            if plugin_secrets
                .iter()
                .skip(index.saturating_add(1))
                .any(|other| {
                    other.name == secret.name
                        || other.environment_variable == secret.environment_variable
                })
            {
                return Err(Error::invalid_input(
                    "duplicate application secret name or environment variable",
                ));
            }
        }
        Ok(ValidatedSettings {
            settings: self.clone(),
            owner_github_id: OwnerGithubId(owner),
            namespace: namespace.clone(),
            hostname,
            database_name: namespace.replace('-', "_"),
            auth_mode,
            plugin_secrets,
        })
    }
}
impl Settings {
    fn validate_sizes(&self) -> Result<()> {
        if !(1..=100_000).contains(&self.monthly_budget_usd)
            || !(1..=4).contains(&self.desired_count)
            || self.network.nat_gateways > 2
        {
            return Err(Error::invalid_input(
                "budget, desired count, or NAT gateway count is out of range",
            ));
        }
        require(
            &self.network.database_instance_class,
            r"^db\.[a-z0-9]+\.[a-z0-9]+$",
            "database instance class",
        )
    }
}

fn validate_repository(repo: &str) -> Result<()> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || !(1..=39).contains(&owner.len())
        || !(1..=100).contains(&name.len())
        || !owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Error::invalid_input("invalid GitHub repository"));
    }
    Ok(())
}

fn validate_cidrs(cidrs: &[String]) -> Result<()> {
    if cidrs.len() > 50 {
        return Err(Error::invalid_input("at most 50 IPv4 ingress CIDRs"));
    }
    for cidr in cidrs {
        let valid = cidr.split_once('/').is_some_and(|(ip, prefix)| {
            ip.parse::<Ipv4Addr>().is_ok()
                && prefix
                    .parse::<u8>()
                    .is_ok_and(|value| (8..=32).contains(&value))
        });
        if !valid {
            return Err(Error::invalid_input("invalid IPv4 ingress CIDR"));
        }
    }
    Ok(())
}

fn parse_plugin(raw: &str) -> Result<PluginSecret> {
    let (name, rest) = raw
        .split_once('=')
        .ok_or_else(|| Error::invalid_input("invalid application secret"))?;
    let (environment_variable, json_key) = rest
        .split_once(':')
        .map_or((rest, None), |(env, key)| (env, Some(key)));
    require(name, r"^[a-z][a-z0-9_-]{0,40}$", "application secret name")?;
    require(
        environment_variable,
        r"^[A-Z][A-Z0-9_]{0,80}$",
        "secret environment variable",
    )?;
    if json_key.is_some_and(|key| !matches(key, r"^[A-Za-z0-9_-]{1,80}$"))
        || [
            "AWS_",
            "POSTGRES_",
            "AUTH_",
            "NODE_",
            "APP_",
            "BACKEND_",
            "PGSSL",
        ]
        .iter()
        .any(|prefix| environment_variable.starts_with(prefix))
    {
        return Err(Error::invalid_input(
            "invalid or reserved application secret",
        ));
    }
    Ok(PluginSecret {
        name: name.to_owned(),
        environment_variable: environment_variable.to_owned(),
        json_key: json_key.map(str::to_owned),
    })
}
