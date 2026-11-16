use super::*;
use std::path::PathBuf;

impl Settings {
    /// A valid preview configuration, shared by tests across the crate.
    pub(crate) fn sample(project: PathBuf) -> Self {
        Self {
            project,
            container_engine: crate::container::ContainerEngine::Docker,
            name: "portal".into(),
            aws_account_id: "123456789012".into(),
            aws_region: "us-east-1".into(),
            domain_name: "preview.example.com".into(),
            route53_zone_id: "Z12345".into(),
            github_repository: "Owner/Portal".into(),
            owner_github_id: "123456".into(),
            budget_email: None,
            alert_email: None,
            allowed_ingress_cidrs: vec!["10.0.0.0/8".into()],
            environment: "preview".into(),
            pull_request: None,
            namespace: Some("pr-38".into()),
            hostname: None,
            auth_mode: Some("guest".into()),
            spot: None,
            monthly_budget_usd: 100,
            account_baseline: true,
            network: NetworkSettings {
                nat_gateways: 0,
                fck_nat: false,
                fck_nat_ami_id: None,
                database_instance_class: "db.t4g.micro".into(),
                database_multi_az: false,
            },
            desired_count: 1,
            max_high_findings: 0,
            max_critical_findings: 0,
            plugin_secrets: vec![],
            github_oidc_provider_arn: None,
            terraform: None,
            approve: false,
        }
    }
}

type Case = (&'static str, fn(&mut Settings));

fn outcome(change: impl FnOnce(&mut Settings)) -> Result<(), &'static str> {
    let mut settings = Settings::sample(PathBuf::from("."));
    change(&mut settings);
    settings.validate().map(drop).map_err(|error| error.code())
}

#[test]
fn every_invalid_setting_is_rejected_as_invalid_input() {
    let cases: [Case; 27] = [
        ("nat gateways", |s| s.network.nat_gateways = 3),
        ("fck-nat and managed NAT conflict", |s| {
            s.network.fck_nat = true;
            s.network.nat_gateways = 1;
        }),
        ("unused fck-nat AMI ID", |s| {
            s.network.fck_nat_ami_id = Some("ami-12345678".into());
        }),
        ("invalid fck-nat AMI ID", |s| {
            s.network.fck_nat = true;
            s.network.fck_nat_ami_id = Some("ami-ABCDEF12".into());
        }),
        ("database class", |s| {
            s.network.database_instance_class = "t4g.micro".into();
        }),
        ("name too short", |s| s.name = "ab".into()),
        ("environment too short", |s| s.environment = "x".into()),
        ("account id", |s| s.aws_account_id = "123".into()),
        ("region case", |s| s.aws_region = "US-east-1".into()),
        ("empty domain label", |s| {
            s.domain_name = "bad..example.com".into();
        }),
        ("zone id", |s| s.route53_zone_id = "Z?".into()),
        ("repository", |s| {
            s.github_repository = "bad owner/repo".into();
        }),
        ("owner id overflow", |s| {
            s.owner_github_id = "18446744073709551616".into();
        }),
        ("budget email", |s| s.budget_email = Some("bad".into())),
        ("alert email", |s| s.alert_email = Some("bad".into())),
        ("cidr prefix", |s| {
            s.allowed_ingress_cidrs = vec!["10.0.0.1/33".into()];
        }),
        ("ipv6 cidr", |s| {
            s.allowed_ingress_cidrs = vec!["::1/8".into()];
        }),
        ("budget range", |s| s.monthly_budget_usd = 100_001),
        ("desired count", |s| s.desired_count = 0),
        ("hostname two labels down", |s| {
            s.hostname = Some("a.b.preview.example.com".into());
        }),
        ("hostname outside domain", |s| {
            s.hostname = Some("elsewhere.example.org".into());
        }),
        ("auth mode", |s| s.auth_mode = Some("oauth".into())),
        ("namespace trailing hyphen", |s| {
            s.namespace = Some("ends-".into());
        }),
        ("reserved secret variable", |s| {
            s.plugin_secrets = vec!["x=POSTGRES_PASSWORD".into()];
        }),
        ("secret json key", |s| {
            s.plugin_secrets = vec!["x=TOKEN:bad key".into()];
        }),
        ("secret without variable", |s| {
            s.plugin_secrets = vec!["x".into()];
        }),
        ("duplicate secret variable", |s| {
            s.plugin_secrets = vec!["one=TOKEN".into(), "two=TOKEN".into()];
        }),
    ];
    for (label, change) in cases {
        assert_eq!(outcome(change), Err("INVALID_INPUT"), "{label}");
    }
}

