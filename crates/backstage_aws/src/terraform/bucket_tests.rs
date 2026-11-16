//! The state bucket must be private, versioned and TLS-only before anything
//! depends on it, and a rerun after any partial failure must converge without
//! creating twice, adopting a foreign bucket or discarding unrelated config.
use super::*;
use aws_sdk_s3::{
    Client,
    operation::{
        create_bucket::{CreateBucketError, CreateBucketOutput},
        get_bucket_encryption::{GetBucketEncryptionError, GetBucketEncryptionOutput},
        get_bucket_policy::{GetBucketPolicyError, GetBucketPolicyOutput},
        get_bucket_tagging::{GetBucketTaggingError, GetBucketTaggingOutput},
        head_bucket::{HeadBucketError, HeadBucketOutput},
        put_bucket_encryption::PutBucketEncryptionOutput,
        put_bucket_ownership_controls::PutBucketOwnershipControlsOutput,
        put_bucket_policy::PutBucketPolicyOutput,
        put_bucket_tagging::PutBucketTaggingOutput,
        put_bucket_versioning::PutBucketVersioningOutput,
        put_public_access_block::PutPublicAccessBlockOutput,
    },
    types::error::{BucketAlreadyOwnedByYou, NotFound},
};
use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};
use aws_smithy_types::error::ErrorMetadata;
use std::sync::{Arc, Mutex};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type Seen = Arc<Mutex<Vec<String>>>;

const ACCOUNT: &str = "123456789012";
const KEY: &str = "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555";

#[derive(Clone, Copy)]
enum Head {
    Exists,
    Missing,
    Denied,
}

#[derive(Clone, Copy)]
enum Create {
    Created,
    OwnedByYou,
    Taken,
}

struct Given {
    region: &'static str,
    head: Head,
    create: Create,
    tags: Option<Vec<(&'static str, &'static str)>>,
    policy: Option<String>,
    encryption_algorithm: ServerSideEncryption,
    encryption_key: Option<&'static str>,
    encryption_read_denied: bool,
}

impl Given {
    fn new(head: Head) -> Self {
        Self {
            region: "us-east-1",
            head,
            create: Create::Created,
            tags: None,
            policy: None,
            encryption_algorithm: ServerSideEncryption::Aes256,
            encryption_key: None,
            encryption_read_denied: false,
        }
    }
}

fn only(seen: &Seen) -> String {
    match recorded(seen).as_slice() {
        [one] => one.clone(),
        _ => String::new(),
    }
}

struct Rig {
    _project: tempfile::TempDir,
    ctx: Ctx,
    bucket: String,
    head: Rule,
    create: Rule,
    access: Rule,
    ownership: Rule,
    versioning: Rule,
    tags_put: Rule,
    policy_put: Rule,
    encryption: Rule,
    created_with: Seen,
    tags_written: Seen,
    policy_written: Seen,
    encryption_written: Seen,
}

impl Rig {
    fn writes(&self) -> [usize; 6] {
        [
            &self.create,
            &self.access,
            &self.ownership,
            &self.versioning,
            &self.tags_put,
            &self.policy_put,
        ]
        .map(Rule::num_calls)
    }
}

fn remember(seen: &Seen, text: String) {
    if let Ok(mut all) = seen.lock() {
        all.push(text);
    }
}

fn recorded(seen: &Seen) -> Vec<String> {
    seen.lock().map_or_else(|_| Vec::new(), |all| all.clone())
}

fn code(code: &str) -> ErrorMetadata {
    ErrorMetadata::builder().code(code).build()
}

