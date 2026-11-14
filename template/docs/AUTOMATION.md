# Automated workflows

Settings live in committed `environments/preview.env` and, once provisioned, `environments/production.env`. The setup action loads their `BACKSTAGE_AWS_*` lines, installs Terraform 1.17.2, and downloads/verifies the matching CLI release. The CLI reads env/flags, not these files directly. Never place secret values in settings files.

## Preview

`preview.yml` handles same-repository PR opens, updates, and reopens. It checks out the PR merge revision, loads preview settings, configures Buildx, assumes the preview deploy role using GitHub OIDC, and calls:

```sh
backstage-aws env up --approve
```

`BACKSTAGE_AWS_PULL_REQUEST` supplies the PR number; `GITHUB_TOKEN` permits the CLI to create/update the comment. Fork PRs are excluded. The CLI builds/pushes while cloning the seed database, applies the namespace stack, waits for ECS/ALB readiness, then posts the URL and returns JSON.

Close events use `pull_request_target` and trusted base-branch code to call `env down`. They have PR-write permission so the CLI can update the comment. Deploy and teardown share a per-PR concurrency group and do not cancel active applies. GitHub may replace older pending runs.

## Production

`deploy.yml` runs on `main` or manual dispatch. It skips until `environments/production.env` is committed. Provision production and its OAuth secret in a separate AWS account before committing that file.

It configures Buildx, signs in through GitHub OIDC, and calls `backstage-aws env up --scan --approve`. A manual image digest skips building. Protect `main` and review workflow/settings changes; there is no GitHub deployment Environment or separate deployment approval.

## Identity and validation

Preview OIDC trust accepts the configured repository's subjects; production accepts only its `ref:refs/heads/main` subject. Both require `aud=sts.amazonaws.com`. No static AWS keys or GitHub environment variables are needed.

`validate.yml` runs offline Terraform checks and the app's immutable install, typecheck, tests and backend build. It needs no AWS deployment credentials. Offline success does not prove an AWS rollout or application behavior.

For another CI system, install the same CLI/Terraform/Buildx, load settings, supply your own short-lived AWS identity, serialize mutations, and call the same commands. GitHub comments remain a GitHub-specific CLI feature; other providers can consume the JSON result. [Other CI guide](https://github.com/lprior-repo/backstage-aws/blob/main/docs/guides/other-ci.md).
