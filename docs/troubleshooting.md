# Troubleshooting

Every command writes one JSON document to stdout; only `--help` and `--version` print plain text. Progress and child-tool diagnostics go to stderr. Use `error.code` and `error.message`; `SETUP_BLOCKED` also includes `error.details`. Never put tokens, dumps, state files or saved plans in tickets. See [JSON output and errors](reference/output-and-errors.md) and the [CLI reference](reference/cli.md).

## Exit codes and error codes

| Exit | Error code(s) | What to do |
|---:|---|---|
| `0` | — | Command succeeded. |
| `2` | `INVALID_INPUT` | Fix flags, required settings, paths, names or CIDRs. |
| `3` | `APPROVAL_REQUIRED`, `PRECONDITION_FAILED`, `SETUP_BLOCKED` | `setup` is read-only; follow its single `error.details.next` action and rerun `error.details.resume`. For mutations, add `--approve` only after reviewing and approving the change. |
| `4` | `TOOL_FAILED`, `AWS_FAILED`, `GITHUB_FAILED`, `STATE_FAILED`, `DB_TASK_FAILED` | Inspect the code/message and stderr. Resolve the tool, permission, API, state or database-task failure; check for partial remote changes before retrying. |
| `5` | `REMOTE_TIMEOUT` | The remote operation may still be running. Inspect the AWS resource and operation status before retrying; do not assume it was cancelled. |

`setup` deliberately stops at the first incomplete check. Its blocked report uses exit 3 and includes the checked stages, next action and resumable argv. `organization-email` and `identity-center-mfa` are manual confirmations, not API-verified facts.

## Common failures

| Symptom | Check / action |
|---|---|
| Setup is blocked on credentials or account | Use a configured non-root IAM Identity Center profile; check `aws sts get-caller-identity --profile "$AWS_PROFILE"` and the exact target account ID. Root MFA must be on and root access keys absent. |
| Setup requests an account action | Follow its `next` instructions in order. Verify the Organization management-account email and required Identity Center MFA in the console before supplying the matching `--confirm` value. |
| Setup rejects settings or DNS | Load the generated `environments/preview.env`; provide every required setting (domain and zone ID are both set or both empty) and at least one allowed IPv4 CIDR. If a domain is set, ensure the public Route 53 zone is delegated to its assigned name servers; compare `dig +short NS DOMAIN` with `aws route53 get-hosted-zone`. |
| Terraform version is rejected | Install exactly 1.17.2. `BACKSTAGE_AWS_TERRAFORM` can select another executable path, not another version. |
| Docker or image build fails | Docker: check `docker buildx version` and `docker info`, and that the daemon is reachable. Podman (`--container-engine podman`): check `podman info`. Run them as the same user that runs the CLI. |
| GitHub release download or attestation fails | Confirm generated workflows request the matching tag and repository and that the release asset/attestation verifies. Ask the maintainer to repair a missing or invalid release; do not bypass verification. |
| GitHub publish or OIDC role assumption fails | Check `GITHUB_TOKEN`/`GH_TOKEN`, repository owner/name, token workflow-file access, and admin access for an existing repository. For Actions, check `id-token: write`, OIDC audience `sts.amazonaws.com`, role ARN and trust subject. Fork pull requests do not deploy. |
| GitHub OIDC provider already exists | Set `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN` to the existing provider ARN before bootstrap; do not try to create a duplicate. |
| ALB URL is unreachable | Check DNS, hostname, certificate (domain mode) or the `Host` header `<namespace>.<name>.internal` (domain-free mode), configured IPv4 ingress CIDRs, ALB target health and security groups. App task ingress is from the ALB only; never open database ingress. |
| Task cannot reach AWS services | Check public-IP assignment, routes, security groups and execution-role permissions. If using `fck-nat`, check instance health/EIP and private-subnet routes. RDS remains isolated. |
| Spot task is interrupted | Spot is default except production. Set `BACKSTAGE_AWS_SPOT=false` for regular Fargate, then rerun the reviewed `env up --approve`. |
| Seed import fails | `seed --file` requires a PostgreSQL custom-format dump, such as `pg_dump --format=custom`. Check admin-task logs, routes, versions/extensions and S3 permissions. It replaces the shared seed only; existing namespace databases do not change. |
| Image scan blocks deployment | `env up --scan --approve` requires an ECR scan completed within the last day and enforces configured high/critical finding limits. Inspect findings before changing limits. |
| Terraform lock or remote timeout | Ensure no writer is active for that stack. Remove an exact stale S3 `.tflock` object only after all matching runs have stopped; never delete state. Exit 5 means remote work may continue. |
| Bootstrap state migration fails or was interrupted | The CLI, not Terraform, owns the state bucket: it creates it private and versioned with default SSE-S3, applies the local KMS key, configures that exact key as the bucket default before the first remote upload, and never downgrades existing encryption. Rerun `backstage-aws bootstrap --approve`; it resumes from preserved local state. Do not run `terraform untaint`, delete the bucket, or delete `infra/bootstrap/terraform.tfstate`. |
| Bootstrap is denied `cloudtrail:CreateTrail` or other account-baseline actions | The account owner must keep the access stack's scoped `enable_account_controls = true` policy enabled until `bootstrap` succeeds, then disable it. Do not grant administrator access or disable the baseline to bypass a denied action. The trail tag policy must allow the bootstrap provider's `Project`, `ManagedBy` and `Purpose` tag keys. |
| Environment does not become healthy | Run `backstage-aws env status`; inspect ECS service events, stopped-task reason, CloudWatch logs, image, secrets, DB access and ALB health. After a timeout, inspect remote work before retrying `backstage-aws env up --approve`. |
| Teardown stops partway | After confirming no matching operation is active, rerun `backstage-aws env down --approve`. It removes the namespace service, database and state; do not delete state manually. |

Serialize writers per stack/namespace and never cancel an active Terraform apply. `seed --file` cannot restore an existing namespace. For RDS point-in-time/snapshot recovery or dump handling, follow [Backup and restore](guides/backup-and-restore.md); restore and cutover are operator tasks, not CLI commands.
