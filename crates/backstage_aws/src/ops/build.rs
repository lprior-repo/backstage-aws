//! Builds and pushes the portal image, using a registry layer cache in ECR.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    process,
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecr::types::ImageIdentifier;
use serde::Serialize;
use std::{
    ffi::OsStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize)]
pub(crate) struct BuildResult {
    pub(crate) image_digest: String,
    pub(crate) image_tag: String,
    pub(crate) repository: String,
}

pub(crate) async fn build(ctx: &Ctx, tag: Option<&str>) -> Result<BuildResult> {
    ctx.require_approval()?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let repository = output_str(&base, "ecr_repository_url")?.to_owned();
    // Tags are immutable, so two namespaces building in the same second must
    // not ask for the same one.
    let generated = || format!("{}-{}", ctx.settings.namespace, unix_seconds());
    let image_tag = tag.map_or_else(generated, str::to_owned);
    let tag_ok = |byte: u8| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte);
    if image_tag.is_empty() || image_tag.len() > 128 || !image_tag.bytes().all(tag_ok) {
        return Err(Error::invalid_input(
            "image tag must be 1-128 characters of letters, digits, `_`, `.` or `-`",
        ));
    }
    let app = ctx.project_dir.join("app");
    login(ctx, &repository).await?;
    // ECR needs OCI image manifests for the cache, and a plain image manifest
    // (no provenance index) so the pushed digest can be scanned and deployed.
    let cache = format!("type=registry,ref={repository}:buildcache");
    let args = [
        "buildx",
        "build",
        "--platform=linux/amd64",
        "--file=packages/backend/Dockerfile",
        "--provenance=false",
        "--push",
        &format!("--tag={repository}:{image_tag}"),
        &format!("--cache-from={cache}"),
        &format!("--cache-to={cache},mode=max,image-manifest=true,oci-mediatypes=true"),
        ".",
    ];
    process::run(
        OsStr::new("docker"),
        &args,
        &app,
        None,
        Duration::from_mins(45),
    )
    .await?;
    let described = ctx
        .ecr
        .describe_images()
        .repository_name(output_str(&base, "ecr_repository_name")?)
        .image_ids(ImageIdentifier::builder().image_tag(&image_tag).build())
        .send()
        .await
        .map_err(|error| Error::aws_failed("ecr", "DescribeImages", error.to_string()))?;
    let image_digest = described
        .image_details()
        .first()
        .and_then(|image| image.image_digest())
        .ok_or_else(|| Error::aws_failed("ecr", "DescribeImages", "pushed image has no digest"))?
        .to_owned();
    validate_digest(&image_digest)?;
    Ok(BuildResult {
        image_digest,
        image_tag,
        repository,
    })
}

/// Logs Docker in to the registry. The password travels on stdin only.
async fn login(ctx: &Ctx, repository: &str) -> Result<()> {
    let failed = |detail: &str| Error::aws_failed("ecr", "GetAuthorizationToken", detail);
    let response = ctx
        .ecr
        .get_authorization_token()
        .send()
        .await
        .map_err(|error| failed(&error.to_string()))?;
    let token = response
        .authorization_data()
        .first()
        .and_then(|data| data.authorization_token())
        .and_then(|token| aws_smithy_types::base64::decode(token).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| failed("no usable authorization token"))?;
    let password = token
        .strip_prefix("AWS:")
        .ok_or_else(|| failed("unexpected authorization token format"))?;
    let registry = repository.split('/').next().unwrap_or(repository);
    let args = ["login", "--username=AWS", "--password-stdin", registry];
    let limit = Duration::from_mins(1);
    process::run(
        OsStr::new("docker"),
        &args,
        &ctx.project_dir,
        Some(password.as_bytes()),
        limit,
    )
    .await
}

pub(crate) fn validate_digest(digest: &str) -> Result<()> {
    let hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
    match digest.strip_prefix("sha256:") {
        Some(rest) if rest.len() == 64 && rest.bytes().all(hex) => Ok(()),
        _ => Err(Error::invalid_input(
            "image digest must be `sha256:` followed by 64 lowercase hex characters",
        )),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::validate_digest;

    #[test]
    fn only_lowercase_sha256_digests_are_accepted() {
        let good = format!("sha256:{}", "0a".repeat(32));
        assert!(validate_digest(&good).is_ok());
        for bad in [
            format!("sha256:{}", "0A".repeat(32)),
            format!("sha256:{}", "0a".repeat(31)),
            format!("sha512:{}", "0a".repeat(32)),
            "latest".to_owned(),
        ] {
            assert!(validate_digest(&bad).is_err(), "{bad}");
        }
    }
}
