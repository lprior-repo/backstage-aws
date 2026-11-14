//! Values derived from validated settings: the variables each Terraform stack
//! receives and the GitHub Actions variables the bootstrap stack publishes.
use super::ValidatedSettings;
use serde_json::{Map, Value, json};

impl ValidatedSettings {
    pub(crate) fn database_name(&self) -> &str {
        &self.database_name
    }

    pub(crate) fn env_state_key(&self) -> String {
        format!("env/{}.tfstate", self.namespace)
    }
    /// The bootstrap stack creates the state bucket under this name, so every
    /// command can find an account's state from the settings alone.
    pub(crate) fn state_bucket(&self) -> String {
        let s = &self.settings;
        format!("{}-{}-{}-tfstate", s.name, s.aws_account_id, s.aws_region)
    }

    pub(crate) fn bootstrap_tfvars(&self) -> Value {
        let s = &self.settings;
        json!({
            "aws_account_id": s.aws_account_id,
            "aws_region": s.aws_region,
            "name": s.name,
            "state_bucket_name": self.state_bucket(),
            "existing_github_oidc_provider_arn": s.github_oidc_provider_arn,
            "enable_account_baseline": s.account_baseline,
            "budget_email": s.budget_email,
            "monthly_budget_usd": s.monthly_budget_usd,
            "github_repository": s.github_repository,
            "github_environment": s.environment,
            "owner_github_id": s.owner_github_id,
            "github_actions_variables": self.github_actions_variables(),
        })
    }

    pub(crate) fn base_tfvars(&self) -> Value {
        let s = &self.settings;
        json!({
            "aws_account_id": s.aws_account_id,
            "aws_region": s.aws_region,
            "name": s.name,
            "environment": s.environment,
            "domain_name": s.domain_name,
            "route53_zone_id": s.route53_zone_id,
            "allowed_ingress_cidrs": s.allowed_ingress_cidrs,
            "single_nat_gateway": s.network.single_nat_gateway,
            "database_multi_az": s.network.database_multi_az,
            "alert_email": s.alert_email,
            "application_secrets": self.application_secrets(),
        })
    }

    pub(crate) fn env_tfvars(&self, digest: &str) -> Value {
        let s = &self.settings;
        json!({
            "aws_account_id": s.aws_account_id,
            "aws_region": s.aws_region,
            "name": s.name,
            "namespace": self.namespace,
            "hostname": self.hostname,
            "domain_name": s.domain_name,
            "image_digest": digest,
            "desired_count": s.desired_count,
            "auth_mode": self.auth_mode,
            "database_name": self.database_name,
            "application_secrets": self.application_secrets(),
            "state_bucket_name": self.state_bucket(),
        })
    }

    /// Plugin secrets by name. The base stack creates them; every environment
    /// maps them into its task definition.
    fn application_secrets(&self) -> Value {
        let secrets = self.plugin_secrets.iter().map(|secret| {
            let spec = json!({
                "environment_variable": secret.environment_variable,
                "json_key": secret.json_key,
            });
            (secret.name.clone(), spec)
        });
        Value::Object(secrets.collect())
    }

    /// Settings every workflow run needs, published as variables of the GitHub
    /// environment. The namespace and hostname are per pull request and are
    /// deliberately absent; the bootstrap stack adds the role and state names.
    fn github_actions_variables(&self) -> Value {
        let s = &self.settings;
        let mut values = Map::new();
        let mut set =
            |key: &str, value: String| values.insert(key.to_owned(), Value::String(value));
        set("BACKSTAGE_AWS_NAME", s.name.clone());
        set("BACKSTAGE_AWS_AWS_ACCOUNT_ID", s.aws_account_id.clone());
        set("BACKSTAGE_AWS_AWS_REGION", s.aws_region.clone());
        set("BACKSTAGE_AWS_DOMAIN_NAME", s.domain_name.clone());
        set("BACKSTAGE_AWS_ROUTE53_ZONE_ID", s.route53_zone_id.clone());
        set(
            "BACKSTAGE_AWS_GITHUB_REPOSITORY",
            s.github_repository.clone(),
        );
        set("BACKSTAGE_AWS_OWNER_GITHUB_ID", s.owner_github_id.clone());
        set("BACKSTAGE_AWS_ENVIRONMENT", s.environment.clone());
        set("BACKSTAGE_AWS_AUTH_MODE", self.auth_mode.clone());
        set("BACKSTAGE_AWS_DESIRED_COUNT", s.desired_count.to_string());
        set(
            "BACKSTAGE_AWS_MAX_HIGH_FINDINGS",
            s.max_high_findings.to_string(),
        );
        set(
            "BACKSTAGE_AWS_MAX_CRITICAL_FINDINGS",
            s.max_critical_findings.to_string(),
        );
        if !s.allowed_ingress_cidrs.is_empty() {
            set(
                "BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS",
                s.allowed_ingress_cidrs.join(","),
            );
        }
        if !s.plugin_secrets.is_empty() {
            set("BACKSTAGE_AWS_PLUGIN_SECRETS", s.plugin_secrets.join(","));
        }
        Value::Object(values)
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::Result, settings::Settings};
    use serde_json::Value;
    use std::path::PathBuf;

