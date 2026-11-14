//! Values derived from validated settings: the variables each Terraform stack
//! receives and the settings file committed to the project.
use super::ValidatedSettings;
use serde_json::{Value, json};

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

    /// The role CI assumes; the bootstrap stack creates it under this name.
    pub(crate) fn deploy_role_arn(&self) -> String {
        let s = &self.settings;
        format!(
            "arn:aws:iam::{}:role/{}-github-deploy",
            s.aws_account_id, s.name
        )
    }

    /// Spot capacity costs about a third as much and can be reclaimed at two
    /// minutes' notice, which production must not be.
    fn spot(&self) -> bool {
        let s = &self.settings;
        s.spot.unwrap_or(s.environment != "production")
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
            "environment": s.environment,
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
            "nat_gateways": s.network.nat_gateways,
            "database_instance_class": s.network.database_instance_class,
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
            "spot": self.spot(),
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

    /// `environments/<environment>.env`: everything a later run needs, in the
    /// form the CLI reads it. The namespace and hostname are per pull request
    /// and are deliberately absent.
    pub(crate) fn settings_file(&self) -> String {
        let s = &self.settings;
        let mut lines = vec![
            format!(
                "# Settings for the `{}` environment, written by backstage-aws.",
                s.environment
            ),
            format!(
                "# Load them with: set -a; . environments/{}.env; set +a",
                s.environment
            ),
        ];
        let mut set = |key: &str, value: &dyn std::fmt::Display| {
            lines.push(format!(
                "BACKSTAGE_AWS_{key}='{}'",
                value.to_string().replace('\'', "'\\''")
            ));
        };
        set("NAME", &s.name);
        set("AWS_ACCOUNT_ID", &s.aws_account_id);
        set("AWS_REGION", &s.aws_region);
        set("DOMAIN_NAME", &s.domain_name);
        set("ROUTE53_ZONE_ID", &s.route53_zone_id);
        set("GITHUB_REPOSITORY", &s.github_repository);
        set("OWNER_GITHUB_ID", &s.owner_github_id);
        set("ENVIRONMENT", &s.environment);
        set("AUTH_MODE", &self.auth_mode);
        set("ALLOWED_INGRESS_CIDRS", &s.allowed_ingress_cidrs.join(","));
        set("PLUGIN_SECRETS", &s.plugin_secrets.join(","));
        set("DESIRED_COUNT", &s.desired_count);
        set("MAX_HIGH_FINDINGS", &s.max_high_findings);
        set("MAX_CRITICAL_FINDINGS", &s.max_critical_findings);
        set("NAT_GATEWAYS", &s.network.nat_gateways);
        set(
            "DATABASE_INSTANCE_CLASS",
            &s.network.database_instance_class,
        );
        set("DATABASE_MULTI_AZ", &s.network.database_multi_az);
        set("SPOT", &self.spot());
        set("ACCOUNT_BASELINE", &s.account_baseline);
        set("MONTHLY_BUDGET_USD", &s.monthly_budget_usd);
        let blank = String::new();
        set("BUDGET_EMAIL", s.budget_email.as_ref().unwrap_or(&blank));
        set("ALERT_EMAIL", s.alert_email.as_ref().unwrap_or(&blank));
        let provider = s.github_oidc_provider_arn.as_ref().unwrap_or(&blank);
        set("GITHUB_OIDC_PROVIDER_ARN", provider);
        // Not a CLI setting: the workflow passes it to the AWS sign-in step.
        set("DEPLOY_ROLE_ARN", &self.deploy_role_arn());
        lines.push(String::new());
        lines.join("\n")
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
        let spot = |vars: Value| vars.get("spot").cloned();
        assert_eq!(spot(validated.env_tfvars("sha256:abc")), Some(false.into()));
        settings.spot = Some(true);
        let vars = settings.validate()?.env_tfvars("sha256:abc");
        assert_eq!(spot(vars), Some(true.into()));
        Ok(())
    }

    #[test]
    fn the_settings_file_roundtrips_literal_values_without_pinning_a_namespace()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut settings = Settings::sample(PathBuf::from("."));
        settings.plugin_secrets = vec!["pagerduty=PAGERDUTY_TOKEN:token".into()];
        settings.alert_email = Some("o'hara+${HOME}@example.com".into());
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("settings.env");
        std::fs::write(&path, settings.validate()?.settings_file())?;
        let output = std::process::Command::new("bash")
            .env_clear()
            .args([
                "--noprofile",
                "--norc",
                "-c",
                "set -a; . \"$1\"; env -0",
                "test",
            ])
            .arg(path)
            .output()?;
        assert!(output.status.success(), "{:?}", output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let values: std::collections::HashMap<&str, &str> = stdout
            .split('\0')
            .filter_map(|line| line.split_once('='))
            .collect();
        assert_eq!(values.get("BACKSTAGE_AWS_NAME"), Some(&"portal"));
        assert_eq!(
            values.get("BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS"),
            Some(&"10.0.0.0/8")
        );
        assert_eq!(
            values.get("BACKSTAGE_AWS_PLUGIN_SECRETS"),
            Some(&"pagerduty=PAGERDUTY_TOKEN:token")
        );
        assert_eq!(
            values.get("BACKSTAGE_AWS_ALERT_EMAIL"),
            Some(&"o'hara+${HOME}@example.com")
        );
        for key in ["NAMESPACE", "HOSTNAME", "PULL_REQUEST", "APPROVE"] {
            assert!(!values.contains_key(format!("BACKSTAGE_AWS_{key}").as_str()));
        }
        Ok(())
    }
}
