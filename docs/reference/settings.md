# Settings

Flags override `BACKSTAGE_AWS_*` variables. Options are global (before or after subcommands); blank optional strings are unset, empty ingress/plugin entries ignored. The CLI never loads `.env`: generated workflows load `environments/<environment>.env`. `check` needs no deployment settings.

## Required

| Flag | Environment | Validation |
|---|---|---|
| `--name` | `BACKSTAGE_AWS_NAME` | 3–20 chars; lowercase letter first, then lowercase letters, digits, `-`. Resource prefix. |
| `--aws-account-id` | `BACKSTAGE_AWS_AWS_ACCOUNT_ID` | 12 digits; other credential accounts are refused. |
| `--aws-region` | `BACKSTAGE_AWS_AWS_REGION` | Lowercase region form, e.g. `us-east-1`. |
| `--domain-name` | `BACKSTAGE_AWS_DOMAIN_NAME` | DNS labels 1–63 chars, lowercase letters/digits/`-`, alphanumeric ends; max 253 chars. |
| `--route53-zone-id` | `BACKSTAGE_AWS_ROUTE53_ZONE_ID` | `Z` + 5–32 uppercase letters/digits. |
| `--github-repository` | `BACKSTAGE_AWS_GITHUB_REPOSITORY` | `owner/name`; owner 1–39 alphanumeric/`-`, name 1–100 alphanumeric/`-_.`. |
| `--owner-github-id` | `BACKSTAGE_AWS_OWNER_GITHUB_ID` | Positive decimal, 1–16 digits; initial production user. |

## Optional

| Flag | Environment | Default | Validation / effect |
|---|---|---|---|
| `--project DIR` | `BACKSTAGE_AWS_PROJECT` | `.` | Project root. |
| `--budget-email EMAIL` | `BACKSTAGE_AWS_BUDGET_EMAIL` | unset | Basic email syntax; without it no monthly budget is created. |
| `--alert-email EMAIL` | `BACKSTAGE_AWS_ALERT_EMAIL` | unset | Basic email syntax; database/load-balancer alarms. |
| `--allowed-ingress-cidrs CIDR[,CIDR...]` | `BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS` | empty | Up to 50 IPv4 CIDRs, `/8`–`/32`; `base` requires ≥1. |
| `--environment NAME` | `BACKSTAGE_AWS_ENVIRONMENT` | `preview` | 2–20 lowercase letters/digits/`-`, lowercase letter first; `production` selects production defaults. |
| `--pull-request N` | `BACKSTAGE_AWS_PULL_REQUEST` | unset | Unsigned `u64` (zero allowed); derives namespace and identifies PR comments. |
| `--namespace NAME` | `BACKSTAGE_AWS_NAMESPACE` | derived | 1–31 chars, lowercase letter first, then lowercase letters/digits/`-`; no trailing `-`, `db-admin` prefix, or `seed`, `postgres`, `template0`, `template1`, `portal`, `vpc-flow`. Overrides PR/environment. |
| `--hostname HOST` | `BACKSTAGE_AWS_HOSTNAME` | derived | Production domain; otherwise `<namespace>.<domain>`. Must be domain or exactly one label below it. |
| `--auth-mode MODE` | `BACKSTAGE_AWS_AUTH_MODE` | `github` in production, else `guest` | `guest` or `github`. |
| `--spot BOOL` | `BACKSTAGE_AWS_SPOT` | `true` except production | Fargate Spot; production default `false`. |
| `--monthly-budget-usd USD` | `BACKSTAGE_AWS_MONTHLY_BUDGET_USD` | `100` | Integer 1–100000; notification needs `--budget-email`. |
| `--account-baseline BOOL` | `BACKSTAGE_AWS_ACCOUNT_BASELINE` | `true` | Bootstrap CloudTrail, password policy, S3 public-access block. |
| `--nat-gateways N` | `BACKSTAGE_AWS_NAT_GATEWAYS` | `0` | Integer 0–2. Zero: tasks use public addresses/direct egress, but only load balancer reaches them. |
| `--database-instance-class CLASS` | `BACKSTAGE_AWS_DATABASE_INSTANCE_CLASS` | `db.t4g.micro` | `db.<alphanumeric>.<alphanumeric>`; one shared instance. |
| `--database-multi-az BOOL` | `BACKSTAGE_AWS_DATABASE_MULTI_AZ` | `false` | Standby in another AZ; roughly doubles database compute cost. |
| `--desired-count N` | `BACKSTAGE_AWS_DESIRED_COUNT` | `1` | 1–4 tasks per environment. |
| `--max-high-findings N` | `BACKSTAGE_AWS_MAX_HIGH_FINDINGS` | `0` | Nonnegative `env up --scan` threshold. |
| `--max-critical-findings N` | `BACKSTAGE_AWS_MAX_CRITICAL_FINDINGS` | `0` | Nonnegative `env up --scan` threshold. |
| `--plugin-secrets SPEC[,SPEC...]` | `BACKSTAGE_AWS_PLUGIN_SECRETS` | empty | `name=ENV_VAR` or `name=ENV_VAR:json_key`; names and variables unique. References only; values are stored separately. |
| `--github-oidc-provider-arn ARN` | `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN` | unset | Reuse existing GitHub OIDC provider; otherwise bootstrap creates one. |
| `--terraform PATH` | `BACKSTAGE_AWS_TERRAFORM` | `terraform` on `PATH` | Must report 1.17.2; CLI does not install it. |
| `--approve` | `BACKSTAGE_AWS_APPROVE` | `false` | Switch; env form must be `true`. |

## Credentials and deployment

| Credential | Use |
|---|---|
| AWS SDK credential chain | AWS CLI operations; GitHub Actions assumes the bootstrap role through OIDC (`id-token: write`). |
| `GITHUB_TOKEN` or `GH_TOKEN` | GitHub API; preview workflow grants `pull-requests: write`. |
| `BACKSTAGE_AWS_OAUTH_CLIENT_ID`, `BACKSTAGE_AWS_OAUTH_CLIENT_SECRET` | `secrets set-identity` only when `--file` is omitted. Prefer file/secret manager; never pass values on CLI. |

`init` and `bootstrap` write `environments/<environment>.env`; bootstrap adds `BACKSTAGE_AWS_DEPLOY_ROLE_ARN`. This is ordinary repository config, not a GitHub deployment Environment. The CI deploy role uses OIDC; it does not read secret values. Identity and plugin values remain in Secrets Manager, accessed at runtime by application/task roles.

Defaults: no NAT, one shared `db.t4g.micro` RDS database with 20 GB gp3, one 1-vCPU/2-GB task per environment, Spot outside production. Budget `$100` is an alert threshold, not a spending cap. Actual cost and live AWS/GitHub behavior are unverified.
