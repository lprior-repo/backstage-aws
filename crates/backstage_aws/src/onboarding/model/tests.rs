use super::*;
use crate::settings::{NetworkSettings, Settings};
use std::path::PathBuf;

fn settings() -> Settings {
    Settings {
        project: PathBuf::from("/tmp/project with spaces;not-shell"),
        name: String::from("demo-name"),
        aws_account_id: String::from("123456789012"),
        aws_region: String::from("us-east-1"),
        domain_name: String::from("example.com"),
        route53_zone_id: String::from("Z123456789"),
        github_repository: String::from("owner/repo"),
        owner_github_id: String::from("12345"),
        budget_email: Some(String::from("budget@example.com")),
        alert_email: None,
        allowed_ingress_cidrs: vec![
            String::from("203.0.113.0/24"),
            String::from("198.51.100.4/32"),
        ],
        environment: String::from("preview"),
        pull_request: Some(27),
        namespace: Some(String::from("pr-27")),
        hostname: Some(String::from("pr-27.example.com")),
        auth_mode: Some(String::from("github")),
        spot: Some(true),
        monthly_budget_usd: 125,
        account_baseline: true,
        network: NetworkSettings {
            nat_gateways: 1,
            fck_nat: false,
            fck_nat_ami_id: None,
            database_instance_class: String::from("db.t4g.small"),
            database_multi_az: false,
        },
        desired_count: 2,
        max_high_findings: 3,
        max_critical_findings: 1,
        plugin_secrets: vec![String::from("catalog=CATALOG_TOKEN:token")],
        github_oidc_provider_arn: None,
        terraform: Some(PathBuf::from("/opt/terraform")),
        approve: true,
    }
}

fn args() -> super::super::SetupArgs {
    super::super::SetupArgs {
        aws_cli: PathBuf::from("/opt/aws cli"),
        profile: Some(String::from("work profile")),
        confirmed: Vec::new(),
    }
}

fn check(stage: Stage, status: Status) -> Check {
    Check {
        stage,
        status,
        detail: format!("observed {stage:?}"),
    }
}

fn all_successful_checks() -> Vec<Check> {
    Stage::ORDER
        .iter()
        .map(|stage| {
            let status = if stage.manual() {
                Status::Acknowledged
            } else {
                Status::Verified
            };
            check(*stage, status)
        })
        .collect()
}

fn confirmed_args() -> super::super::SetupArgs {
    super::super::SetupArgs {
        confirmed: vec![
            ManualCheck::OrganizationEmail,
            ManualCheck::IdentityCenterMfa,
        ],
        ..args()
    }
}

#[test]
fn missing_early_evidence_blocks_later_verified_stages_and_orders_checks() {
    let checks = vec![
        check(Stage::Project, Status::Verified),
        check(Stage::Account, Status::Verified),
    ];
    let report = report(&settings(), &args(), checks).expect("valid report");
    assert_eq!(report.status, ReportStatus::Blocked);
    assert_eq!(
        report.checks.first().map(|item| item.stage),
        Some(Stage::AwsCli)
    );
    assert_eq!(report.checks[0].status, Status::NotChecked);
    assert_eq!(
        report.next.as_ref().map(|item| item.title.as_str()),
        Some("Install AWS CLI v2")
    );
    assert_eq!(
        report
            .checks
            .iter()
            .find(|item| item.stage == Stage::Account)
            .map(|item| item.status),
        Some(Status::Verified)
    );
}

#[test]
fn duplicate_or_ambiguous_evidence_is_rejected() {
    let duplicate = vec![
        check(Stage::Account, Status::Verified),
        check(Stage::Account, Status::Blocked),
    ];
    assert_eq!(
        report(&settings(), &args(), duplicate)
            .err()
            .map(|e| e.code()),
        Some("INVALID_INPUT")
    );
    let illegal_ack = vec![check(Stage::Account, Status::Acknowledged)];
    assert_eq!(
        report(&settings(), &args(), illegal_ack)
            .err()
            .map(|e| e.code()),
        Some("INVALID_INPUT")
    );
}

#[test]
fn acknowledgements_require_matching_explicit_manual_confirmation() {
    let manual = vec![check(Stage::OrganizationEmail, Status::Acknowledged)];
    assert_eq!(
        report(&settings(), &args(), manual).err().map(|e| e.code()),
        Some("INVALID_INPUT")
    );
    let checks = vec![check(Stage::OrganizationEmail, Status::Acknowledged)];
    let result = report(&settings(), &confirmed_args(), checks).expect("confirmed report");
    assert_eq!(result.checks[6].status, Status::Acknowledged);
    assert_eq!(result.status, ReportStatus::Blocked);
}

#[test]
fn partial_observations_never_create_a_ready_report() {
    let checks = Stage::ORDER
        .iter()
        .filter(|stage| !stage.manual())
        .map(|stage| check(*stage, Status::Verified))
        .collect();
    let result = report(&settings(), &confirmed_args(), checks).expect("partial report");
    assert_eq!(result.status, ReportStatus::Blocked);
    assert_eq!(
        result.next.as_ref().map(|action| action.title.as_str()),
        Some("Verify organization email")
    );
    assert_eq!(result.checks[6].status, Status::NotChecked);
}

