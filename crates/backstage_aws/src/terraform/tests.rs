use super::*;
use aws_sdk_s3::{
    Client,
    operation::{
        create_bucket::CreateBucketOutput,
        get_bucket_encryption::GetBucketEncryptionOutput,
        get_bucket_policy::GetBucketPolicyOutput,
        get_bucket_tagging::GetBucketTaggingOutput,
        get_object::{GetObjectError, GetObjectOutput},
        head_bucket::{HeadBucketError, HeadBucketOutput},
        put_bucket_encryption::PutBucketEncryptionOutput,
        put_bucket_ownership_controls::PutBucketOwnershipControlsOutput,
        put_bucket_policy::PutBucketPolicyOutput,
        put_bucket_tagging::PutBucketTaggingOutput,
        put_bucket_versioning::PutBucketVersioningOutput,
        put_public_access_block::PutPublicAccessBlockOutput,
    },
    primitives::ByteStream,
    types::{
        ServerSideEncryption, ServerSideEncryptionByDefault, ServerSideEncryptionConfiguration,
        ServerSideEncryptionRule,
        error::{NoSuchKey, NotFound},
    },
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
/// Terraform's bootstrap state object key in the state bucket.
const BOOTSTRAP_KEY: &str = "bootstrap/terraform.tfstate";
const LOCAL_STATE: &str = r#"{"version":4,"lineage":"11111111-2222-3333-4444-555555555555","serial":1,"outputs":{"state_kms_key_arn":{"value":"arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555","type":"string"}},"resources":[
  {"mode":"managed","type":"aws_s3_bucket","name":"state","instances":[
    {"attributes":{"bucket":"portal-123456789012-us-east-1-tfstate"}}]},
  {"mode":"data","type":"aws_caller_identity","name":"current","instances":[
    {"attributes":{"account_id":"123456789012"}}]}]}"#;

/// Makes the account look bootstrapped, or not, to the next `bootstrap`.
/// Bucket reconciliation succeeds either way; only the create path differs.
/// Migrations land in `bootstrap-remote-state`, which serves the bootstrap object,
/// and `bootstrap-encrypted` appears once the bucket's default encryption is set.
fn account(ctx: &mut Ctx, bootstrapped: bool) -> TestResult {
    let head =
        mock!(Client::head_bucket).match_requests(|request| request.bucket() == Some(BUCKET));
    let head = if bootstrapped {
        head.then_output(|| HeadBucketOutput::builder().build())
    } else {
        head.then_error(|| HeadBucketError::NotFound(NotFound::builder().build()))
    };
    let create = mock!(Client::create_bucket).then_output(|| CreateBucketOutput::builder().build());
    let block = mock!(Client::put_public_access_block)
        .then_output(|| PutPublicAccessBlockOutput::builder().build());
    let ownership = mock!(Client::put_bucket_ownership_controls)
        .then_output(|| PutBucketOwnershipControlsOutput::builder().build());
    let versioning = mock!(Client::put_bucket_versioning)
        .then_output(|| PutBucketVersioningOutput::builder().build());
    let policy_read =
        mock!(Client::get_bucket_policy).then_output(|| GetBucketPolicyOutput::builder().build());
    let policy_write =
        mock!(Client::put_bucket_policy).then_output(|| PutBucketPolicyOutput::builder().build());
    let untagged = GetBucketTaggingOutput::builder()
        .set_tag_set(Some(Vec::new()))
        .build()?;
    let tags_read = mock!(Client::get_bucket_tagging).then_output(move || untagged.clone());
    let tags_write =
        mock!(Client::put_bucket_tagging).then_output(|| PutBucketTaggingOutput::builder().build());
    // A new bucket has S3's default SSE-S3 encryption, which `encrypt` may replace.
    let default = ServerSideEncryptionByDefault::builder()
        .sse_algorithm(ServerSideEncryption::Aes256)
        .build()?;
    let rule = ServerSideEncryptionRule::builder()
        .apply_server_side_encryption_by_default(default)
        .build();
    let encryption = GetBucketEncryptionOutput::builder()
        .server_side_encryption_configuration(
            ServerSideEncryptionConfiguration::builder()
                .rules(rule)
                .build()?,
        )
        .build();
    let encryption_read =
        mock!(Client::get_bucket_encryption).then_output(move || encryption.clone());
    let marker = ctx.project_dir.join("bootstrap-encrypted");
    let encryption_write = mock!(Client::put_bucket_encryption).then_output(move || {
        if let Err(error) = fs::write(&marker, "") {
            eprintln!("cannot mark bucket encrypted: {error}");
        }
        PutBucketEncryptionOutput::builder().build()
    });
    let remote = ctx.project_dir.join("bootstrap-remote-state");
    let (present, absent) = (remote.clone(), remote.clone());
    let remote_present = mock!(Client::get_object)
        .match_requests(move |request| request.key() == Some(BOOTSTRAP_KEY) && present.is_file())
        .then_output(move || {
            GetObjectOutput::builder()
                .body(ByteStream::from(stored_state(&remote)))
                .build()
        });
    let remote_missing = mock!(Client::get_object)
        .match_requests(move |request| request.key() == Some(BOOTSTRAP_KEY) && !absent.is_file())
        .then_error(|| GetObjectError::NoSuchKey(NoSuchKey::builder().build()));
    ctx.s3 = mock_client!(
        aws_sdk_s3,
        RuleMode::MatchAny,
        [
            &head,
            &create,
            &block,
            &ownership,
            &versioning,
            &policy_read,
            &policy_write,
            &tags_read,
            &tags_write,
            &encryption_read,
            &encryption_write,
            &remote_present,
            &remote_missing,
        ]
    );
    Ok(())
}

