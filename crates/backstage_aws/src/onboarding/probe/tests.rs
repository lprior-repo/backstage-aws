use super::*;
use crate::settings::NetworkSettings;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

fn settings() -> Settings {
    Settings {
        project: PathBuf::from("."),
        container_engine: crate::container::ContainerEngine::Docker,
        name: "demo".into(),
        aws_account_id: "123456789012".into(),
        aws_region: "us-east-1".into(),
        domain_name: "example.com".into(),
        route53_zone_id: "Z12345".into(),
        github_repository: "owner/repo".into(),
        owner_github_id: "1234".into(),
        budget_email: None,
        alert_email: None,
        allowed_ingress_cidrs: vec!["203.0.113.1/32".into()],
        environment: "preview".into(),
        pull_request: None,
        namespace: None,
        hostname: None,
        auth_mode: None,
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
        plugin_secrets: Vec::new(),
        github_oidc_provider_arn: None,
        terraform: None,
        approve: false,
    }
}

fn fake_cli(directory: &TempDir, body: &str) -> std::io::Result<PathBuf> {
    let path = directory.path().join("fake-aws");
    fs::write(
        directory.path().join("aws-response.sh"),
        format!("#!/bin/sh\n{body}\n"),
    )?;
    if !path.exists() {
        std::os::unix::fs::symlink(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/probe-aws"),
            &path,
        )?;
    }
    Ok(path)
}

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn passed<T>(outcome: std::result::Result<T, Check>) -> std::result::Result<T, String> {
    outcome.map_err(|check| format!("unexpected {:?} check: {}", check.stage, check.detail))
}

fn blocked<T>(
    outcome: std::result::Result<T, Check>,
    expectation: &str,
) -> std::result::Result<Check, String> {
    match outcome {
        Err(check) => Ok(check),
        Ok(_) => Err(format!("expected a blocking check: {expectation}")),
    }
}

