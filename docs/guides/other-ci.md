# Other CI systems

The deploy contract is a committed `environments/<environment>.env`, optional PR number, and CLI command. Generated workflows support GitHub Actions only.

## Setup

| Requirement | Notes |
| --- | --- |
| CLI + Terraform 1.17.2 | Install through your approved process; Terraform must be on `PATH` or configured with `--terraform`. |
| Docker Buildx | CI builds images; do not depend on a developer image. |
| Settings | Load a reviewed trusted-branch file; it is shell code, not a secrets store. |
| AWS identity | Configure provider-specific short-lived federation/role. Bootstrap creates GitHub OIDC trust only; do not broaden that role for other CI. |
| Serialization | One writer per stack/namespace; do not cancel active applies. Never grant deploy credentials to fork code. |

Use the generated deploy policy as the permissions starting point; it does not read Secrets Manager values. Provider trust is your responsibility.

## GitLab CI example

Configure GitLab workload identity to assume a narrowly scoped AWS role and expose its short-lived credentials to a protected job. This is a shell sketch; runner/federation setup is provider-specific.

```yaml
deploy_preview:
  stage: deploy
  rules:
    - if: '$CI_PIPELINE_SOURCE == "merge_request_event" && $CI_MERGE_REQUEST_SOURCE_PROJECT_PATH == $CI_PROJECT_PATH'
  resource_group: "preview-$CI_MERGE_REQUEST_IID"
  script:
    - set -a
    - . environments/preview.env
    - set +a
    - BACKSTAGE_AWS_PULL_REQUEST="$CI_MERGE_REQUEST_IID" backstage-aws env up --approve
```

Use trusted base-branch settings and configure GitLab-to-AWS federation separately. `env down --approve` can remove the namespace on a trusted close/cleanup job. GitLab merge-request comments are **not** implemented by the CLI; it comments only on GitHub PRs when given `GITHUB_TOKEN`. Call the GitLab API separately if needed.

## Jenkins production example

Use a protected production branch/job, provider-specific AWS role assumption and a lockable resource for serialization:

```groovy
lock(resource: 'backstage-production') {
  sh '''
    set -a
    . environments/production.env
    set +a
    backstage-aws env up --scan --approve
  '''
}
```

To deploy a retained image instead of building:

```sh
backstage-aws env up --scan --digest sha256:... --approve
```

Deploy only reviewed releases; a healthy ECS/ALB rollout does not prove application behavior.

## Result handling

Stdout is one JSON document; progress and child output go to stderr. Use the exit status and parse stdout, not progress text. Exit codes: `0` success, `2` invalid input, `3` blocked/precondition/approval, `4` external failure, `5` timeout (remote work may continue). See [Output and errors](../reference/output-and-errors.md).
