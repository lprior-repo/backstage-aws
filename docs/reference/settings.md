# Settings

Flags override corresponding `BACKSTAGE_AWS_*` variables. Options are global (before or after subcommands); whitespace-only optional strings are unset. Empty ingress-CIDR and plugin-secret list entries are ignored; whitespace-only entries are validated. The CLI never loads `.env`; generated workflows load `environments/<environment>.env`. `check` needs no deployment settings.

## Required

| Flag | Environment | Validation |
|---|---|---|
| `--name` | `BACKSTAGE_AWS_NAME` | CLI accepts 3–20 chars; lowercase letter first, then lowercase letters, digits, `-`. Terraform bootstrap/access require 3–18 for a complete deployment. |
| `--aws-account-id` | `BACKSTAGE_AWS_AWS_ACCOUNT_ID` | Exactly 12 decimal digits; `doctor` rejects credentials from a different account. |
| `--aws-region` | `BACKSTAGE_AWS_AWS_REGION` | Must match `^[a-z]{2}-[a-z]+-[0-9]+$`, e.g. `us-east-1`. |
| `--domain-name` | `BACKSTAGE_AWS_DOMAIN_NAME` | CLI accepts lowercase DNS labels, no trailing dot; labels 1–63 characters with alphanumeric ends and only lowercase letters/digits/`-`; max 253 characters. Base Terraform additionally requires at least two labels. |
| `--route53-zone-id` | `BACKSTAGE_AWS_ROUTE53_ZONE_ID` | CLI requires `Z` + 5–32 uppercase letters/digits; base uses the same rule. Bootstrap's optional DNS permission input caps the total at 32 characters. |
| `--github-repository` | `BACKSTAGE_AWS_GITHUB_REPOSITORY` | `owner/name`; owner 1–39 alphanumeric/`-`, name 1–100 alphanumeric/`-_.`. |
| `--owner-github-id` | `BACKSTAGE_AWS_OWNER_GITHUB_ID` | Must match `[1-9][0-9]{0,15}` (no leading zero); initial production user. |

Set both `--domain-name` and `--route53-zone-id`, or leave both empty. Exactly one set is rejected (exit 2).

**Domain-free mode** (both empty): `base` creates no ACM certificate, Route 53 record or HTTPS listener. The load balancer serves plain HTTP on port 80 only, to the allowed ingress CIDRs only; its default action is a fixed 404 and each environment attaches a host-header rule. The hostname is `<namespace>.<name>.internal` (`<name>.internal` for `production`), which is not a public DNS name; send it as the `Host` header to the `load_balancer_dns_name` output. `env up` and `check` prove readiness through AWS APIs (ECS service steady and healthy target-group targets) and also report a best-effort `network_probe` (`skipped`, `reachable` or `unreachable`) that never fails the command. Adding a domain later is a re-apply of `base` with both settings set: the certificate, records and HTTPS listener are created and the HTTP listener becomes a redirect; existing resources are not recreated.

HTTP-only tasks disable only CSP's `upgrade-insecure-requests` directive so browsers can load assets without a TLS listener. Domain/HTTPS tasks retain the default upgrade directive. Other CSP protections are unchanged; HTTP mode is for ingress-restricted sandboxes, not production.

## Optional

