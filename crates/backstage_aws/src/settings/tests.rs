use super::*;
use std::path::PathBuf;

impl Settings {
    /// A valid preview configuration, shared by tests across the crate.
    pub(crate) fn sample(project: PathBuf) -> Self {
        Self {
            project,
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
            namespace: Some("pr-38".into()),
            hostname: None,
            auth_mode: Some("guest".into()),
            monthly_budget_usd: 300,
            account_baseline: true,
            network: NetworkSettings {
                single_nat_gateway: true,
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

fn outcome(change: fn(&mut Settings)) -> Result<(), &'static str> {
    let mut settings = Settings::sample(PathBuf::from("."));
    change(&mut settings);
    settings.validate().map(drop).map_err(|error| error.code())
}

#[test]
fn every_invalid_setting_is_rejected_as_invalid_input() {
    let cases: [(&str, fn(&mut Settings)); 22] = [
        ("name too short", |s| s.name = "ab".into()),
        ("environment too short", |s| s.environment = "x".into()),
        ("account id", |s| s.aws_account_id = "123".into()),
        ("region case", |s| s.aws_region = "US-east-1".into()),
        ("empty domain label", |s| {
            s.domain_name = "bad..example.com".into()
        }),
        ("zone id", |s| s.route53_zone_id = "Z?".into()),
        ("repository", |s| {
            s.github_repository = "bad owner/repo".into()
        }),
        ("owner id overflow", |s| {
            s.owner_github_id = "18446744073709551616".into()
        }),
        ("budget email", |s| s.budget_email = Some("bad".into())),
        ("alert email", |s| s.alert_email = Some("bad".into())),
        ("cidr prefix", |s| {
            s.allowed_ingress_cidrs = vec!["10.0.0.1/33".into()]
        }),
        ("ipv6 cidr", |s| {
            s.allowed_ingress_cidrs = vec!["::1/8".into()]
        }),
        ("budget range", |s| s.monthly_budget_usd = 100_001),
        ("desired count", |s| s.desired_count = 0),
        ("hostname two labels down", |s| {
            s.hostname = Some("a.b.preview.example.com".into())
        }),
        ("hostname outside domain", |s| {
            s.hostname = Some("elsewhere.example.org".into())
        }),
        ("auth mode", |s| s.auth_mode = Some("oauth".into())),
        ("namespace trailing hyphen", |s| {
            s.namespace = Some("ends-".into())
        }),
        ("reserved secret variable", |s| {
            s.plugin_secrets = vec!["x=POSTGRES_PASSWORD".into()]
        }),
        ("secret json key", |s| {
            s.plugin_secrets = vec!["x=TOKEN:bad key".into()]
        }),
        ("secret without variable", |s| {
            s.plugin_secrets = vec!["x".into()]
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
    for change in [
        (|s| s.namespace = Some("seed".into())) as fn(&mut Settings),
        |s| s.namespace = Some("postgres".into()),
        |s| s.namespace = Some("template0".into()),
        |s| s.namespace = Some("template1".into()),
    ] {
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
    assert!(validated.settings.allowed_ingress_cidrs.is_empty());
    Ok(())
}