/// The migrated state bytes. A missing copy becomes empty bytes, which fail the
/// state parse downstream, so the error surfaces from `bootstrap`.
fn stored_state(path: &Path) -> Vec<u8> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    bytes
}

fn remote_init(stack_and_mode: &str, key: &str) -> String {
    format!(
        "{stack_and_mode} -backend-config=bucket={BUCKET} -backend-config=key={key} \
         -backend-config=region=us-east-1 -backend-config=use_lockfile=true"
    )
}

/// The first `bootstrap` init: Terraform reads the stack's local state file.
fn local_init(ctx: &Ctx) -> String {
    let state = Stack::Bootstrap.dir(ctx).join("terraform.tfstate");
    format!(
        "bootstrap: init -input=false -lockfile=readonly -reconfigure -backend-config=path={}",
        state.display()
    )
}

fn calls(ctx: &Ctx) -> std::io::Result<Vec<String>> {
    let log = match fs::read_to_string(ctx.project_dir.join("calls.log")) {
        Ok(log) => log,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    Ok(log.lines().map(str::to_owned).collect())
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
    assert_eq!(calls(&ctx)?, Vec::<String>::new());
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
        calls(&ctx)?,
        [
            init.as_str(),
            "env: plan -input=false -out backstage-aws.tfplan",
            "env: apply -input=false backstage-aws.tfplan",
        ]
    );
    let planned = fs::read_to_string(ctx.project_dir.join("env-planned.tfvars"))?;
    assert_eq!(planned, "namespace = \"pr-38\"\n");
    Ok(())
}

#[tokio::test]
async fn a_failed_destroy_plan_is_never_applied() -> TestResult {
    let (_temp, ctx) = project(true)?;
    fs::write(ctx.project_dir.join("fail-plan"), "")?;
    let failed = destroy(&ctx, Stack::Env, &json!({})).await;
    assert_eq!(failed.map_err(|error| error.code()), Err("TOOL_FAILED"));
    let calls = calls(&ctx)?;
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
    account(&mut ctx, false)?;
    // The first apply writes this state; the fake Terraform copies it from here.
    fs::write(ctx.project_dir.join("bootstrap-state"), LOCAL_STATE)?;
    // A backend file left by another account must not send this state there.
    let backend_file = Stack::Bootstrap.dir(&ctx).join("backend.local.tf");
    fs::write(&backend_file, "terraform {}")?;
    assert!(bootstrap(&ctx).await?);
    let local = local_init(&ctx);
    let migrate = remote_init(
        "bootstrap: init -input=false -lockfile=readonly -migrate-state -force-copy",
        BOOTSTRAP_KEY,
    );
    let [plan, apply] = PLAN_AND_APPLY;
    assert_eq!(
        calls(&ctx)?,
        [local.as_str(), plan, apply, migrate.as_str()]
    );
    assert_eq!(fs::read_to_string(backend_file)?, "terraform {}");
    Ok(())
}

#[tokio::test]
async fn state_left_on_disk_is_moved_into_the_bucket_once_before_applying() -> TestResult {
    let (_temp, mut ctx) = project(true)?;
    account(&mut ctx, true)?;
    let local_state = Stack::Bootstrap.dir(&ctx).join("terraform.tfstate");
    fs::write(&local_state, LOCAL_STATE)?;
    assert!(!bootstrap(&ctx).await?);
    let local = local_init(&ctx);
    let migrate = remote_init(
        "bootstrap: init -input=false -lockfile=readonly -migrate-state -force-copy",
        BOOTSTRAP_KEY,
    );
    let [plan, apply] = PLAN_AND_APPLY;
    assert_eq!(
        calls(&ctx)?,
        [local.as_str(), migrate.as_str(), plan, apply]
    );
    assert!(!local_state.exists());

    account(&mut ctx, true)?;
    assert!(!bootstrap(&ctx).await?);
    let migrations = calls(&ctx)?
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
        account(&mut ctx, bootstrapped)?;
        let refused = bootstrap(&ctx).await;
        assert_eq!(
            refused.map_err(|error| error.code()),
            Err("PRECONDITION_FAILED")
        );
    }
    assert_eq!(calls(&ctx)?, Vec::<String>::new());
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

