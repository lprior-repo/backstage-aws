# Troubleshooting

Use the JSON stdout envelope and exit status; inspect stderr for Terraform/AWS diagnostics. Never expose tokens, dumps, state or saved plans in logs/tickets. See [Output and errors](reference/output-and-errors.md).

| Symptom | Check / action |
| --- | --- |
| CLI release download or attestation fails | Inspect requested tag, repo and asset `backstage-aws-x86_64-unknown-linux-gnu.tar.gz`; confirm release exists and attestation verifies. Fix the version/release; do not bypass verification. |
| AWS role assumption denied | Check selected `environments/<env>.env`, role ARN, account, region, GitHub OIDC provider, `aud=sts.amazonaws.com`, and trust `sub`. Preview is same-repo PR; production is repository `main`. Job needs `id-token: write`; preview comments need `pull-requests: write`. No GitHub deployment Environments are used. |
| GitHub provider already exists | Set `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN` to its ARN; do not create a duplicate provider. `backstage-aws doctor` checks operator identity/repository access; it requires local Docker/Buildx. |
| Task cannot reach AWS | Check task IP assignment, routes, security groups and execution-role permissions. For fck-nat, check instance health/EIP and both private-subnet ENI routes; replace failed instances with `base --approve`. DB stays isolated. |
| ALB URL is unreachable | Check DNS/hostname, certificate, `BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS` (IPv4 `/8`–`/32`), ALB target health and security groups. Do not expose DB ingress. |
| Spot capacity/interruptions | Spot is default except `production`. Request regular Fargate explicitly: `BACKSTAGE_AWS_SPOT=false backstage-aws env up --approve`. |
| Seed import fails | `seed --file` needs PostgreSQL custom format, e.g. `pg_dump --format=custom`; check versions/extensions and admin-task logs, routes and S3 permissions. Run `base` first. `seed` replaces shared seed only; existing namespaces remain unchanged. |
| Image scan blocks deployment | `env up --scan` requires a completed ECR scan within the last day and rejects counts above configured high/critical thresholds. Inspect findings; rebuild/fix, or review thresholds before changing them. |
| Terraform lock or timeout | Ensure no writer is active for the stack. Only then remove its exact stale `<state key>.tflock`; never delete state. Exit 5 means remote work may continue—inspect Terraform/ECS/RDS before retry. |
| Bootstrap state migration fails | Preserve `infra/bootstrap/terraform.tfstate`; rerun `bootstrap --approve` with the same account/settings. Do not delete local state. |
| Environment fails health wait | Run `env status`; inspect ECS service events, task stopped reason and CloudWatch logs. Check image, secrets, DB access, health endpoint and ALB target. Fix cause, then rerun `env up --approve`; inspect remote work first after timeout. |
| OAuth/guest sign-in fails | Verify `BACKSTAGE_AWS_AUTH_MODE`, OAuth secret and exact callback `https://HOST/api/auth/github/handler/frame`, DNS/TLS and task secret access. Initial production user must match `BACKSTAGE_AWS_OWNER_GITHUB_ID`. Preview defaults to guest. |
| Teardown stops partway | Rerun `env down --approve`; it destroys stack, drops namespace DB, then deletes state. Do not remove state manually to skip a failed step. |

## Keep mutations safe

Serialize writers per stack/namespace; never cancel an active Terraform apply. A stale lock can be removed only after all matching runs have stopped. See [Tear down](guides/teardown.md).

`seed --file` cannot restore an existing namespace. For RDS PITR/snapshot or dump recovery, follow [Back up and restore](guides/backup-and-restore.md); restore/cutover is an operator task, not a CLI feature.
