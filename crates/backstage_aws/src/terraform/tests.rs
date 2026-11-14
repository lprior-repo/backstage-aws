use super::*;
use aws_sdk_s3::{
    Client,
    operation::{
        get_object::{GetObjectError, GetObjectOutput},
        head_bucket::{HeadBucketError, HeadBucketOutput},
    },
    primitives::ByteStream,
    types::error::{NoSuchKey, NotFound},
};
use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};
use serde_json::json;
use std::fs;

mod contract;

const FAKE_TERRAFORM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/terraform");

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// A project with the three stack directories, driven by the fake Terraform.
fn project(
    approve: bool,
) -> std::result::Result<(tempfile::TempDir, Ctx), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    for stack in Stack::ALL {
        fs::create_dir_all(temp.path().join("infra").join(stack.name()))?;
    }
    let mut ctx = Ctx::sample(temp.path(), approve)?;
    ctx.terraform = PathBuf::from(FAKE_TERRAFORM);
    Ok((temp, ctx))
}

/// The state bucket's name follows from the sample settings.
const BUCKET: &str = "portal-123456789012-us-east-1-tfstate";
const LOCAL_STATE: &str = r#"{"resources":[{"type":"aws_s3_bucket","instances":[
    {"attributes":{"arn":"arn:aws:s3:::portal-123456789012-us-east-1-tfstate"}}]}]}"#;

/// Makes the account look bootstrapped, or not, to the next `bootstrap`.
fn account(ctx: &mut Ctx, bootstrapped: bool) {
    let rule =
        mock!(Client::head_bucket).match_requests(|request| request.bucket() == Some(BUCKET));
    let rule = if bootstrapped {
        rule.then_output(|| HeadBucketOutput::builder().build())
    } else {
        rule.then_error(|| HeadBucketError::NotFound(NotFound::builder().build()))
    };
    ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&rule]);
}

fn remote_init(stack_and_mode: &str, key: &str) -> String {
    format!(
        "{stack_and_mode} -backend-config=bucket={BUCKET} -backend-config=key={key} \
         -backend-config=region=us-east-1 -backend-config=use_lockfile=true"
    )
}

fn calls(ctx: &Ctx) -> Vec<String> {
    let log = fs::read_to_string(ctx.project_dir.join("calls.log")).unwrap_or_default();
    log.lines().map(str::to_owned).collect()
}

const PLAN_AND_APPLY: [&str; 2] = [
    "bootstrap: plan -input=false -out backstage-aws.tfplan",
    "bootstrap: apply -input=false backstage-aws.tfplan",
];

#[tokio::test]
async fn only_the_pinned_terraform_version_is_accepted() -> TestResult {
    let dir = tempfile::tempdir()?;
    for (version, accepted) in [("1.17.2", true), ("1.17.3", false), ("1.9.0", false)] {
        let link = dir.path().join(format!("terraform-{version}"));
        std::os::unix::fs::symlink(FAKE_TERRAFORM, &link)?;
        assert_eq!(verify_version(&link).await.is_ok(), accepted, "{version}");
    }
    let absent = verify_version(&dir.path().join("absent")).await;
    assert_eq!(absent.map_err(|error| error.code()), Err("TOOL_FAILED"));
    Ok(())
}

#[tokio::test]
async fn nothing_runs_without_approval() -> TestResult {
    let (_temp, ctx) = project(false)?;
    for refused in [
        apply(&ctx, Stack::Env, &json!({})).await,
        destroy(&ctx, Stack::Env, &json!({})).await,
        bootstrap(&ctx).await.map(drop),
    ] {
        assert_eq!(
            refused.map_err(|error| error.code()),
            Err("APPROVAL_REQUIRED")
        );
    }
    assert_eq!(calls(&ctx), Vec::<String>::new());
    assert!(!Stack::Env.dir(&ctx).join("terraform.tfvars.json").exists());
    Ok(())
}

#[tokio::test]
async fn apply_runs_exactly_the_plan_it_wrote_against_the_namespace_state() -> TestResult {
    let (_temp, ctx) = project(true)?;
    apply(&ctx, Stack::Env, &json!({"namespace": "pr-38"})).await?;
    let init = remote_init(
        "env: init -input=false -lockfile=readonly -reconfigure",
        "env/pr-38.tfstate",
    );
    assert_eq!(
        calls(&ctx),
        [
            init.as_str(),
            "env: plan -input=false -out backstage-aws.tfplan",
            "env: apply -input=false backstage-aws.tfplan",
        ]
    );
    let vars = fs::read(Stack::Env.dir(&ctx).join("terraform.tfvars.json"))?;
    assert_eq!(
        serde_json::from_slice::<Value>(&vars)?,
        json!({"namespace": "pr-38"})
    );
    Ok(())
}

#[tokio::test]
async fn a_failed_destroy_plan_is_never_applied() -> TestResult {
    let (_temp, ctx) = project(true)?;
    fs::write(ctx.project_dir.join("fail-plan"), "")?;
    let failed = destroy(&ctx, Stack::Env, &json!({})).await;
    assert_eq!(failed.map_err(|error| error.code()), Err("TOOL_FAILED"));
    let calls = calls(&ctx);
    assert_eq!(
        calls.last().map(String::as_str),
        Some("env: plan -input=false -out backstage-aws.tfplan -destroy")
    );
    assert!(!calls.iter().any(|call| call.starts_with("env: apply")));
    Ok(())
}