fn rig(given: &Given, approve: bool) -> std::result::Result<Rig, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut ctx = Ctx::sample(temp.path(), approve)?;
    ctx.settings.settings.aws_region = given.region.to_owned();
    let bucket = ctx.settings.state_bucket();
    let ours = {
        let bucket = bucket.clone();
        move |name: Option<&str>, owner: Option<&str>| {
            name == Some(bucket.as_str()) && owner == Some(ACCOUNT)
        }
    };
    let (created_with, tags_written, policy_written, encryption_written) = (
        Seen::default(),
        Seen::default(),
        Seen::default(),
        Seen::default(),
    );

    let [head, create, access, ownership, versioning] =
        creation_rules(given, &bucket, &created_with);

    let [tags_get, tags_put] = tagging_rules(given, &bucket, &tags_written)?;

    let policy_get = mock!(Client::get_bucket_policy).match_requests({
        let ours = ours.clone();
        move |r| ours(r.bucket(), r.expected_bucket_owner())
    });
    let policy_get = match given.policy.clone() {
        Some(text) => policy_get.then_output(move || {
            GetBucketPolicyOutput::builder()
                .policy(text.clone())
                .build()
        }),
        None => policy_get.then_error(|| GetBucketPolicyError::generic(code("NoSuchBucketPolicy"))),
    };
    let seen = policy_written.clone();
    let policy_put = mock!(Client::put_bucket_policy)
        .match_requests({
            let ours = ours.clone();
            move |r| {
                let policy = r
                    .policy()
                    .map_or_else(String::new, std::borrow::ToOwned::to_owned);
                remember(&seen, policy);
                ours(r.bucket(), r.expected_bucket_owner())
            }
        })
        .then_output(|| PutBucketPolicyOutput::builder().build());

    let [encryption_get, encryption] = encryption_rules(given, &bucket, &encryption_written)?;

    ctx.s3 = mock_client!(
        aws_sdk_s3,
        RuleMode::MatchAny,
        [
            &head,
            &create,
            &access,
            &ownership,
            &versioning,
            &tags_get,
            &tags_put,
            &policy_get,
            &policy_put,
            &encryption_get,
            &encryption
        ]
    );
    Ok(Rig {
        _project: temp,
        ctx,
        bucket,
        head,
        create,
        access,
        ownership,
        versioning,
        tags_put,
        policy_put,
        encryption,
        created_with,
        tags_written,
        policy_written,
        encryption_written,
    })
}

fn tls_policy(rig: &Rig) -> Value {
    let arn = format!("arn:aws:s3:::{}", rig.bucket);
    json!({
        "Sid": "DenyInsecureTransport", "Effect": "Deny", "Principal": "*",
        "Action": "s3:*", "Resource": [arn, format!("{arn}/*")],
        "Condition": {"Bool": {"aws:SecureTransport": "false"}}
    })
}

const FRESH_TAGS: &str = "ManagedBy=Terraform,Project=portal,Purpose=TerraformState";

#[tokio::test]
async fn nothing_happens_without_approval() -> TestResult {
    let rig = rig(&Given::new(Head::Missing), false)?;
    let ensured = ensure(&rig.ctx).await;
    let encrypted = encrypt(&rig.ctx, KEY).await;
    for refused in [ensured, encrypted] {
        assert_eq!(
            refused.map_err(|error| error.code()),
            Err("APPROVAL_REQUIRED")
        );
    }
    assert_eq!(rig.head.num_calls(), 0);
    assert_eq!(rig.writes(), [0; 6]);
    assert_eq!(rig.encryption.num_calls(), 0);
    Ok(())
}

#[tokio::test]
async fn a_missing_bucket_in_us_east_1_is_created_without_a_location_and_fully_hardened()
-> TestResult {
    let rig = rig(&Given::new(Head::Missing), true)?;
    ensure(&rig.ctx).await?;
    assert_eq!(recorded(&rig.created_with), ["None"]);
    // One create and every hardening write, each exactly once with exact payloads.
    assert_eq!(rig.writes(), [1; 6]);
    assert_eq!(recorded(&rig.tags_written), [FRESH_TAGS]);
    let policy: Value = serde_json::from_str(&only(&rig.policy_written))?;
    assert_eq!(
        policy,
        json!({"Version": "2012-10-17", "Statement": [tls_policy(&rig)]})
    );
    // Encryption is none of ensure's business.
    assert_eq!(rig.encryption.num_calls(), 0);
    Ok(())
}

