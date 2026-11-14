# Deploy your first PR

**Requirements:** [sandbox account, delegated DNS, GitHub access, Terraform 1.17.2](prerequisites.md).

**Status:** local/offline checks pass. AWS deployment and recovery remain untested. Run this in a billable sandbox first. Images build in GitHub Actions; no local Docker needed.

## 1. Install

```sh
git clone https://github.com/lprior-repo/backstage-aws.git
cd backstage-aws
cargo install --locked --path crates/backstage_aws
gh release view v0.1.0 --repo lprior-repo/backstage-aws
```

**Release gate:** generated CI downloads and verifies `v0.1.0`. If unpublished, stop before provisioning: a maintainer must publish the matching attested release. Source installation alone does not make CI usable.

## 2. Configure

Replace every uppercase placeholder. The repository is your **new Backstage project**, not this CLI repository.

```sh
export AWS_PROFILE=backstage-preview
export BACKSTAGE_AWS_NAME=portal
export BACKSTAGE_AWS_AWS_ACCOUNT_ID=YOUR_12_DIGIT_ACCOUNT_ID
export BACKSTAGE_AWS_AWS_REGION=us-east-1
export BACKSTAGE_AWS_DOMAIN_NAME=preview.YOUR_DOMAIN
export BACKSTAGE_AWS_ROUTE53_ZONE_ID=YOUR_ZONE_ID
export BACKSTAGE_AWS_GITHUB_REPOSITORY=OWNER/REPOSITORY
export BACKSTAGE_AWS_OWNER_GITHUB_ID="$(gh api user --jq .id)"
export BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS=YOUR_PUBLIC_IPV4/32
export BACKSTAGE_AWS_BUDGET_EMAIL=you@YOUR_DOMAIN
export BACKSTAGE_AWS_ALERT_EMAIL=you@YOUR_DOMAIN
export GITHUB_TOKEN="$(gh auth token)"
```

Zone ID: no `/hostedzone/` prefix. CIDRs: comma-separated, `/8`–`/32`; no `0.0.0.0/0`. Reuse an existing OIDC provider via `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN`.

## 3. Generate and check

```sh
backstage-aws init --output ../portal
cd ../portal
set -a
. environments/preview.env
set +a
backstage-aws check
aws sts get-caller-identity
```

Confirm the AWS account and non-root role. `check` is offline Terraform validation, not an AWS test. Stop on every nonzero exit.

Commit `environments/preview.env`: non-secret configuration loaded by shell/CI, not the CLI. Source only trusted files. Reload it in new shells.

## 4. Provision

```sh
backstage-aws bootstrap --approve
backstage-aws base --approve
# Optional: sanitized PostgreSQL custom-format dump.
backstage-aws seed --file /secure/path/sanitized.dump --approve
```

Bootstrap creates state and GitHub OIDC trust, then rewrites the settings file. Base creates shared infrastructure and initializes an empty seed. Skip `seed` to start empty. [Seed guide](guides/seed-database.md).

Initial RDS/DNS provisioning can take tens of minutes; the preview target excludes it. Preserve local bootstrap state after failures. Confirm any SNS subscription email.

## 5. Publish

```sh
backstage-aws publish --approve
```

Creates/reuses the repository, commits all project files and pushes `main`. Keep dumps/credentials outside the project. Production deployment skips until `environments/production.env` exists.

## 6. Open a PR

```sh
git switch -c test-preview
printf 'Preview acceptance test\n' > preview-check.txt
git add preview-check.txt
git commit -m 'Exercise preview deployment'
git push -u origin test-preview
gh pr create --base main --title 'Test preview' --body 'Exercise deployment.'
```

Watch **Actions → Preview environment**. OIDC → image build/database clone → Terraform → ECS/ALB readiness → CLI URL comment.

Open `https://pr-N.preview.YOUR_DOMAIN` from an allowed network. Sign in as guest; check expected catalog/seed data. First builds are cold: under five minutes is an unmeasured target.

## 7. Prove cleanup

Push another change: the hostname/database should persist and the PR comment update. Close the PR with `gh pr close`; watch the `down` job. Confirm removal with `backstage-aws env list`.

Cleanup deletes that namespace's database. Shared AWS resources remain billable.

**Next:** [Production](guides/production.md) · [Backups](guides/backup-and-restore.md) · [Teardown](guides/teardown.md) · [Troubleshooting](troubleshooting.md).