#[tokio::test]
async fn an_account_without_a_state_bucket_is_bootstrapped_from_state_on_disk() -> TestResult {
    let (_temp, mut ctx) = project(true)?;
    account(&mut ctx, false);
    // A backend file left by another account must not send this state there.
    let backend_file = Stack::Bootstrap.dir(&ctx).join("backend.local.tf");
    fs::write(&backend_file, "terraform {}")?;
    bootstrap(&ctx).await?;
    let remote = remote_init(
        "bootstrap: init -input=false -lockfile=readonly -reconfigure",
        "bootstrap/terraform.tfstate",
    );
    let [plan, apply] = PLAN_AND_APPLY;
    let local = "bootstrap: init -input=false -lockfile=readonly -reconfigure";
    assert_eq!(calls(&ctx), [local, plan, apply, remote.as_str()]);
    assert!(fs::read_to_string(backend_file)?.contains("backend \"s3\""));
    Ok(())
}

#[tokio::test]
async fn state_left_on_disk_is_moved_into_the_bucket_once_before_applying() -> TestResult {
    let (_temp, mut ctx) = project(true)?;
    account(&mut ctx, true);
    let local_state = Stack::Bootstrap.dir(&ctx).join("terraform.tfstate");
    fs::write(&local_state, LOCAL_STATE)?;
    assert!(bootstrap(&ctx).await?);
    let migrate = remote_init(
        "bootstrap: init -input=false -lockfile=readonly -migrate-state -force-copy",
        "bootstrap/terraform.tfstate",
    );
    let [plan, apply] = PLAN_AND_APPLY;
    assert_eq!(calls(&ctx), [migrate.as_str(), plan, apply]);
    assert!(!local_state.exists());

    account(&mut ctx, true);
    assert!(!bootstrap(&ctx).await?);
    let migrations = calls(&ctx)
        .iter()
        .filter(|call| call.contains("-migrate-state"))
        .count();
    assert_eq!(migrations, 1);
    Ok(())
}

#[tokio::test]
async fn state_on_disk_for_another_account_is_never_applied_or_moved() -> TestResult {
    let (_temp, mut ctx) = project(true)?;
    let foreign = LOCAL_STATE.replace("123456789012", "999999999999");
    fs::write(
        Stack::Bootstrap.dir(&ctx).join("terraform.tfstate"),
        foreign,
    )?;
    for bootstrapped in [false, true] {
        account(&mut ctx, bootstrapped);
        let refused = bootstrap(&ctx).await;
        assert_eq!(
            refused.map_err(|error| error.code()),
            Err("PRECONDITION_FAILED")
        );
    }
    assert_eq!(calls(&ctx), Vec::<String>::new());
    Ok(())
}

fn state_rule(key: &'static str, body: &'static str) -> Rule {
    mock!(Client::get_object)
        .match_requests(move |request| {
            request.bucket() == Some(BUCKET) && request.key() == Some(key)
        })
        .then_output(move || {
            GetObjectOutput::builder()
                .body(ByteStream::from_static(body.as_bytes()))
                .build()
        })
}

#[tokio::test]
async fn outputs_come_from_the_state_object_of_the_stack_and_namespace() -> TestResult {
    let (_temp, mut ctx) = project(false)?;
    let state = r#"{"version":4,"outputs":{"url":{"value":"https://pr-38.example.com","type":"string"},"desired_count":{"value":1}}}"#;
    let rule = state_rule("env/pr-38.tfstate", state);
    ctx.s3 = mock_client!(aws_sdk_s3, [&rule]);
    assert_eq!(
        outputs(&ctx, Stack::Env).await?,
        Some(json!({"url": "https://pr-38.example.com", "desired_count": 1}))
    );
    let rule = state_rule("base/terraform.tfstate", r#"{"outputs":{}}"#);
    ctx.s3 = mock_client!(aws_sdk_s3, [&rule]);
    assert_eq!(outputs(&ctx, Stack::Base).await?, Some(json!({})));
    Ok(())
}

#[tokio::test]
async fn only_a_missing_state_object_means_the_stack_was_never_applied() -> TestResult {
    let (_temp, mut ctx) = project(false)?;
    let missing = mock!(Client::get_object)
        .then_error(|| GetObjectError::NoSuchKey(NoSuchKey::builder().build()));
    ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&missing]);
    assert_eq!(outputs(&ctx, Stack::Env).await?, None);
    let required = required_outputs(&ctx, Stack::Base).await;
    assert_eq!(
        required.map_err(|error| error.code()),
        Err("PRECONDITION_FAILED")
    );

    let denied = mock!(Client::get_object).then_error(|| GetObjectError::unhandled("AccessDenied"));
    ctx.s3 = mock_client!(aws_sdk_s3, [&denied]);
    let unreadable = outputs(&ctx, Stack::Env).await;
    assert_eq!(unreadable.map_err(|error| error.code()), Err("AWS_FAILED"));

    let corrupt = state_rule("env/pr-38.tfstate", "not json");
    ctx.s3 = mock_client!(aws_sdk_s3, [&corrupt]);
    let unparsed = outputs(&ctx, Stack::Env).await;
    assert_eq!(unparsed.map_err(|error| error.code()), Err("STATE_FAILED"));
    Ok(())
}

#[test]
fn output_str_names_the_missing_output() {
    let outputs = json!({"ecs_cluster_name": "portal-cluster"});
    assert_eq!(
        output_str(&outputs, "ecs_cluster_name").ok(),
        Some("portal-cluster")
    );
    let missing = output_str(&outputs, "vpc_id").map_err(|error| error.to_string());
    assert_eq!(
        missing,
        Err("Terraform output `vpc_id` is missing".to_owned())
    );
}
