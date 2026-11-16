//! The Terraform state bucket's lifecycle, kept outside Terraform so a failed
//! or interrupted first run recovers by simply running again.
//!
//! `ensure` creates the bucket when it is missing and reconciles its privacy,
//! versioning, ownership, tags and transport policy. It never touches
//! encryption: a new empty bucket has S3's default SSE-S3 and holds no state.
//! `encrypt` then sets the Terraform-managed KMS key as the default before any
//! state is uploaded. Every step is idempotent, and nothing here reads, writes
//! or deletes objects or calls KMS.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
};
use aws_sdk_s3::{
    error::ProvideErrorMetadata,
    operation::{create_bucket::CreateBucketError, head_bucket::HeadBucketError},
    types::{
        BucketLocationConstraint, BucketVersioningStatus, CreateBucketConfiguration,
        ObjectOwnership, OwnershipControls, OwnershipControlsRule, PublicAccessBlockConfiguration,
        ServerSideEncryption, ServerSideEncryptionByDefault, ServerSideEncryptionConfiguration,
        ServerSideEncryptionRule, Tag, Tagging, VersioningConfiguration,
    },
};
use aws_smithy_types::error::display::DisplayErrorContext;
use serde_json::{Value, json};

#[cfg(test)]
#[path = "bucket_tests.rs"]
mod tests;

const TLS_SID: &str = "DenyInsecureTransport";
const MANAGED_BY: &str = "Terraform";
const PURPOSE: &str = "TerraformState";

/// The bucket a command targets and the account that must own it.
struct Target {
    name: String,
    owner: String,
}

impl Target {
    fn of(ctx: &Ctx) -> Self {
        Self {
            name: ctx.settings.state_bucket(),
            owner: ctx.settings.settings.aws_account_id.clone(),
        }
    }
}

fn failed(operation: &'static str, error: impl std::error::Error) -> Error {
    Error::aws_failed("s3", operation, DisplayErrorContext(&error).to_string())
}

/// Creates the state bucket if missing and reconciles its hardening.
pub(super) async fn ensure(ctx: &Ctx) -> Result<()> {
    ctx.require_approval()?;
    let target = Target::of(ctx);
    if !exists(ctx, &target).await? {
        create(ctx, &target).await?;
    }
    reconcile(ctx, &target).await
}

/// Makes `key_arn` the bucket's default encryption, with bucket keys.
pub(super) async fn encrypt(ctx: &Ctx, key_arn: &str) -> Result<()> {
    ctx.require_approval()?;
    if !key_arn.starts_with("arn:") || !key_arn.contains(":kms:") || key_arn.contains("alias/") {
        return Err(Error::invalid_input(format!(
            "the state bucket needs a KMS key ARN, not {key_arn:?}"
        )));
    }
    let target = Target::of(ctx);
    check_encryption(ctx, &target, key_arn).await?;
    let default = ServerSideEncryptionByDefault::builder()
        .sse_algorithm(ServerSideEncryption::AwsKms)
        .kms_master_key_id(key_arn)
        .build()
        .map_err(|error| failed("PutBucketEncryption", error))?;
    let rule = ServerSideEncryptionRule::builder()
        .apply_server_side_encryption_by_default(default)
        .bucket_key_enabled(true)
        .build();
    let configuration = ServerSideEncryptionConfiguration::builder()
        .rules(rule)
        .build()
        .map_err(|error| failed("PutBucketEncryption", error))?;
    ctx.s3
        .put_bucket_encryption()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .server_side_encryption_configuration(configuration)
        .send()
        .await
        .map_err(|error| failed("PutBucketEncryption", error))?;
    Ok(())
}

/// Never replace an unrelated KMS key or downgrade a stronger algorithm.
async fn check_encryption(ctx: &Ctx, target: &Target, key_arn: &str) -> Result<()> {
    let existing = ctx
        .s3
        .get_bucket_encryption()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .send()
        .await
        .map_err(|error| failed("GetBucketEncryption", error))?;
    let Some([rule]) = existing
        .server_side_encryption_configuration()
        .map(ServerSideEncryptionConfiguration::rules)
    else {
        return Err(Error::precondition_failed(
            "ambiguous state bucket encryption; unchanged",
        ));
    };
    let defaults = rule
        .apply_server_side_encryption_by_default()
        .ok_or_else(|| {
            Error::precondition_failed("missing state bucket encryption defaults; unchanged")
        })?;
    if defaults.sse_algorithm() == &ServerSideEncryption::Aes256
        || (defaults.sse_algorithm() == &ServerSideEncryption::AwsKms
            && defaults.kms_master_key_id() == Some(key_arn))
    {
        Ok(())
    } else {
        Err(Error::precondition_failed(
            "state bucket already uses a different encryption key or algorithm; unchanged",
        ))
    }
}

