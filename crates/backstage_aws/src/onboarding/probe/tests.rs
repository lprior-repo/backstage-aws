use super::*;
use crate::settings::NetworkSettings;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

fn settings() -> Settings {
    Settings {
        project: PathBuf::from("."),
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

fn fake_cli(directory: &TempDir, body: &str) -> PathBuf {
    let path = directory.path().join("fake-aws");
    fs::write(
        directory.path().join("aws-response.sh"),
        format!("#!/bin/sh\n{body}\n"),
    )
    .expect("write fake response");
    if !path.exists() {
        std::os::unix::fs::symlink(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/probe-aws"),
            &path,
        )
        .expect("link immutable executable");
    }
    path
}

#[test]
fn strict_json_rejects_unexpected_fields_and_wrong_types() {
    let extra: Value = serde_json::json!({"Account": "123456789012", "unexpected": "private"});
    assert!(parse::object(&extra, &["Account"], "identity").is_err());
    let wrong_type: Value = serde_json::json!({"Account": 123});
    assert!(
        parse::string(
            wrong_type.as_object().expect("object"),
            "Account",
            "identity"
        )
        .is_err()
    );
}

#[test]
fn error_codes_are_extracted_without_stderr_payload() {
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
        std::process::Command::new("true")
            .status()
            .expect("true status"),
    );
    assert!(!error.to_string().contains("secret-value"));
    assert!(error.to_string().contains("AccessDenied"));
}

#[tokio::test]
async fn identity_rejects_root_and_wrong_account() {
    let directory = TempDir::new().expect("temp dir");
    let root_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"123\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:iam::123456789012:root\"}'",
    );
    let root = aws::identity(&root_cli, None, &settings())
        .await
        .expect("identity call")
        .expect("resolved credentials")
        .1
        .expect_err("root must block");
    assert_eq!(root.status, Status::ActionRequired);
    let wrong_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"999999999999\",\"Arn\":\"arn:aws:sts::999999999999:assumed-role/test/session\"}'",
    );
    let wrong = aws::identity(&wrong_cli, None, &settings())
        .await
        .expect("identity call")
        .expect("resolved credentials")
        .1
        .expect_err("wrong account must block");
    assert_eq!(wrong.stage, Stage::Account);
}

#[tokio::test]
async fn caller_rejects_iam_users_federated_users_and_malformed_assumed_role_arns() {
    let directory = TempDir::new().expect("temp dir");
    let user_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AIDA\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:iam::123456789012:user/example\"}'",
    );
    let user = aws::identity(&user_cli, None, &settings())
        .await
        .expect("identity call")
        .expect("resolved credentials")
        .1
        .expect_err("IAM user must block");
    assert_eq!(user.stage, Stage::Account);
    let federated_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::123456789012:federated-user/example\"}'",
    );
    let federated = aws::identity(&federated_cli, None, &settings())
        .await
        .expect("identity call")
        .expect("resolved credentials")
        .1
        .expect_err("federated user must block");
    assert_eq!(federated.stage, Stage::Account);
    let malformed_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::123456789012:assumed-role/\"}'",
    );
    assert!(
        aws::identity(&malformed_cli, None, &settings())
            .await
            .is_err()
    );
    let mismatched_arn_cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::999999999999:assumed-role/test/session\"}'",
    );
    assert!(
        aws::identity(&mismatched_arn_cli, None, &settings())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn denied_and_organizations_not_in_use_are_actionable_blocks() {
    let directory = TempDir::new().expect("temp dir");
    let denied_cli = fake_cli(
        &directory,
        "echo 'An error occurred (AccessDenied): secret' >&2; exit 254",
    );
    let denied = aws::identity(&denied_cli, None, &settings())
        .await
        .expect("identity call")
        .expect_err("access denied must block");
    assert_eq!(denied.stage, Stage::Credentials);
    assert_eq!(denied.status, Status::Blocked);
    assert!(!denied.detail.contains("secret"));
    let org_cli = fake_cli(
        &directory,
        "echo 'An error occurred (AWSOrganizationsNotInUseException): secret' >&2; exit 254",
    );
    let org = account::organization(&org_cli, None, &settings())
        .await
        .expect("organization call")
        .expect_err("not-in-use must be actionable");
    assert_eq!(org.stage, Stage::Organization);
    assert_eq!(org.status, Status::ActionRequired);
    assert!(!org.detail.contains("secret"));
}

#[tokio::test]
async fn malformed_success_payload_is_an_external_protocol_error() {
    let directory = TempDir::new().expect("temp dir");
    let cli = fake_cli(&directory, "printf '{malformed'");
    let result = aws::identity(&cli, None, &settings()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn probe_rerun_uses_fresh_cli_output() {
    let directory = TempDir::new().expect("temp dir");
    let cli = fake_cli(
        &directory,
        "printf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"999999999999\",\"Arn\":\"arn:aws:sts::999999999999:assumed-role/test/session\"}'",
    );
    let first = aws::identity(&cli, None, &settings())
        .await
        .expect("first identity call")
        .expect("resolved credentials")
        .1
        .expect_err("initial account mismatch");
    fs::write(directory.path().join("aws-response.sh"), "#!/bin/sh\nprintf '%s' '{\"UserId\":\"AROATEST:session\",\"Account\":\"123456789012\",\"Arn\":\"arn:aws:sts::123456789012:assumed-role/test/session\"}'\n").expect("change fake response");
    let second = aws::identity(&cli, None, &settings())
        .await
        .expect("second identity call")
        .expect("resolved credentials")
        .1
        .expect("updated account matches");
    assert_eq!(first.stage, Stage::Account);
    assert_eq!(second.stage, Stage::Account);
    assert_eq!(second.status, Status::Verified);
}

#[test]
fn project_probe_does_not_create_or_overwrite_directories() {
    let directory = TempDir::new().expect("temp dir");
    let empty = directory.path().join("empty");
    fs::create_dir(&empty).expect("create project dir");
    let result = local::project(&empty)
        .expect("project probe")
        .expect_err("missing generated paths block");
    assert_eq!(result.stage, Stage::Project);
    assert!(!empty.join("infra").exists());
    for path in ["infra/bootstrap", "infra/base", "infra/env"] {
        fs::create_dir_all(empty.join(path)).expect("create generated directory");
    }
    let ready = local::project(&empty)
        .expect("project probe")
        .expect("complete project");
    assert_eq!(ready.status, Status::Verified);
}
