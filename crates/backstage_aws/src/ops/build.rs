//! Builds and pushes the portal image with Docker Buildx (registry layer cache
//! in ECR) or Podman, as selected by the container engine setting.
use crate::{
    container::ContainerEngine,
    ctx::Ctx,
    error::{Error, Result},
    process,
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecr::types::ImageIdentifier;
use serde::Serialize;
use std::{
    ffi::OsStr,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize)]
pub(crate) struct BuildResult {
    pub(crate) image_digest: String,
    pub(crate) image_tag: String,
    pub(crate) repository: String,
}

pub(crate) async fn build(ctx: &Ctx, tag: Option<&str>) -> Result<BuildResult> {
    let engine = ctx.settings.settings.container_engine;
    build_with_engine(ctx, tag, engine, OsStr::new(engine.program())).await
}

async fn build_with_engine(
    ctx: &Ctx,
    tag: Option<&str>,
    engine: ContainerEngine,
    program: &OsStr,
) -> Result<BuildResult> {
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
    match engine {
        ContainerEngine::Docker => {
            publish_with_buildx(ctx, program, &app, &repository, &image_tag).await?;
        }
        ContainerEngine::Podman => {
            publish_with_podman(ctx, program, &app, &repository, &image_tag).await?;
        }
    }
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

async fn publish_with_buildx(
    ctx: &Ctx,
    docker: &OsStr,
    app: &Path,
    repository: &str,
    image_tag: &str,
) -> Result<()> {
    ensure_builder(app, docker).await?;
    login(ctx, repository, docker).await?;
    // ECR needs OCI image manifests for the cache, and a plain image manifest
    // (no provenance index) so the pushed digest can be scanned and deployed.
    let cache = format!("type=registry,ref={repository}:buildcache");
    let args = [
        "buildx",
        "build",
        "--builder=backstage-aws",
        "--platform=linux/amd64",
        "--file=packages/backend/Dockerfile",
        "--provenance=false",
        "--push",
        &format!("--tag={repository}:{image_tag}"),
        &format!("--cache-from={cache}"),
        &format!("--cache-to={cache},mode=max,image-manifest=true,oci-mediatypes=true"),
        ".",
    ];
    process::run(docker, &args, app, None, Duration::from_mins(45)).await
}

/// Podman builds an OCI image (its default format, with no provenance index)
/// and pushes it as-is, so the ECR digest is that of the built manifest.
/// There is no registry layer cache: Podman's `--cache-to` would publish one
/// tag per layer into the immutable-tag repository.
async fn publish_with_podman(
    ctx: &Ctx,
    podman: &OsStr,
    app: &Path,
    repository: &str,
    image_tag: &str,
) -> Result<()> {
    login(ctx, repository, podman).await?;
    let reference = format!("{repository}:{image_tag}");
    let build = [
        "build",
        "--platform=linux/amd64",
        "--file=packages/backend/Dockerfile",
        &format!("--tag={reference}"),
        ".",
    ];
    process::run(podman, &build, app, None, Duration::from_mins(45)).await?;
    process::run(
        podman,
        &["push", &reference],
        app,
        None,
        Duration::from_mins(30),
    )
    .await
}

const BUILDER_NAME: &str = "backstage-aws";

async fn ensure_builder(app: &Path, docker: &OsStr) -> Result<()> {
    let names = process::capture(
        docker,
        &["buildx", "ls", "--format", "{{.Name}}"],
        app,
        Duration::from_secs(30),
    )
    .await?;
    let names = String::from_utf8(names)
        .map_err(|error| Error::tool_failed("docker", error.to_string()))?;
    if !names.lines().any(|name| name.trim() == BUILDER_NAME) {
        process::run(
            docker,
            &[
                "buildx",
                "create",
                "--name",
                BUILDER_NAME,
                "--driver",
                "docker-container",
            ],
            app,
            None,
            Duration::from_secs(30),
        )
        .await?;
    }
    let inspection = process::capture(
        docker,
        &[
            "buildx",
            "inspect",
            "--builder",
            BUILDER_NAME,
            "--bootstrap",
        ],
        app,
        Duration::from_secs(120),
    )
    .await?;
    let inspection = String::from_utf8(inspection)
        .map_err(|error| Error::tool_failed("docker", error.to_string()))?;
    let has_container_driver = inspection.lines().any(|line| {
        line.split_once(':').is_some_and(|(field, value)| {
            field.trim() == "Driver" && value.trim() == "docker-container"
        })
    });
    if !has_container_driver {
        return Err(Error::tool_failed(
            "docker",
            format!("builder {BUILDER_NAME} is not a docker-container builder"),
        ));
    }
    Ok(())
}

/// Logs the container engine in to the registry. The password travels on stdin only.
async fn login(ctx: &Ctx, repository: &str, engine: &OsStr) -> Result<()> {
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
        engine,
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
    use super::{build_with_engine, ensure_builder, validate_digest};
    use crate::container::ContainerEngine;
    use aws_sdk_ecr::{
        operation::{
            describe_images::DescribeImagesOutput,
            get_authorization_token::GetAuthorizationTokenOutput,
        },
        types::{AuthorizationData, ImageDetail},
    };
    use aws_sdk_s3::{operation::get_object::GetObjectOutput, primitives::ByteStream};
    use aws_smithy_mocks::{RuleMode, mock, mock_client};
    use std::{fs, os::unix::fs::PermissionsExt};

    const BASE_STATE: &str = r#"{"outputs":{
        "ecr_repository_url":{"value":"123456789012.dkr.ecr.us-east-1.amazonaws.com/portal-images"},
        "ecr_repository_name":{"value":"portal-images"}}}"#;
    const DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn docker_fixture(
        driver: &str,
        build_exit: i32,
    ) -> TestResult<(
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    )> {
        let temp = tempfile::tempdir()?;
        let app = temp.path().join("app");
        fs::create_dir(&app)?;
        let docker = temp.path().join("docker");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$PWD/docker.calls\"\ncase \"$*\" in\n  'buildx ls '*) printf 'backstage-aws\\n' ;;\n  'buildx inspect '*) printf 'Name: backstage-aws\\nDriver: {driver}\\n' ;;\n  'login '*) cat > \"$PWD/login.stdin\" ;;\n  'buildx build '*) exit {build_exit} ;;\n  *) exit 8 ;;\nesac\n"
        );
        make_executable(&docker, &script)?;
        let calls = app.join("docker.calls");
        Ok((temp, app, docker, calls))
    }

    fn make_executable(path: &std::path::Path, script: &str) -> TestResult {
        fs::write(path, script)?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)?;
        Ok(())
    }

    /// A fake `podman` that logs each argv to `podman.calls` in its working
    /// directory: the project root for `login`, `app/` for build and push.
    fn podman_fixture(
        build_exit: i32,
        push_exit: i32,
    ) -> TestResult<(tempfile::TempDir, std::path::PathBuf, std::path::PathBuf)> {
        let temp = tempfile::tempdir()?;
        let app = temp.path().join("app");
        fs::create_dir(&app)?;
        let podman = temp.path().join("podman");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$PWD/podman.calls\"\ncase \"$*\" in\n  'login '*) cat > \"$PWD/login.stdin\" ;;\n  'build '*) exit {build_exit} ;;\n  'push '*) exit {push_exit} ;;\n  *) exit 8 ;;\nesac\n"
        );
        make_executable(&podman, &script)?;
        let calls = app.join("podman.calls");
        Ok((temp, podman, calls))
    }

    fn mocked_ctx(root: &std::path::Path) -> crate::error::Result<crate::ctx::Ctx> {
        let mut ctx = crate::ctx::Ctx::sample(root, true)?;
        let state = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| request.key() == Some("base/terraform.tfstate"))
            .then_output(|| {
                GetObjectOutput::builder()
                    .body(ByteStream::from_static(BASE_STATE.as_bytes()))
                    .build()
            });
        ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&state]);
        Ok(ctx)
    }

    fn auth_rule() -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_ecr::Client::get_authorization_token).then_output(|| {
            GetAuthorizationTokenOutput::builder()
                .authorization_data(
                    AuthorizationData::builder()
                        .authorization_token("QVdTOnNlY3JldA==")
                        .proxy_endpoint("https://123456789012.dkr.ecr.us-east-1.amazonaws.com")
                        .build(),
                )
                .build()
        })
    }

    fn describe_rule() -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_ecr::Client::describe_images)
            .match_requests(|request| {
                request.repository_name() == Some("portal-images")
                    && request.image_ids().first().and_then(|id| id.image_tag())
                        == Some("release-1")
            })
            .then_output(|| {
                DescribeImagesOutput::builder()
                    .image_details(
                        ImageDetail::builder()
                            .image_digest(DIGEST)
                            .image_tags("release-1")
                            .build(),
                    )
                    .build()
            })
    }

    #[tokio::test]
    async fn build_pushes_the_requested_tag_and_looks_up_that_exact_ecr_image() -> TestResult {
        let (temp, _app, docker, calls) = docker_fixture("       docker-container", 0)?;
        let mut ctx = mocked_ctx(temp.path())?;
        let auth = auth_rule();
        let describe = describe_rule();
        ctx.ecr = mock_client!(aws_sdk_ecr, RuleMode::MatchAny, [&auth, &describe]);

        let result = build_with_engine(
            &ctx,
            Some("release-1"),
            ContainerEngine::Docker,
            docker.as_os_str(),
        )
        .await?;

        assert_eq!(result.image_tag, "release-1");
        assert_eq!(result.image_digest, DIGEST);
        assert_eq!(
            result.repository,
            "123456789012.dkr.ecr.us-east-1.amazonaws.com/portal-images"
        );
        assert_eq!((auth.num_calls(), describe.num_calls()), (1, 1));
        let docker_calls = fs::read_to_string(calls)?;
        assert_eq!(
            docker_calls.lines().collect::<Vec<_>>(),
            [
                "buildx ls --format {{.Name}}",
                "buildx inspect --builder backstage-aws --bootstrap",
                "buildx build --builder=backstage-aws --platform=linux/amd64 --file=packages/backend/Dockerfile --provenance=false --push --tag=123456789012.dkr.ecr.us-east-1.amazonaws.com/portal-images:release-1 --cache-from=type=registry,ref=123456789012.dkr.ecr.us-east-1.amazonaws.com/portal-images:buildcache --cache-to=type=registry,ref=123456789012.dkr.ecr.us-east-1.amazonaws.com/portal-images:buildcache,mode=max,image-manifest=true,oci-mediatypes=true ."
            ]
        );
        let login = fs::read_to_string(temp.path().join("login.stdin"))?;
        assert_eq!(login, "secret");
        let login_args = fs::read_to_string(temp.path().join("docker.calls"))?;
        assert_eq!(
            login_args.lines().collect::<Vec<_>>(),
            ["login --username=AWS --password-stdin 123456789012.dkr.ecr.us-east-1.amazonaws.com"]
        );
        assert!(!login_args.contains("secret"));
        Ok(())
    }

    #[tokio::test]
    async fn docker_build_failure_prevents_the_ecr_digest_lookup() -> TestResult {
        let (temp, _app, docker, calls) = docker_fixture("docker-container", 19)?;
        let mut ctx = mocked_ctx(temp.path())?;
        let auth = auth_rule();
        let describe = describe_rule();
        ctx.ecr = mock_client!(aws_sdk_ecr, RuleMode::MatchAny, [&auth, &describe]);

        assert!(
            build_with_engine(
                &ctx,
                Some("release-1"),
                ContainerEngine::Docker,
                docker.as_os_str(),
            )
            .await
            .is_err()
        );

        assert_eq!((auth.num_calls(), describe.num_calls()), (1, 0));
        let docker_calls = fs::read_to_string(calls)?;
        assert_eq!(docker_calls.lines().count(), 3);
        assert!(
            docker_calls
                .lines()
                .last()
                .is_some_and(|line| line.starts_with("buildx build "))
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("login.stdin"))?,
            "secret"
        );
        Ok(())
    }

    const REPOSITORY: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/portal-images";
    const REGISTRY: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com";

    #[tokio::test]
    async fn podman_builds_pushes_the_tag_and_returns_the_ecr_digest() -> TestResult {
        let (temp, podman, calls) = podman_fixture(0, 0)?;
        let mut ctx = mocked_ctx(temp.path())?;
        let auth = auth_rule();
        let describe = describe_rule();
        ctx.ecr = mock_client!(aws_sdk_ecr, RuleMode::MatchAny, [&auth, &describe]);

        let result = build_with_engine(
            &ctx,
            Some("release-1"),
            ContainerEngine::Podman,
            podman.as_os_str(),
        )
        .await?;

        assert_eq!(result.image_tag, "release-1");
        assert_eq!(result.image_digest, DIGEST);
        assert_eq!(result.repository, REPOSITORY);
        assert_eq!((auth.num_calls(), describe.num_calls()), (1, 1));
        let app_calls = fs::read_to_string(calls)?;
        assert_eq!(
            app_calls.lines().collect::<Vec<_>>(),
            [
                format!(
                    "build --platform=linux/amd64 --file=packages/backend/Dockerfile --tag={REPOSITORY}:release-1 ."
                ),
                format!("push {REPOSITORY}:release-1"),
            ]
        );
        let login_calls = fs::read_to_string(temp.path().join("podman.calls"))?;
        assert_eq!(
            login_calls.lines().collect::<Vec<_>>(),
            [format!("login --username=AWS --password-stdin {REGISTRY}")]
        );
        assert!(!login_calls.contains("secret"));
        assert_eq!(
            fs::read_to_string(temp.path().join("login.stdin"))?,
            "secret"
        );
        Ok(())
    }

    #[tokio::test]
    async fn podman_build_failure_prevents_the_push_and_the_ecr_digest_lookup() -> TestResult {
        let (temp, podman, calls) = podman_fixture(19, 0)?;
        let mut ctx = mocked_ctx(temp.path())?;
        let auth = auth_rule();
        let describe = describe_rule();
        ctx.ecr = mock_client!(aws_sdk_ecr, RuleMode::MatchAny, [&auth, &describe]);

        let outcome = build_with_engine(
            &ctx,
            Some("release-1"),
            ContainerEngine::Podman,
            podman.as_os_str(),
        )
        .await;

        assert!(outcome.is_err());
        assert_eq!((auth.num_calls(), describe.num_calls()), (1, 0));
        let app_calls = fs::read_to_string(calls)?;
        assert_eq!(app_calls.lines().count(), 1);
        assert!(app_calls.starts_with("build "));
        Ok(())
    }

    #[tokio::test]
    async fn podman_push_failure_prevents_the_ecr_digest_lookup() -> TestResult {
        let (temp, podman, calls) = podman_fixture(0, 21)?;
        let mut ctx = mocked_ctx(temp.path())?;
        let auth = auth_rule();
        let describe = describe_rule();
        ctx.ecr = mock_client!(aws_sdk_ecr, RuleMode::MatchAny, [&auth, &describe]);

        let outcome = build_with_engine(
            &ctx,
            Some("release-1"),
            ContainerEngine::Podman,
            podman.as_os_str(),
        )
        .await;

        assert!(outcome.is_err());
        assert_eq!((auth.num_calls(), describe.num_calls()), (1, 0));
        let app_calls = fs::read_to_string(calls)?;
        assert_eq!(app_calls.lines().count(), 2);
        assert!(
            app_calls
                .lines()
                .last()
                .is_some_and(|line| line.starts_with("push "))
        );
        Ok(())
    }

    #[tokio::test]
    async fn accepts_the_documented_padded_buildx_driver_field() -> TestResult {
        let (_temp, app, docker, calls) = docker_fixture("       docker-container", 0)?;
        ensure_builder(&app, docker.as_os_str()).await?;
        let calls = fs::read_to_string(calls)?;
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            [
                "buildx ls --format {{.Name}}",
                "buildx inspect --builder backstage-aws --bootstrap"
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejects_a_non_container_driver_without_creating_another_builder() -> TestResult {
        let (_temp, app, docker, calls) = docker_fixture("docker", 0)?;
        let error = ensure_builder(&app, docker.as_os_str())
            .await
            .err()
            .ok_or("wrong builder driver must be rejected")?;
        assert_eq!(
            error.to_string(),
            "docker: builder backstage-aws is not a docker-container builder"
        );
        let calls = fs::read_to_string(calls)?;
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            [
                "buildx ls --format {{.Name}}",
                "buildx inspect --builder backstage-aws --bootstrap"
            ]
        );
        Ok(())
    }

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