    const BOOTSTRAP: &[&str] = &[
        include_str!("../../../../template/infra/bootstrap/variables.tf"),
        include_str!("../../../../template/infra/bootstrap/account.tf"),
        include_str!("../../../../template/infra/bootstrap/github.tf"),
    ];
    const BASE: &[&str] = &[include_str!("../../../../template/infra/base/variables.tf")];
    const ENV: &[&str] = &[include_str!("../../../../template/infra/env/variables.tf")];

    /// `(name, has_default)` for every variable block in the given sources.
    fn declared(sources: &[&str]) -> Vec<(String, bool)> {
        let mut variables: Vec<(String, bool)> = Vec::new();
        for line in sources.iter().flat_map(|source| source.lines()) {
            let name = line
                .strip_prefix("variable \"")
                .and_then(|rest| rest.strip_suffix("\" {"));
            if let Some(name) = name {
                variables.push((name.to_owned(), false));
            } else if line.starts_with("  default ") {
                if let Some(last) = variables.last_mut() {
                    last.1 = true;
                }
            }
        }
        variables
    }

    /// The CLI may only write declared variables and must write every variable
    /// that has no default; otherwise `terraform plan` fails.
    fn assert_matches_stack(vars: &Value, sources: &[&str]) {
        let written: Vec<&String> = vars
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, _)| key)
            .collect();
        let declared = declared(sources);
        assert!(!declared.is_empty());
        for key in &written {
            assert!(
                declared.iter().any(|(name, _)| name == *key),
                "undeclared: {key}"
            );
        }
        for (name, has_default) in &declared {
            assert!(
                *has_default || written.contains(&name),
                "required but not written: {name}"
            );
        }
    }

    #[test]
    fn every_stack_receives_only_declared_variables_and_every_required_one() -> Result<()> {
        let settings = Settings::sample(PathBuf::from(".")).validate()?;
        assert_matches_stack(&settings.bootstrap_tfvars(), BOOTSTRAP);
        assert_matches_stack(&settings.base_tfvars(), BASE);
        assert_matches_stack(&settings.env_tfvars("sha256:abc"), ENV);
        Ok(())
    }

    #[test]
    fn a_preview_namespace_derives_its_database_hostname_and_state_key() -> Result<()> {
        let settings = Settings::sample(PathBuf::from(".")).validate()?;
        assert_eq!(settings.namespace, "pr-38");
        assert_eq!(settings.database_name(), "pr_38");
        assert_eq!(settings.hostname, "pr-38.preview.example.com");
        assert_eq!(settings.env_state_key(), "env/pr-38.tfstate");
        assert_eq!(
            settings.state_bucket(),
            "portal-123456789012-us-east-1-tfstate"
        );
        let vars = settings.env_tfvars("sha256:abc");
        assert_eq!(
            vars.get("hostname"),
            Some(&Value::from("pr-38.preview.example.com"))
        );
        assert_eq!(vars.get("database_name"), Some(&Value::from("pr_38")));
        Ok(())
    }

    #[test]
    fn production_is_served_at_the_domain_itself() -> Result<()> {
        let mut settings = Settings::sample(PathBuf::from("."));
        settings.environment = "production".into();
        settings.namespace = None;
        let validated = settings.validate()?;
        assert_eq!(validated.namespace, "production");
        assert_eq!(validated.hostname, "preview.example.com");
        Ok(())
    }

    #[test]
    fn published_variables_never_pin_a_namespace_or_hostname() -> Result<()> {
        let settings = Settings::sample(PathBuf::from(".")).validate()?;
        let bootstrap = settings.bootstrap_tfvars();
        let published = bootstrap
            .get("github_actions_variables")
            .and_then(Value::as_object);
        let keys: Vec<&String> = published
            .into_iter()
            .flatten()
            .map(|(key, _)| key)
            .collect();
        assert!(keys.iter().any(|key| *key == "BACKSTAGE_AWS_NAME"));
        for per_pull_request in ["BACKSTAGE_AWS_NAMESPACE", "BACKSTAGE_AWS_HOSTNAME"] {
            assert!(
                !keys.iter().any(|key| *key == per_pull_request),
                "{per_pull_request}"
            );
        }
        Ok(())
    }
}