| Flag | Environment | Default | Validation / effect |
|---|---|---|---|
| `--project DIR` | `BACKSTAGE_AWS_PROJECT` | `.` | Project root. |
| `--container-engine ENGINE` | `BACKSTAGE_AWS_CONTAINER_ENGINE` | `docker` | Exactly `docker` or `podman`; local Compose and image-build command. No automatic fallback. |
| `--budget-email EMAIL` | `BACKSTAGE_AWS_BUDGET_EMAIL` | Unset | Basic email syntax; no monthly budget is created without it. |
| `--alert-email EMAIL` | `BACKSTAGE_AWS_ALERT_EMAIL` | unset | Basic email syntax; database/load-balancer alarms. |
| `--allowed-ingress-cidrs CIDR[,CIDR...]` | `BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS` | empty | Up to 50 IPv4 CIDRs, `/8`–`/32`; `base` requires ≥1. |
| `--environment NAME` | `BACKSTAGE_AWS_ENVIRONMENT` | `preview` | 2–20 lowercase letters/digits/`-`, lowercase letter first; environment `production` defaults to GitHub auth and no Spot. |
| `--pull-request N` | `BACKSTAGE_AWS_PULL_REQUEST` | unset | Unsigned `u64` (zero allowed); derives namespace and identifies PR comments. |
| `--namespace NAME` | `BACKSTAGE_AWS_NAMESPACE` | derived | 1–31 characters, lowercase letter first, then lowercase letters/digits/`-`; no trailing `-`, `db-admin` prefix, or `seed`, `postgres`, `template0`, `template1`, `portal`, `vpc-flow`. Default: `pr-<number>` when a PR is set, otherwise environment. |
| `--hostname HOST` | `BACKSTAGE_AWS_HOSTNAME` | derived | Defaults to the domain apex when namespace is `production`, otherwise `<namespace>.<domain>`; must be the apex or exactly one label below it. |
| `--auth-mode MODE` | `BACKSTAGE_AWS_AUTH_MODE` | `github` when environment is `production`, else `guest` | `guest` or `github`. |
| `--spot BOOL` | `BACKSTAGE_AWS_SPOT` | `true` outside production; `false` in production | Fargate Spot. |
| `--monthly-budget-usd USD` | `BACKSTAGE_AWS_MONTHLY_BUDGET_USD` | `100` | Integer 1–100000; no budget is created without `--budget-email`. |
| `--account-baseline BOOL` | `BACKSTAGE_AWS_ACCOUNT_BASELINE` | `true` | Bootstrap CloudTrail, password policy, S3 public-access block. |
| `--nat-gateways N` | `BACKSTAGE_AWS_NAT_GATEWAYS` | `0` | 0–2 managed gateways. Zero uses public task IPs unless fck-nat is enabled. |
| `--fck-nat BOOL` | `BACKSTAGE_AWS_FCK_NAT` | `false` | One shared NAT instance; private app/admin IPs. Requires zero managed gateways. |
| `--fck-nat-ami-id ID` | `BACKSTAGE_AWS_FCK_NAT_AMI_ID` | discovered | Optional pin matching `ami-` plus 8 or 17 lowercase hex digits; only with fck-nat. Terraform filters for the official publisher and 1.4.0 arm64 release. |
| `--database-instance-class CLASS` | `BACKSTAGE_AWS_DATABASE_INSTANCE_CLASS` | `db.t4g.micro` | Must match `^db\.[a-z0-9]+\.[a-z0-9]+$`; one shared instance. |
| `--database-multi-az BOOL` | `BACKSTAGE_AWS_DATABASE_MULTI_AZ` | `false` | Standby in a second AZ; roughly doubles database compute cost. |
| `--desired-count N` | `BACKSTAGE_AWS_DESIRED_COUNT` | `1` | 1–4 tasks per environment. |
| `--max-high-findings N` | `BACKSTAGE_AWS_MAX_HIGH_FINDINGS` | `0` | Nonnegative `env up --scan` threshold. |
| `--max-critical-findings N` | `BACKSTAGE_AWS_MAX_CRITICAL_FINDINGS` | `0` | Nonnegative `env up --scan` threshold. |
| `--plugin-secrets SPEC[,SPEC...]` | `BACKSTAGE_AWS_PLUGIN_SECRETS` | empty | `name=ENV_VAR` or `name=ENV_VAR:json_key`; references only, not secret values. See validation below. |
| `--github-oidc-provider-arn ARN` | `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN` | unset | Reuse existing GitHub OIDC provider; otherwise bootstrap creates one. |
| `--terraform PATH` | `BACKSTAGE_AWS_TERRAFORM` | `terraform` on `PATH` | Must report 1.17.2; CLI does not install it. |
| `--approve` | `BACKSTAGE_AWS_APPROVE` | `false` | Switch; env form must be `true`. |
Plugin-secret `name` must match `[a-z][a-z0-9_-]{0,40}`; `ENV_VAR` must match `[A-Z][A-Z0-9_]{0,80}` and cannot start with `AWS_`, `POSTGRES_`, `AUTH_`, `NODE_`, `APP_`, `BACKEND_`, or `PGSSL`. Optional `json_key` must match `[A-Za-z0-9_-]{1,80}`. Names and environment variables must each be unique.


## Credentials and deployment

| Credential | Use |
|---|---|
| AWS SDK credential chain | AWS CLI operations; GitHub Actions assumes the bootstrap role through OIDC (`id-token: write`). |
| `GITHUB_TOKEN` or `GH_TOKEN` | GitHub API; preview workflow grants `pull-requests: write`. |
| `BACKSTAGE_AWS_OAUTH_CLIENT_ID`, `BACKSTAGE_AWS_OAUTH_CLIENT_SECRET` | `secrets set-identity` only when `--file` is omitted. Prefer file/secret manager; never pass values on CLI. |

`init` and `bootstrap` both write `environments/<environment>.env`, including the derived `BACKSTAGE_AWS_DEPLOY_ROLE_ARN`; `bootstrap` creates that role. This is ordinary repository config, not a GitHub deployment Environment. The CI deploy role uses OIDC; it does not read secret values. Identity and plugin values remain in Secrets Manager, accessed at runtime by application/task roles.

Defaults: no NAT, one shared `db.t4g.micro` RDS database with 20 GB initial gp3 storage (auto-scaling up to 100 GB), one 1-vCPU/2-GB task per environment, Spot outside production. Budget `$100` is an alert threshold, not a spending cap. Actual cost and live AWS/GitHub behavior are unverified.