#[test]
fn strict_json_rejects_unexpected_fields_and_wrong_types() -> TestResult {
    let extra: Value = serde_json::json!({"Account": "123456789012", "unexpected": "private"});
    assert!(parse::object(&extra, &["Account"], "identity").is_err());
    let wrong_type: Value = serde_json::json!({"Account": 123});
    assert!(
        parse::string(
            wrong_type.as_object().ok_or("expected an object")?,
            "Account",
            "identity"
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn error_codes_are_extracted_without_stderr_payload() -> TestResult {
    assert_eq!(
        parse::error_code("An error occurred (AccessDenied): confidential text"),
        Some("AccessDenied".into())
    );
    assert_eq!(
        parse::error_code("plain failure with confidential text"),
        None
    );
    let error = command_error(
        "AWS CLI",
        "read-only operation",
        b"An error occurred (AccessDenied): secret-value",
        std::process::Command::new("true").status()?,
    );
    assert!(!error.to_string().contains("secret-value"));
    assert!(error.to_string().contains("AccessDenied"));
    Ok(())
}

#[tokio::test]
async fn identity_accepts_root_for_configured_account_and_rejects_wrong_account() -> TestResult {
    let directory = TempDir::new()?;
    let root_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"123\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:iam::123456789012:root\"}'",
    )?;
    let (_, root_account) = passed(aws::identity(&root_cli, None, &settings()).await?)?;
    assert!(
        matches!(&root_account, Ok(check) if check.status == Status::Verified),
        "root must verify for the configured account"
    );
    let wrong_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"999999999999\",\"Arn\":\"arn:aws:sts::999999999999:assumed-role/test/session\"}'",
    )?;
    let wrong = blocked(
        passed(aws::identity(&wrong_cli, None, &settings()).await?)?.1,
        "wrong account must block",
    )?;
    assert_eq!(wrong.stage, Stage::Account);
    Ok(())
}

#[tokio::test]
async fn caller_rejects_iam_users_federated_users_and_malformed_assumed_role_arns() -> TestResult {
    let directory = TempDir::new()?;
    let user_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AIDA\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:iam::123456789012:user/example\"}'",
    )?;
    let user = blocked(
        passed(aws::identity(&user_cli, None, &settings()).await?)?.1,
        "IAM user must block",
    )?;
    assert_eq!(user.stage, Stage::Account);
    let federated_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::123456789012:federated-user/example\"}'",
    )?;
    let federated = blocked(
        passed(aws::identity(&federated_cli, None, &settings()).await?)?.1,
        "federated user must block",
    )?;
    assert_eq!(federated.stage, Stage::Account);
    let malformed_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::123456789012:assumed-role/\"}'",
    )?;
    assert!(
        aws::identity(&malformed_cli, None, &settings())
            .await
            .is_err()
    );
    let mismatched_arn_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::999999999999:assumed-role/test/session\"}'",
    )?;
    assert!(
        aws::identity(&mismatched_arn_cli, None, &settings())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn denied_and_organizations_not_in_use_are_actionable_blocks() -> TestResult {
    let directory = TempDir::new()?;
    let denied_cli = fake_cli(
        &directory,
        "echo 'An error occurred (AccessDenied): secret' >&2; exit 254",
    )?;
    let denied = blocked(
        aws::identity(&denied_cli, None, &settings()).await?,
        "access denied must block",
    )?;
    assert_eq!(denied.stage, Stage::Credentials);
    assert_eq!(denied.status, Status::Blocked);
    assert!(!denied.detail.contains("secret"));
    let org_cli = fake_cli(
        &directory,
        "echo 'An error occurred (AWSOrganizationsNotInUseException): secret' >&2; exit 254",
    )?;
    let org = blocked(
        account::organization(&org_cli, None, &settings()).await?,
        "not-in-use must be actionable",
    )?;
    assert_eq!(org.stage, Stage::Organization);
    assert_eq!(org.status, Status::ActionRequired);
    assert!(!org.detail.contains("secret"));
    Ok(())
}

#[tokio::test]
async fn malformed_success_payload_is_an_external_protocol_error() -> TestResult {
    let directory = TempDir::new()?;
    let cli = fake_cli(&directory, "printf '{malformed'")?;
    let result = aws::identity(&cli, None, &settings()).await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn probe_rerun_uses_fresh_cli_output() -> TestResult {
    let directory = TempDir::new()?;
    let cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"999999999999\",\"Arn\":\"arn:aws:sts::999999999999:assumed-role/test/session\"}'",
    )?;
    let first = blocked(
        passed(aws::identity(&cli, None, &settings()).await?)?.1,
        "initial account mismatch",
    )?;
    fs::write(
        directory.path().join("aws-response.sh"),
        "#!/bin/sh\nprintf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::123456789012:assumed-role/test/session\"}'\n",
    )?;
    let second = passed(passed(aws::identity(&cli, None, &settings()).await?)?.1)?;
    assert_eq!(first.stage, Stage::Account);
    assert_eq!(second.stage, Stage::Account);
    assert_eq!(second.status, Status::Verified);
    Ok(())
}

#[test]
fn project_probe_does_not_create_or_overwrite_directories() -> TestResult {
    let directory = TempDir::new()?;
    let empty = directory.path().join("empty");
    fs::create_dir(&empty)?;
    let result = blocked(local::project(&empty)?, "missing generated paths block")?;
    assert_eq!(result.stage, Stage::Project);
    assert!(!empty.join("infra").exists());
    for path in ["infra/bootstrap", "infra/base", "infra/env"] {
        fs::create_dir_all(empty.join(path))?;
    }
    let ready = passed(local::project(&empty)?)?;
    assert_eq!(ready.status, Status::Verified);
    Ok(())
}

#[tokio::test]
async fn organization_outside_the_configured_account_is_blocked() -> TestResult {
    use crate::onboarding::model::{Stage, Status};
    let response = r#"{"Organization":{"Id":"o-example","MasterAccountId":"999999999999"}}"#;
    let directory = TempDir::new()?;
    let cli = fake_cli(
        &directory,
        &format!(
            "case \"$1/$2\" in organizations/describe-organization) echo '{response}';; *) exit 64;; esac"
        ),
    )?;
    let check = account::organization(&cli, None, &settings()).await?.err();
    assert_eq!(
        check.map(|value| (value.stage, value.status)),
        Some((Stage::Organization, Status::Blocked))
    );
    Ok(())
}
