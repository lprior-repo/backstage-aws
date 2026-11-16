# Automated workflows

Settings live in committed `environments/preview.env` and, once provisioned, `environments/production.env`. The setup action loads their `BACKSTAGE_AWS_*` lines, installs Terraform 1.17.2, and downloads/verifies the matching CLI release. The CLI reads env/flags, not these files directly. Never place secret values in settings files.

## Preview

`preview.yml` handles same-repository PR opens, updates, and reopens. It checks out the PR merge revision, uses the shared setup action to load preview settings and install Terraform and the verified CLI, assumes the preview deploy role using GitHub OIDC, and calls:

```sh
backstage-aws env up --approve
```

`BACKSTAGE_AWS_PULL_REQUEST` supplies the PR number; `GITHUB_TOKEN` permits the CLI to create/update the comment. Fork PRs are excluded. The CLI builds and pushes the image with the selected container engine (Docker Buildx by default) while cloning the seed database, then applies the namespace stack, waits for ECS/ALB readiness, and posts the URL.

Close events use `pull_request_target` and trusted base-branch code to call `backstage-aws env down --approve`. They have PR-write permission so the CLI can update the comment. Deploy and teardown share a per-PR concurrency group and do not cancel active applies. GitHub may replace older pending runs.

## Production

`deploy.yml` runs on `main` or manual dispatch. It skips until `environments/production.env` is committed. Provision production and its OAuth secret in a separate AWS account before committing that file.

When `production.env` exists, the shared setup action loads it and installs Terraform and the verified CLI. The workflow assumes the production role through GitHub OIDC and calls `backstage-aws env up --scan --approve`. The CLI uses Docker Buildx to build and push the image; a manual image digest skips the build. Protect `main` and review workflow/settings changes; there is no GitHub deployment Environment or separate deployment approval.

## Identity and validation

Preview OIDC trust accepts the configured repository's subjects; production accepts only its `ref:refs/heads/main` subject. Both require `aud=sts.amazonaws.com`. The workflows use no static AWS credentials, GitHub deployment Environments, or environment-scoped variables. GitHub Actions provides `GITHUB_TOKEN` for preview PR comments; OIDC supplies the AWS identity.

`Validate source` runs Terraform checks and the app's immutable install, typecheck, tests, backend build, and local PostgreSQL/application rehearsal. It needs no AWS deployment credentials. These checks do not prove an AWS rollout or application behavior.

For another CI system, make the same CLI, Terraform 1.17.2, and Docker Buildx or Podman available, load settings, supply your own short-lived AWS identity, serialize mutations, and call the same commands. GitHub comments remain a GitHub-specific CLI feature; other providers can consume the JSON result. [Other CI guide](https://github.com/lprior-repo/backstage-aws/blob/main/docs/guides/other-ci.md).