#[test]
fn complete_readiness_means_all_live_checks_plus_acknowledged_manual_gates() {
    let result =
        report(&settings(), &confirmed_args(), all_successful_checks()).expect("complete report");
    assert_eq!(result.status, ReportStatus::ReadyForDoctor);
    let doctor = result.next.expect("ready report directs user to doctor");
    assert_eq!(doctor.title, "Run doctor");
    assert!(doctor.command.as_ref().is_some_and(|argv| {
        argv.windows(2)
            .any(|pair| pair == ["backstage-aws", "doctor"])
    }));
    assert!(
        doctor
            .instructions
            .iter()
            .any(|line| line.contains("up --approve") && line.contains("charges"))
    );
    assert_eq!(result.checks.len(), Stage::ORDER.len());
    assert_eq!(result.checks[6].status, Status::Acknowledged);
    assert_eq!(result.checks[8].status, Status::Acknowledged);
}

#[test]
fn resume_is_reconstructed_as_argv_from_settings_without_shell_interpolation() {
    let mut setup = confirmed_args();
    setup.confirmed = vec![ManualCheck::OrganizationEmail];
    let result = report(&settings(), &setup, Vec::new()).expect("report");
    let command = result.resume;
    assert_eq!(command.first().map(String::as_str), Some("backstage-aws"));
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--project", "/tmp/project with spaces;not-shell"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--aws-account-id", "123456789012"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--aws-region", "us-east-1"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--allowed-ingress-cidrs", "203.0.113.0/24"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--allowed-ingress-cidrs", "198.51.100.4/32"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--name", "demo-name"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--domain-name", "example.com"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--route53-zone-id", "Z123456789"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--github-repository", "owner/repo"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--owner-github-id", "12345"])
    );
    assert!(command.windows(2).any(|pair| pair == ["--spot", "true"]));
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--database-instance-class", "db.t4g.small"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--terraform", "/opt/terraform"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--aws-cli", "/opt/aws cli"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--confirm", "organization-email"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--nat-gateways", "1"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--fck-nat", "false"])
    );
    assert!(!command.iter().any(|value| value == "--approve"));
    assert_eq!(
        result.environment.get("AWS_PROFILE"),
        Some(&String::from("work profile"))
    );
}

#[test]
fn resume_preserves_fck_nat_and_the_explicit_ami_pin() {
    let mut source = settings();
    source.network.nat_gateways = 0;
    source.network.fck_nat = true;
    source.network.fck_nat_ami_id = Some("ami-1234567890abcdef0".into());
    let result = report(&source, &confirmed_args(), Vec::new()).expect("report");
    assert!(
        result
            .resume
            .windows(2)
            .any(|pair| pair == ["--fck-nat", "true"])
    );
    assert!(
        result
            .resume
            .windows(2)
            .any(|pair| pair == ["--fck-nat-ami-id", "ami-1234567890abcdef0"])
    );
    assert!(
        result
            .resume
            .windows(2)
            .any(|pair| pair == ["--nat-gateways", "0"])
    );
}

#[test]
fn profile_is_not_exported_when_not_explicit() {
    let setup = super::super::SetupArgs {
        profile: None,
        ..args()
    };
    let result = report(&settings(), &setup, Vec::new()).expect("report");
    assert!(result.environment.is_empty());
    assert!(!result.resume.iter().any(|value| value == "--profile"));
}

#[test]
fn action_uses_actual_blocker_detail_without_claiming_missing_checks_are_verified() {
    let checks = vec![
        check(Stage::AwsCli, Status::Verified),
        Check {
            stage: Stage::Credentials,
            status: Status::ActionRequired,
            detail: String::from("SSO session expired"),
        },
    ];
    let result = report(&settings(), &args(), checks).expect("report");
    let action = result.next.expect("credential action");
    assert_eq!(
        action.instructions.first().map(String::as_str),
        Some("SSO session expired")
    );
    assert_eq!(
        action.command,
        Some(vec![
            String::from("/opt/aws cli"),
            String::from("configure"),
            String::from("sso"),
            String::from("--profile"),
            String::from("work profile")
        ])
    );
    assert_eq!(result.checks[1].status, Status::ActionRequired);
}

#[test]
fn untrusted_blocker_detail_stays_data_and_never_becomes_a_command() {
    let detail = String::from("denied; aws iam create-user --user-name attacker");
    let checks = vec![
        check(Stage::AwsCli, Status::Verified),
        Check {
            stage: Stage::Credentials,
            status: Status::Blocked,
            detail: detail.clone(),
        },
    ];
    let result = report(&settings(), &args(), checks).expect("report");
    let next = result.next.expect("blocker action");
    assert_eq!(next.instructions.first(), Some(&detail));
    assert!(
        next.command
            .as_ref()
            .is_none_or(|argv| !argv.contains(&detail))
    );
}

#[test]
fn unreadable_account_state_never_becomes_an_enable_or_create_instruction() {
    let evidence = vec![Check {
        stage: Stage::AwsCli,
        status: Status::Blocked,
        detail: String::from("aws discovery denied"),
    }];
    let result = report(&settings(), &args(), evidence).expect("report");
    let next = result.next.expect("blocked action");
    assert_eq!(next.title, "Resolve blocked account discovery");
    assert!(next.command.is_none());
    assert!(
        next.instructions
            .iter()
            .any(|line| line.contains("not evidence of a missing resource"))
    );
}