/// Whether the bucket exists and belongs to this account. Anything but a
/// plain "not found" (denied, owned by someone else, throttled) stops the run
/// before it writes.
async fn exists(ctx: &Ctx, target: &Target) -> Result<bool> {
    let head = ctx
        .s3
        .head_bucket()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .send()
        .await;
    match head {
        Ok(_) => Ok(true),
        Err(error)
            if error
                .as_service_error()
                .is_some_and(HeadBucketError::is_not_found) =>
        {
            Ok(false)
        }
        Err(error) => Err(failed("HeadBucket", error)),
    }
}

/// Creates the bucket with ACLs disabled. A bucket this account already owns
/// is the one safe collision: an earlier run got this far.
async fn create(ctx: &Ctx, target: &Target) -> Result<()> {
    let region = &ctx.settings.settings.aws_region;
    let mut request = ctx
        .s3
        .create_bucket()
        .bucket(&target.name)
        .object_ownership(ObjectOwnership::BucketOwnerEnforced);
    if region != "us-east-1" {
        let location = CreateBucketConfiguration::builder()
            .location_constraint(BucketLocationConstraint::from(region.as_str()))
            .build();
        request = request.create_bucket_configuration(location);
    }
    match request.send().await {
        Ok(_) => Ok(()),
        Err(error)
            if error
                .as_service_error()
                .is_some_and(CreateBucketError::is_bucket_already_owned_by_you) =>
        {
            Ok(())
        }
        Err(error) => Err(failed("CreateBucket", error)),
    }
}

async fn reconcile(ctx: &Ctx, target: &Target) -> Result<()> {
    block_public_access(ctx, target).await?;
    enforce_ownership(ctx, target).await?;
    enable_versioning(ctx, target).await?;
    reconcile_tags(ctx, target).await?;
    reconcile_policy(ctx, target).await
}

async fn block_public_access(ctx: &Ctx, target: &Target) -> Result<()> {
    let block = PublicAccessBlockConfiguration::builder()
        .block_public_acls(true)
        .ignore_public_acls(true)
        .block_public_policy(true)
        .restrict_public_buckets(true)
        .build();
    ctx.s3
        .put_public_access_block()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .public_access_block_configuration(block)
        .send()
        .await
        .map_err(|error| failed("PutPublicAccessBlock", error))?;
    Ok(())
}

async fn enforce_ownership(ctx: &Ctx, target: &Target) -> Result<()> {
    let rule = OwnershipControlsRule::builder()
        .object_ownership(ObjectOwnership::BucketOwnerEnforced)
        .build()
        .map_err(|error| failed("PutBucketOwnershipControls", error))?;
    let controls = OwnershipControls::builder()
        .rules(rule)
        .build()
        .map_err(|error| failed("PutBucketOwnershipControls", error))?;
    ctx.s3
        .put_bucket_ownership_controls()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .ownership_controls(controls)
        .send()
        .await
        .map_err(|error| failed("PutBucketOwnershipControls", error))?;
    Ok(())
}

async fn enable_versioning(ctx: &Ctx, target: &Target) -> Result<()> {
    let versioning = VersioningConfiguration::builder()
        .status(BucketVersioningStatus::Enabled)
        .build();
    ctx.s3
        .put_bucket_versioning()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .versioning_configuration(versioning)
        .send()
        .await
        .map_err(|error| failed("PutBucketVersioning", error))?;
    Ok(())
}