/// List all temporary Terraform workspace directories in the project root.
fn temp_workspaces(project_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut workspaces = Vec::new();
    for entry in fs::read_dir(project_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with(".terraform-workspace-")
        {
            workspaces.push(entry.path());
        }
    }
    Ok(workspaces)
}

#[tokio::test]
async fn env_stack_failed_apply_retains_workspace() -> TestResult {
    let (_temp, ctx) = project(true)?;
    fs::write(ctx.project_dir.join("fail-apply"), "")?;
    let result = apply(&ctx, Stack::Env, &json!({"env": "test"})).await;
    assert_eq!(result.map_err(|error| error.code()), Err("TOOL_FAILED"));
    let workspaces = temp_workspaces(&ctx.project_dir)?;
    assert_eq!(workspaces.len(), 1);
    let [workspace] = workspaces.as_slice() else {
        return Err("expected exactly one retained workspace".into());
    };
    // Verify errored.tfstate was written with correct content.
    let errored = workspace.join("env").join("errored.tfstate");
    assert!(
        errored.exists(),
        "errored.tfstate should exist in retained workspace"
    );
    let content = fs::read_to_string(&errored)?;
    assert!(
        content.contains("\"errored\":true"),
        "errored.tfstate should indicate failure"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let state_mode = fs::metadata(&errored)?.permissions().mode() & 0o777;
        assert_eq!(state_mode, 0o600, "errored.tfstate should be 0600");
        let dir_mode = fs::metadata(workspace)?.permissions().mode() & 0o777;
        assert_eq!(dir_mode & 0o077, 0, "workspace dir should be private");
    }
    Ok(())
}

#[tokio::test]
async fn base_stack_failed_apply_retains_workspace() -> TestResult {
    let (_temp, ctx) = project(true)?;
    fs::write(ctx.project_dir.join("fail-apply"), "")?;
    let result = apply(&ctx, Stack::Base, &json!({"env": "test"})).await;
    assert_eq!(result.map_err(|error| error.code()), Err("TOOL_FAILED"));
    let workspaces = temp_workspaces(&ctx.project_dir)?;
    assert_eq!(workspaces.len(), 1);
    let [workspace] = workspaces.as_slice() else {
        return Err("expected exactly one retained workspace".into());
    };
    let errored = workspace.join("base").join("errored.tfstate");
    assert!(
        errored.exists(),
        "errored.tfstate should exist in retained workspace"
    );
    let content = fs::read_to_string(&errored)?;
    assert!(content.contains("\"errored\":true"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let state_mode = fs::metadata(&errored)?.permissions().mode() & 0o777;
        assert_eq!(state_mode, 0o600);
    }
    Ok(())
}

#[tokio::test]
async fn bootstrap_remote_failed_apply_retains_workspace() -> TestResult {
    let (_temp, mut ctx) = project(true)?;
    // Bucket exists; migrate state from disk then fail during apply.
    account(&mut ctx, true)?;
    let local_state = Stack::Bootstrap.dir(&ctx).join("terraform.tfstate");
    fs::write(&local_state, LOCAL_STATE)?;
    fs::write(ctx.project_dir.join("fail-apply"), "")?;
    let result = bootstrap(&ctx).await;
    assert_eq!(result.map_err(|error| error.code()), Err("TOOL_FAILED"));
    let workspaces = temp_workspaces(&ctx.project_dir)?;
    assert_eq!(workspaces.len(), 1);
    let [workspace] = workspaces.as_slice() else {
        return Err("expected exactly one retained workspace".into());
    };
    let errored = workspace.join("bootstrap").join("errored.tfstate");
    assert!(
        errored.exists(),
        "errored.tfstate should exist in retained workspace"
    );
    let content = fs::read_to_string(&errored)?;
    assert!(content.contains("\"errored\":true"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let state_mode = fs::metadata(&errored)?.permissions().mode() & 0o777;
        assert_eq!(state_mode, 0o600);
    }
    Ok(())
}

#[tokio::test]
async fn successful_apply_cleans_up_workspace() -> TestResult {
    let (_temp, ctx) = project(true)?;
    let result = apply(&ctx, Stack::Env, &json!({"env": "test"})).await;
    assert!(result.is_ok());
    let workspaces = temp_workspaces(&ctx.project_dir)?;
    assert_eq!(workspaces.len(), 0);
    Ok(())
}

#[tokio::test]
async fn successful_bootstrap_cleans_up_workspace() -> TestResult {
    let (_temp, mut ctx) = project(true)?;
    account(&mut ctx, false)?;
    fs::write(ctx.project_dir.join("bootstrap-state"), LOCAL_STATE)?;
    let result = bootstrap(&ctx).await;
    assert!(result.is_ok());
    let workspaces = temp_workspaces(&ctx.project_dir)?;
    assert_eq!(workspaces.len(), 0);
    Ok(())
}
