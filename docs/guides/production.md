# Production

Production is a separate AWS account and environment named `production`. Its name disables Spot by default; explicitly set `BACKSTAGE_AWS_SPOT=false` so a copied preview setting cannot change that. The generated production workflow trusts the same repository's `main` branch and uses GitHub OIDC—no AWS keys or GitHub deployment Environment.

## Prepare the second account

Use [Prerequisites](../prerequisites.md) for the production account, public DNS zone, short-lived operator credentials and Terraform 1.17.2. Use a production hostname/domain, restrictive ingress CIDRs, and the same GitHub repository as the preview project. GitHub Actions builds the image; no local Docker/Buildx is needed.

Set production values in the shell (do not source preview settings). Clear inherited namespace, PR and OIDC-provider selectors; explicitly choose production auth and capacity:

```sh
export AWS_PROFILE=backstage-production
export BACKSTAGE_AWS_NAME=portal
export BACKSTAGE_AWS_ENVIRONMENT=production
export BACKSTAGE_AWS_AWS_ACCOUNT_ID=PRODUCTION_ACCOUNT_ID
export BACKSTAGE_AWS_AWS_REGION=us-east-1
export BACKSTAGE_AWS_DOMAIN_NAME=YOUR_PRODUCTION_DOMAIN
export BACKSTAGE_AWS_ROUTE53_ZONE_ID=PRODUCTION_ZONE_ID
export BACKSTAGE_AWS_GITHUB_REPOSITORY=OWNER/REPOSITORY
export BACKSTAGE_AWS_OWNER_GITHUB_ID=YOUR_NUMERIC_GITHUB_ID
export BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS=YOUR_PUBLIC_IPV4/32
export BACKSTAGE_AWS_AUTH_MODE=github
export BACKSTAGE_AWS_SPOT=false
unset BACKSTAGE_AWS_PULL_REQUEST BACKSTAGE_AWS_NAMESPACE BACKSTAGE_AWS_HOSTNAME
unset BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN BACKSTAGE_AWS_PROJECT
aws sts get-caller-identity
```

Confirm the account ID and non-root role. If this account already has GitHub's OIDC provider, set its ARN after the `unset`; otherwise bootstrap creates it. Set `BACKSTAGE_AWS_ACCOUNT_BASELINE=false` only when the account's controls are already managed and you have reviewed the change.

## Bootstrap and deploy

Run from the generated project root; do not load `preview.env`.

```sh
backstage-aws bootstrap --approve
backstage-aws base --approve
backstage-aws secrets set-identity --file /secure/oauth.json --approve
```

`/secure/oauth.json`: `clientId` and `clientSecret`. Keep it outside Git. OAuth callback: `https://YOUR_PRODUCTION_HOST/api/auth/github/handler/frame`. Optionally seed with `backstage-aws seed --file /secure/production.dump --approve`.

Bootstrap writes `environments/production.env`; generated workflow files live in `.github/workflows/`, **not** `template/.github/`. Review and commit `production.env` and workflow changes to the existing repository's `main`. That enables deploy-on-push and manual dispatch. The workflow checks out `main`, assumes the role via OIDC, builds/scans the image, and runs `env up --scan --approve`. Protect `main`; the role trust is limited to it. No GitHub Environment approval is configured.

## Operate

```sh
set -a; . environments/production.env; set +a
backstage-aws env status
# In CI; locally requires Docker, or supply an existing --digest.
backstage-aws env up --scan --approve
```

Avoid concurrent local and CI mutations. A healthy ECS/ALB target does not verify login, DNS, migrations or business behavior. `env down` deletes the production namespace database; it is not rollback.

## Capacity and recovery

| Default | Limit |
| --- | --- |
| `db.t4g.micro`, 20 GiB gp3 | Inexpensive baseline, not capacity advice |
| 1-vCPU/2-GiB task; regular Fargate | No redundancy by default |
| Seven-day RDS automated backups | No tested recovery objective |
| $100 monthly budget alert | Not a spend cap |

Assess load, availability, Multi-AZ, task count, networking, retention and RTO/RPO before relying on production. RDS is encrypted, deletion-protected and configured for a final snapshot, but snapshots, keys and recovery are not tested. See [Back up and restore](backup-and-restore.md) before destructive changes.