/// The tags Terraform's provider defaults put on the bucket it used to manage.
fn expected_tags(ctx: &Ctx) -> [(&'static str, String); 3] {
    [
        ("Project", ctx.settings.settings.name.clone()),
        ("ManagedBy", MANAGED_BY.to_owned()),
        ("Purpose", PURPOSE.to_owned()),
    ]
}

/// Adds missing expected tags and keeps every other tag. A different value on
/// an expected key means the bucket may belong to another project: stop.
async fn reconcile_tags(ctx: &Ctx, target: &Target) -> Result<()> {
    let found = ctx
        .s3
        .get_bucket_tagging()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .send()
        .await;
    let mut tags: Vec<(String, String)> = match found {
        Ok(output) => output
            .tag_set()
            .iter()
            .map(|tag| (tag.key().to_owned(), tag.value().to_owned()))
            .collect(),
        Err(error) if error.code() == Some("NoSuchTagSet") => Vec::new(),
        Err(error) => return Err(failed("GetBucketTagging", error)),
    };
    let mut changed = false;
    for (key, value) in expected_tags(ctx) {
        match tags.iter().find(|(existing, _)| existing == key) {
            Some((_, actual)) if *actual == value => {}
            Some((_, actual)) => {
                return Err(Error::precondition_failed(format!(
                    "bucket {} has tag {key}={actual:?}, expected {value:?}; refusing to adopt it",
                    target.name
                )));
            }
            None => {
                tags.push((key.to_owned(), value));
                changed = true;
            }
        }
    }
    if changed {
        put_tags(ctx, target, &tags).await?;
    }
    Ok(())
}

async fn put_tags(ctx: &Ctx, target: &Target, tags: &[(String, String)]) -> Result<()> {
    let mut tagging = Tagging::builder();
    for (key, value) in tags {
        let tag = Tag::builder()
            .key(key)
            .value(value)
            .build()
            .map_err(|error| failed("PutBucketTagging", error))?;
        tagging = tagging.tag_set(tag);
    }
    let tagging = tagging
        .build()
        .map_err(|error| failed("PutBucketTagging", error))?;
    ctx.s3
        .put_bucket_tagging()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .tagging(tagging)
        .send()
        .await
        .map_err(|error| failed("PutBucketTagging", error))?;
    Ok(())
}

fn partition(region: &str) -> &'static str {
    if region.starts_with("cn-") {
        "aws-cn"
    } else if region.starts_with("us-gov-") {
        "aws-us-gov"
    } else {
        "aws"
    }
}

fn tls_statement(ctx: &Ctx, bucket: &str) -> Value {
    let arn = format!(
        "arn:{}:s3:::{bucket}",
        partition(&ctx.settings.settings.aws_region)
    );
    json!({
        "Sid": TLS_SID,
        "Effect": "Deny",
        "Principal": "*",
        "Action": "s3:*",
        "Resource": [arn, format!("{arn}/*")],
        "Condition": {"Bool": {"aws:SecureTransport": "false"}}
    })
}

/// Adds the TLS-only statement to whatever policy exists. Other statements
/// stay byte-for-byte; a policy this code cannot read unambiguously, or a
/// statement with the same Sid but other content, is left alone and reported.
fn merged_policy(existing: Option<&str>, statement: Value, bucket: &str) -> Result<Option<Value>> {
    let ambiguous = |why: &str| {
        Error::precondition_failed(format!(
            "bucket {bucket} has a policy this tool cannot safely merge ({why}); \
             add the {TLS_SID} statement by hand"
        ))
    };
    let Some(text) = existing else {
        return Ok(Some(
            json!({"Version": "2012-10-17", "Statement": [statement]}),
        ));
    };
    let mut policy: Value =
        serde_json::from_str(text).map_err(|_| ambiguous("it is not valid JSON"))?;
    let object = policy
        .as_object_mut()
        .ok_or_else(|| ambiguous("it is not an object"))?;
    let Some(Value::Array(statements)) = object.get_mut("Statement") else {
        return Err(ambiguous("Statement is not a list"));
    };
    match statements
        .iter()
        .find(|s| s.get("Sid") == Some(&json!(TLS_SID)))
    {
        Some(found) if *found == statement => Ok(None),
        Some(_) => Err(ambiguous(
            "its transport statement differs from the expected one",
        )),
        None => {
            statements.push(statement);
            Ok(Some(policy))
        }
    }
}

async fn reconcile_policy(ctx: &Ctx, target: &Target) -> Result<()> {
    let found = ctx
        .s3
        .get_bucket_policy()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .send()
        .await;
    let existing = match found {
        Ok(output) => output.policy().map(str::to_owned),
        Err(error) if error.code() == Some("NoSuchBucketPolicy") => None,
        Err(error) => return Err(failed("GetBucketPolicy", error)),
    };
    let statement = tls_statement(ctx, &target.name);
    let Some(policy) = merged_policy(existing.as_deref(), statement, &target.name)? else {
        return Ok(());
    };
    ctx.s3
        .put_bucket_policy()
        .bucket(&target.name)
        .expected_bucket_owner(&target.owner)
        .policy(policy.to_string())
        .send()
        .await
        .map_err(|error| failed("PutBucketPolicy", error))?;
    Ok(())
}
