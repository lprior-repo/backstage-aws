use super::*;
use crate::settings::{NetworkSettings, Settings};
use std::path::PathBuf;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn settings() -> Settings {
    Settings {
        project: PathBuf::from("/tmp/project with spaces;not-shell"),
        container_engine: crate::container::ContainerEngine::Docker,
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
fn missing_early_evidence_blocks_later_verified_stages_and_orders_checks() -> TestResult {
    let checks = vec![
        check(Stage::Project, Status::Verified),
        check(Stage::Account, Status::Verified),
    ];
    let report = report(&settings(), &args(), checks)?;
    assert_eq!(report.status, ReportStatus::Blocked);
    assert_eq!(
        report.checks.first().map(|item| item.stage),
        Some(Stage::AwsCli)
    );
    assert_eq!(
        report.checks.first().map(|item| item.status),
        Some(Status::NotChecked)
    );
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
    Ok(())
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
fn acknowledgements_require_matching_explicit_manual_confirmation() -> TestResult {
    let manual = vec![check(Stage::OrganizationEmail, Status::Acknowledged)];
    assert_eq!(
        report(&settings(), &args(), manual).err().map(|e| e.code()),
        Some("INVALID_INPUT")
    );
    let checks = vec![check(Stage::OrganizationEmail, Status::Acknowledged)];
    let result = report(&settings(), &confirmed_args(), checks)?;
    assert_eq!(
        result.checks.get(6).map(|item| item.status),
        Some(Status::Acknowledged)
    );
    assert_eq!(result.status, ReportStatus::Blocked);
    Ok(())
}

#[test]
fn partial_observations_never_create_a_ready_report() -> TestResult {
    let checks = Stage::ORDER
        .iter()
        .filter(|stage| !stage.manual())
        .map(|stage| check(*stage, Status::Verified))
        .collect();
    let result = report(&settings(), &confirmed_args(), checks)?;
    assert_eq!(result.status, ReportStatus::Blocked);
    assert_eq!(
        result.next.as_ref().map(|action| action.title.as_str()),
        Some("Verify organization email")
    );
    assert_eq!(
        result.checks.get(6).map(|item| item.status),
        Some(Status::NotChecked)
    );
    Ok(())
}

#[test]
fn complete_readiness_means_all_live_checks_plus_acknowledged_manual_gates() -> TestResult {
    let result = report(&settings(), &confirmed_args(), all_successful_checks())?;
    assert_eq!(result.status, ReportStatus::ReadyForDoctor);
    let doctor = result.next.ok_or("ready report directs user to doctor")?;
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
    assert_eq!(
        result.checks.get(6).map(|item| item.status),
        Some(Status::Acknowledged)
    );
    assert_eq!(
        result.checks.get(8).map(|item| item.status),
        Some(Status::Acknowledged)
    );
    Ok(())
}

#[test]
fn resume_arguments_roundtrip_through_the_real_cli_with_leading_hyphen_values() -> TestResult {
    use clap::Parser;

    let mut source = settings();
    source.project = PathBuf::from("-portal project");
    let mut setup = confirmed_args();
    setup.profile = Some(String::from("-work profile"));
    let report = report(&source, &setup, Vec::new())?;
    let parsed = crate::cli::Cli::try_parse_from(report.resume)?;
    assert_eq!(parsed.settings.project, PathBuf::from("-portal project"));
    assert_eq!(parsed.settings.aws_account_id, source.aws_account_id);
    assert_eq!(parsed.settings.github_repository, source.github_repository);
    assert_eq!(parsed.settings.network.fck_nat, source.network.fck_nat);
    let crate::cli::Command::Setup(parsed_setup) = parsed.command else {
        return Err("resume command must parse as setup".into());
    };
    assert_eq!(parsed_setup.profile.as_deref(), Some("-work profile"));
    assert_eq!(parsed_setup.aws_cli, PathBuf::from("/opt/aws cli"));
    assert_eq!(parsed_setup.confirmed, setup.confirmed);
    Ok(())
}

#[test]
fn resume_preserves_fck_nat_and_the_explicit_ami_pin() -> TestResult {
    let mut source = settings();
    source.network.nat_gateways = 0;
    source.network.fck_nat = true;
    source.network.fck_nat_ami_id = Some("ami-1234567890abcdef0".into());
    let result = report(&source, &confirmed_args(), Vec::new())?;
    assert!(
        result
            .resume
            .iter()
            .any(|argument| argument == "--fck-nat=true")
    );
    assert!(
        result
            .resume
            .iter()
            .any(|argument| argument == "--fck-nat-ami-id=ami-1234567890abcdef0")
    );
    assert!(
        result
            .resume
            .iter()
            .any(|argument| argument == "--nat-gateways=0")
    );
    Ok(())
}

#[test]
fn profile_is_not_exported_when_not_explicit() -> TestResult {
    let setup = super::super::SetupArgs {
        profile: None,
        ..args()
    };
    let result = report(&settings(), &setup, Vec::new())?;
    assert!(result.environment.is_empty());
    assert!(!result.resume.iter().any(|value| value == "--profile"));
    Ok(())
}

#[test]
fn action_uses_actual_blocker_detail_without_claiming_missing_checks_are_verified() -> TestResult {
    let checks = vec![
        check(Stage::AwsCli, Status::Verified),
        Check {
            stage: Stage::Credentials,
            status: Status::ActionRequired,
            detail: String::from("SSO session expired"),
        },
    ];
    let result = report(&settings(), &args(), checks)?;
    let action = result.next.ok_or("expected a credential action")?;
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
    assert_eq!(
        result.checks.get(1).map(|item| item.status),
        Some(Status::ActionRequired)
    );
    Ok(())
}

#[test]
fn untrusted_blocker_detail_stays_data_and_never_becomes_a_command() -> TestResult {
    let detail = String::from("denied; aws iam create-user --user-name attacker");
    let checks = vec![
        check(Stage::AwsCli, Status::Verified),
        Check {
            stage: Stage::Credentials,
            status: Status::Blocked,
            detail: detail.clone(),
        },
    ];
    let result = report(&settings(), &args(), checks)?;
    let next = result.next.ok_or("expected a blocker action")?;
    assert_eq!(next.instructions.first(), Some(&detail));
    assert!(
        next.command
            .as_ref()
            .is_none_or(|argv| !argv.contains(&detail))
    );
    Ok(())
}

#[test]
fn unreadable_account_state_never_becomes_an_enable_or_create_instruction() -> TestResult {
    let evidence = vec![Check {
        stage: Stage::AwsCli,
        status: Status::Blocked,
        detail: String::from("aws discovery denied"),
    }];
    let result = report(&settings(), &args(), evidence)?;
    let next = result.next.ok_or("expected a blocked action")?;
    assert_eq!(next.title, "Resolve blocked account discovery");
    assert!(next.command.is_none());
    assert!(
        next.instructions
            .iter()
            .any(|line| line.contains("not evidence of a missing resource"))
    );
    Ok(())
}