#[tokio::test]
async fn a_regional_bucket_is_created_with_exactly_its_region_as_location() -> TestResult {
    let mut given = Given::new(Head::Missing);
    given.region = "eu-west-1";
    let rig = rig(&given, true)?;
    assert_eq!(rig.bucket, "portal-123456789012-eu-west-1-tfstate");
    ensure(&rig.ctx).await?;
    assert_eq!(recorded(&rig.created_with), [r#"Some("eu-west-1")"#]);
    assert_eq!(rig.writes(), [1; 6]);
    let policy = recorded(&rig.policy_written).join("");
    assert!(policy.contains("arn:aws:s3:::portal-123456789012-eu-west-1-tfstate/*"));
    Ok(())
}

#[tokio::test]
async fn a_denied_or_foreign_bucket_stops_before_any_write() -> TestResult {
    let rig = rig(&Given::new(Head::Denied), true)?;
    let error = ensure(&rig.ctx).await;
    assert_eq!(error.map_err(|e| e.code()), Err("AWS_FAILED"));
    assert_eq!(rig.head.num_calls(), 1);
    assert_eq!(rig.writes(), [0; 6]);
    Ok(())
}

#[tokio::test]
async fn a_name_someone_else_owns_is_never_hardened() -> TestResult {
    let mut given = Given::new(Head::Missing);
    given.create = Create::Taken;
    let rig = rig(&given, true)?;
    let error = ensure(&rig.ctx).await;
    assert_eq!(error.map_err(|e| e.code()), Err("AWS_FAILED"));
    assert_eq!(rig.writes(), [1, 0, 0, 0, 0, 0]);
    Ok(())
}

#[tokio::test]
async fn a_rerun_after_a_partial_run_reuses_the_bucket_and_finishes_the_rest() -> TestResult {
    let rig = rig(&Given::new(Head::Exists), true)?;
    ensure(&rig.ctx).await?;
    assert_eq!(rig.create.num_calls(), 0);
    assert_eq!(rig.writes(), [0, 1, 1, 1, 1, 1]);
    Ok(())
}

#[tokio::test]
async fn a_create_that_lost_a_race_with_our_own_earlier_run_continues() -> TestResult {
    let mut given = Given::new(Head::Missing);
    given.create = Create::OwnedByYou;
    let rig = rig(&given, true)?;
    ensure(&rig.ctx).await?;
    assert_eq!(rig.writes(), [1; 6]);
    Ok(())
}

#[tokio::test]
async fn unrelated_tags_survive_and_missing_ones_are_added() -> TestResult {
    let mut given = Given::new(Head::Exists);
    given.tags = Some(vec![("Team", "platform"), ("Project", "portal")]);
    let rig = rig(&given, true)?;
    ensure(&rig.ctx).await?;
    assert_eq!(
        recorded(&rig.tags_written),
        ["ManagedBy=Terraform,Project=portal,Purpose=TerraformState,Team=platform"]
    );
    Ok(())
}

#[tokio::test]
async fn complete_tags_and_policy_are_left_untouched() -> TestResult {
    let mut given = Given::new(Head::Exists);
    given.tags = Some(vec![
        ("Project", "portal"),
        ("ManagedBy", "Terraform"),
        ("Purpose", "TerraformState"),
        ("Team", "platform"),
    ]);
    let rig_with_policy = rig(&given, true)?;
    given.policy = Some(
        json!({"Version": "2012-10-17", "Statement": [tls_policy(&rig_with_policy)]}).to_string(),
    );
    let rig = rig(&given, true)?;
    ensure(&rig.ctx).await?;
    assert_eq!(
        [rig.tags_put.num_calls(), rig.policy_put.num_calls()],
        [0, 0]
    );
    Ok(())
}

#[tokio::test]
async fn another_projects_tag_is_never_adopted_or_overwritten() -> TestResult {
    let mut given = Given::new(Head::Exists);
    given.tags = Some(vec![("Project", "someone-else")]);
    let rig = rig(&given, true)?;
    let error = ensure(&rig.ctx).await;
    assert_eq!(error.map_err(|e| e.code()), Err("PRECONDITION_FAILED"));
    assert_eq!(
        [rig.tags_put.num_calls(), rig.policy_put.num_calls()],
        [0, 0]
    );
    Ok(())
}

#[tokio::test]
async fn existing_policy_statements_are_preserved_when_tls_only_is_added() -> TestResult {
    let mut given = Given::new(Head::Exists);
    let other = json!({"Sid": "Audit", "Effect": "Allow", "Principal": {"AWS": "arn:aws:iam::123456789012:root"},
        "Action": "s3:GetBucketLocation", "Resource": "arn:aws:s3:::portal-123456789012-us-east-1-tfstate"});
    given.policy =
        Some(json!({"Version": "2012-10-17", "Id": "keep", "Statement": [other]}).to_string());
    let rig = rig(&given, true)?;
    ensure(&rig.ctx).await?;
    let written: Value = serde_json::from_str(&only(&rig.policy_written))?;
    assert_eq!(
        written,
        json!({"Version": "2012-10-17", "Id": "keep", "Statement": [other, tls_policy(&rig)]})
    );
    Ok(())
}

#[tokio::test]
async fn ambiguous_policies_fail_closed_without_a_write() -> TestResult {
    let mut weaker = Given::new(Head::Exists);
    let rig_for_statement = rig(&weaker, true)?;
    let mut changed = tls_policy(&rig_for_statement);
    let condition = changed
        .get_mut("Condition")
        .ok_or("TLS condition missing")?;
    *condition = json!({"Bool": {"aws:SecureTransport": "true"}});
    let policies = [
        "not json".to_owned(),
        r#"{"Version":"2012-10-17","Statement":{"Sid":"One"}}"#.to_owned(),
        json!({"Version": "2012-10-17", "Statement": [changed]}).to_string(),
    ];
    for policy in policies {
        weaker.policy = Some(policy);
        let rig = rig(&weaker, true)?;
        let error = ensure(&rig.ctx).await;
        assert_eq!(error.map_err(|e| e.code()), Err("PRECONDITION_FAILED"));
        assert_eq!(rig.policy_put.num_calls(), 0);
    }
    Ok(())
}

#[tokio::test]
async fn encryption_is_set_to_exactly_the_supplied_key_with_bucket_keys() -> TestResult {
    let rig = rig(&Given::new(Head::Exists), true)?;
    encrypt(&rig.ctx, KEY).await?;
    assert_eq!(
        recorded(&rig.encryption_written),
        [format!(r#"Some("aws:kms")|Some("{KEY}")|Some(true)"#)]
    );
    assert_eq!(rig.writes(), [0; 6]);
    Ok(())
}

#[tokio::test]
async fn a_key_that_is_not_a_kms_arn_is_refused_before_any_call() -> TestResult {
    let rig = rig(&Given::new(Head::Exists), true)?;
    for bad in [
        "",
        "alias/portal-terraform-state",
        "arn:aws:kms:us-east-1:123456789012:alias/portal-terraform-state",
        "arn:aws:s3:::bucket",
    ] {
        let error = encrypt(&rig.ctx, bad).await;
        assert_eq!(error.map_err(|e| e.code()), Err("INVALID_INPUT"), "{bad}");
    }
    assert_eq!(rig.encryption.num_calls(), 0);
    Ok(())
}

fn creation_rules(given: &Given, bucket: &str, created_with: &Seen) -> [Rule; 5] {
    let bucket = bucket.to_owned();
    let ours = owns_bucket(bucket.clone());
    let head = mock!(Client::head_bucket).match_requests({
        let ours = ours.clone();
        move |r| ours(r.bucket(), r.expected_bucket_owner())
    });
    let head = match given.head {
        Head::Exists => head.then_output(|| HeadBucketOutput::builder().build()),
        Head::Missing => head.then_error(|| HeadBucketError::NotFound(NotFound::builder().build())),
        Head::Denied => head.then_error(|| HeadBucketError::generic(code("403"))),
    };

    let (name, region) = (bucket, given.region);
    let seen = created_with.clone();
    let create = mock!(Client::create_bucket).match_requests(move |r| {
        let location = r
            .create_bucket_configuration()
            .and_then(|c| c.location_constraint())
            .map(|l| l.as_str().to_owned());
        remember(&seen, format!("{location:?}"));
        let location_ok = if region == "us-east-1" {
            location.is_none()
        } else {
            location.as_deref() == Some(region)
        };
        r.bucket() == Some(name.as_str())
            && location_ok
            && r.object_ownership() == Some(&ObjectOwnership::BucketOwnerEnforced)
    });
    let create = match given.create {
        Create::Created => create.then_output(|| CreateBucketOutput::builder().build()),
        Create::OwnedByYou => create.then_error(|| {
            CreateBucketError::BucketAlreadyOwnedByYou(BucketAlreadyOwnedByYou::builder().build())
        }),
        Create::Taken => {
            create.then_error(|| CreateBucketError::generic(code("BucketAlreadyExists")))
        }
    };

    let access = mock!(Client::put_public_access_block)
        .match_requests({
            let ours = ours.clone();
            move |r| {
                ours(r.bucket(), r.expected_bucket_owner())
                    && r.public_access_block_configuration().is_some_and(|c| {
                        [
                            c.block_public_acls(),
                            c.ignore_public_acls(),
                            c.block_public_policy(),
                            c.restrict_public_buckets(),
                        ] == [Some(true); 4]
                    })
            }
        })
        .then_output(|| PutPublicAccessBlockOutput::builder().build());
    let ownership = mock!(Client::put_bucket_ownership_controls)
        .match_requests({
            let ours = ours.clone();
            move |r| {
                let rules = r.ownership_controls().map(OwnershipControls::rules);
                ours(r.bucket(), r.expected_bucket_owner())
                    && rules.is_some_and(|rules| {
                        rules.len() == 1
                            && rules.iter().all(|rule| {
                                rule.object_ownership() == &ObjectOwnership::BucketOwnerEnforced
                            })
                    })
            }
        })
        .then_output(|| PutBucketOwnershipControlsOutput::builder().build());
    let versioning = mock!(Client::put_bucket_versioning)
        .match_requests({
            let ours = ours.clone();
            move |r| {
                ours(r.bucket(), r.expected_bucket_owner())
                    && r.versioning_configuration()
                        .and_then(VersioningConfiguration::status)
                        == Some(&BucketVersioningStatus::Enabled)
            }
        })
        .then_output(|| PutBucketVersioningOutput::builder().build());
    [head, create, access, ownership, versioning]
}

fn tagging_rules(
    given: &Given,
    bucket: &str,
    tags_written: &Seen,
) -> std::result::Result<[Rule; 2], Box<dyn std::error::Error>> {
    let ours = owns_bucket(bucket.to_owned());
    let existing_tags = {
        let mut tags = Vec::new();
        for (key, value) in given.tags.iter().flatten() {
            tags.push(Tag::builder().key(*key).value(*value).build()?);
        }
        GetBucketTaggingOutput::builder()
            .set_tag_set(Some(tags))
            .build()?
    };
    let tags_get = mock!(Client::get_bucket_tagging).match_requests({
        let ours = ours.clone();
        move |r| ours(r.bucket(), r.expected_bucket_owner())
    });
    let tags_get = if given.tags.is_some() {
        tags_get.then_output(move || existing_tags.clone())
    } else {
        tags_get.then_error(|| GetBucketTaggingError::generic(code("NoSuchTagSet")))
    };
    let seen = tags_written.clone();
    let tags_put = mock!(Client::put_bucket_tagging)
        .match_requests({
            let ours = ours.clone();
            move |r| {
                let tag_set = r
                    .tagging()
                    .map(Tagging::tag_set)
                    .map_or_else(Vec::new, <[aws_sdk_s3::types::Tag]>::to_vec);
                let mut tags: Vec<String> = tag_set
                    .iter()
                    .map(|t| format!("{}={}", t.key(), t.value()))
                    .collect();
                tags.sort();
                remember(&seen, tags.join(","));
                ours(r.bucket(), r.expected_bucket_owner())
            }
        })
        .then_output(|| PutBucketTaggingOutput::builder().build());
    Ok([tags_get, tags_put])
}

fn owns_bucket(bucket: String) -> impl Fn(Option<&str>, Option<&str>) -> bool + Clone {
    move |name, owner| name == Some(bucket.as_str()) && owner == Some(ACCOUNT)
}

fn encryption_rules(
    given: &Given,
    bucket: &str,
    encryption_written: &Seen,
) -> std::result::Result<[Rule; 2], Box<dyn std::error::Error>> {
    let ours = owns_bucket(bucket.to_owned());
    let defaults = ServerSideEncryptionByDefault::builder()
        .sse_algorithm(given.encryption_algorithm.clone())
        .set_kms_master_key_id(given.encryption_key.map(str::to_owned))
        .build()?;
    let configuration = ServerSideEncryptionConfiguration::builder()
        .rules(
            ServerSideEncryptionRule::builder()
                .apply_server_side_encryption_by_default(defaults)
                .build(),
        )
        .build()?;
    let encryption_get = mock!(Client::get_bucket_encryption).match_requests({
        let ours = ours.clone();
        move |request| ours(request.bucket(), request.expected_bucket_owner())
    });
    let encryption_get = if given.encryption_read_denied {
        encryption_get.then_error(|| GetBucketEncryptionError::generic(code("AccessDenied")))
    } else {
        encryption_get.then_output(move || {
            GetBucketEncryptionOutput::builder()
                .server_side_encryption_configuration(configuration.clone())
                .build()
        })
    };
    let seen = encryption_written.clone();
    let encryption = mock!(Client::put_bucket_encryption)
        .match_requests(move |r| {
            let rules = r
                .server_side_encryption_configuration()
                .map(ServerSideEncryptionConfiguration::rules)
                .map_or_else(
                    Vec::new,
                    <[aws_sdk_s3::types::ServerSideEncryptionRule]>::to_vec,
                );
            for rule in &rules {
                let default = rule.apply_server_side_encryption_by_default();
                remember(
                    &seen,
                    format!(
                        "{:?}|{:?}|{:?}",
                        default.map(|d| d.sse_algorithm().as_str().to_owned()),
                        default
                            .and_then(|d| d.kms_master_key_id())
                            .map(str::to_owned),
                        rule.bucket_key_enabled()
                    ),
                );
            }
            ours(r.bucket(), r.expected_bucket_owner()) && rules.len() == 1
        })
        .then_output(|| PutBucketEncryptionOutput::builder().build());
    Ok([encryption_get, encryption])
}

#[tokio::test]
async fn existing_encryption_is_never_downgraded_or_rekeyed() -> TestResult {
    for (algorithm, key, denied, expected) in [
        (ServerSideEncryption::AwsKms, Some(KEY), false, Ok(())),
        (
            ServerSideEncryption::AwsKms,
            Some("arn:aws:kms:us-east-1:123456789012:key/other"),
            false,
            Err("PRECONDITION_FAILED"),
        ),
        (
            ServerSideEncryption::AwsKmsDsse,
            Some(KEY),
            false,
            Err("PRECONDITION_FAILED"),
        ),
        (ServerSideEncryption::Aes256, None, true, Err("AWS_FAILED")),
    ] {
        let mut given = Given::new(Head::Exists);
        given.encryption_algorithm = algorithm;
        given.encryption_key = key;
        given.encryption_read_denied = denied;
        let rig = rig(&given, true)?;
        assert_eq!(
            encrypt(&rig.ctx, KEY).await.map_err(|error| error.code()),
            expected
        );
        assert_eq!(rig.encryption.num_calls(), usize::from(expected.is_ok()));
    }
    Ok(())
}