#[test]
fn namespaces_that_name_system_databases_are_refused() {
    let changes: [fn(&mut Settings); 4] = [
        |s| s.namespace = Some("seed".into()),
        |s| s.namespace = Some("postgres".into()),
        |s| s.namespace = Some("template0".into()),
        |s| s.namespace = Some("template1".into()),
    ];
    for change in changes {
        assert_eq!(outcome(change), Err("INVALID_INPUT"));
    }
}

#[test]
fn boundary_values_are_accepted() {
    assert_eq!(outcome(|_| ()), Ok(()));
    assert_eq!(
        outcome(|s| {
            s.monthly_budget_usd = 1;
            s.desired_count = 4;
            s.allowed_ingress_cidrs.clear();
        }),
        Ok(())
    );
    for ami_id in ["ami-1234abcd", "ami-1234567890abcdef0"] {
        assert_eq!(
            outcome(|s| {
                s.network.fck_nat = true;
                s.network.fck_nat_ami_id = Some(ami_id.into());
            }),
            Ok(())
        );
    }
}

#[test]
fn blank_values_from_unset_workflow_variables_count_as_unset() -> crate::error::Result<()> {
    let mut settings = Settings::sample(PathBuf::from("."));
    settings.namespace = Some(String::new());
    settings.hostname = Some(String::new());
    settings.auth_mode = Some(String::new());
    settings.alert_email = Some(String::new());
    settings.github_oidc_provider_arn = Some(String::new());
    settings.allowed_ingress_cidrs = vec![String::new()];
    settings.plugin_secrets = vec![String::new()];
    let validated = settings.validate()?;
    assert_eq!(validated.namespace, "preview");
    assert_eq!(validated.hostname, "preview.preview.example.com");
    assert_eq!(validated.auth_mode, "guest");
    assert_eq!(validated.settings.github_oidc_provider_arn, None);
    assert_eq!(
        validated.settings.allowed_ingress_cidrs,
        Vec::<String>::new()
    );
    Ok(())
}

#[test]
fn a_pull_request_names_its_namespace_unless_one_is_given() -> crate::error::Result<()> {
    let mut settings = Settings::sample(PathBuf::from("."));
    settings.pull_request = Some(7);
    assert_eq!(settings.validate()?.namespace, "pr-38");
    settings.namespace = None;
    let validated = settings.validate()?;
    assert_eq!(validated.namespace, "pr-7");
    assert_eq!(validated.hostname, "pr-7.preview.example.com");
    Ok(())
}

#[test]
fn domain_free_mode_derives_internal_hostnames() -> crate::error::Result<()> {
    let mut settings = Settings::sample(PathBuf::from("."));
    settings.domain_name.clear();
    settings.route53_zone_id.clear();
    settings.namespace = Some("production".into());
    assert_eq!(settings.validate()?.hostname, "portal.internal");
    settings.namespace = None;
    settings.pull_request = Some(12);
    let validated = settings.validate()?;
    assert_eq!(validated.namespace, "pr-12");
    assert_eq!(validated.hostname, "pr-12.portal.internal");
    settings.hostname = Some("custom.internal".into());
    assert_eq!(settings.validate()?.hostname, "custom.internal");
    settings.hostname = Some("pr-12.example.com".into());
    assert!(settings.validate().is_err());
    Ok(())
}

#[test]
fn exactly_one_dns_setting_is_invalid_input() {
    for (domain, zone) in [("preview.example.com", ""), ("", "Z12345")] {
        assert_eq!(
            outcome(|s| {
                s.domain_name = domain.into();
                s.route53_zone_id = zone.into();
            }),
            Err("INVALID_INPUT"),
            "domain {domain:?} zone {zone:?}"
        );
    }
    assert_eq!(
        outcome(|s| {
            s.domain_name.clear();
            s.route53_zone_id.clear();
        }),
        Ok(())
    );
}

#[test]
fn domain_free_settings_feed_terraform_with_empty_dns() -> crate::error::Result<()> {
    let mut settings = Settings::sample(PathBuf::from("."));
    settings.domain_name.clear();
    settings.route53_zone_id.clear();
    settings.namespace = Some("pr-12".into());
    let validated = settings.validate()?;
    let base = validated.base_tfvars();
    assert_eq!(base.get("domain_name"), Some(&serde_json::json!("")));
    assert_eq!(base.get("route53_zone_id"), Some(&serde_json::json!("")));
    let env = validated.env_tfvars("sha256:abc");
    assert_eq!(env.get("domain_name"), Some(&serde_json::json!("")));
    assert_eq!(
        env.get("hostname"),
        Some(&serde_json::json!("pr-12.portal.internal"))
    );
    Ok(())
}
