# Production

Use a separate AWS account and `production` environment. Production does not use Spot by default. Generated production deploys use GitHub OIDC and the `<name>-github-deploy` role, whose trust is limited to the configured repository's `refs/heads/main`. No GitHub Deployment Environment or long-lived AWS key is required.

Every successful `backstage-aws` command exits 0 and prints one JSON document. Exit 2 means invalid input, 3 blocked/precondition, 4 external or state failure, and 5 timeout; remote work may continue after exit 5.

## Configure the account

1. In the generated project root, set production values and short-lived credentials. Use Terraform 1.17.2, a real production domain with its Route 53 zone (HTTPS; domain-free mode is plain HTTP and is for sandboxes only), and ingress CIDRs that match the intended users:

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
   export BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS=YOUR_ALLOWED_IPV4_CIDR/32
   export BACKSTAGE_AWS_AUTH_MODE=github
   export BACKSTAGE_AWS_SPOT=false
   unset BACKSTAGE_AWS_PULL_REQUEST BACKSTAGE_AWS_NAMESPACE BACKSTAGE_AWS_HOSTNAME
   unset BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN BACKSTAGE_AWS_PROJECT
   aws sts get-caller-identity
   ```

   Expected: the AWS command exits 0 and reports the production account and a non-root operator role. If the account already has the GitHub OIDC provider, set `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN` to its ARN after the `unset`; otherwise bootstrap creates it. Set `BACKSTAGE_AWS_ACCOUNT_BASELINE=false` only when account controls are already managed and reviewed.

## Bootstrap, identity, and publish

2. Bootstrap the state/backend and deploy role, then create the shared base:

   ```sh
   backstage-aws bootstrap --approve
   backstage-aws base --approve
   ```

   Expected: both exit 0. `bootstrap` JSON has `result.state_migrated`, `result.settings_file`, and `result.deploy_role_arn`; `base` JSON has `result.outputs`. Bootstrap writes `environments/production.env`. Do not load preview settings for these operations.
3. Store the GitHub OAuth app credentials from a private file outside Git:

   ```sh
   set -a; . environments/production.env; set +a
   chmod 600 /secure/oauth.json
   backstage-aws secrets set-identity --file /secure/oauth.json --approve
   ```

   The JSON keys are `clientId` and `clientSecret`. Expected: exit 0 and JSON `result.secret_arn` and `result.version_id`; secret values are not printed. The configured callback is `https://YOUR_PRODUCTION_HOST/api/auth/github/handler/frame`. This command writes a Secrets Manager version but does not restart running tasks.
4. Review the full checkout, including `environments/production.env` and generated `.github/workflows/`, then publish to enable the main-branch production workflow:

   ```sh
   git status --short
   set -a; . environments/production.env; set +a
   GITHUB_TOKEN="$(gh auth token)" backstage-aws publish --approve
   ```

   Expected: `git status` exits 0; `publish` exits 0 and returns JSON `result.repository`, `result.created`, and `result.commit`. `publish` stages all checkout changes, commits if dirty, and pushes the checked-out `HEAD` to `main`; it requires repository-admin token access. If main is protected, publish through the normal reviewed branch/PR process instead. The generated production workflow runs `env up --scan --approve` on push to main or manual dispatch; manual dispatch accepts an optional image digest.

## Operate and recover

5. Load production settings and inspect or deploy:

   ```sh
   set -a; . environments/production.env; set +a
   backstage-aws env status
   backstage-aws env up --scan --approve
   ```

   Expected: `env status` exits 0 with JSON `result.namespace`, `result.url`, `result.service_status`, `result.running_count`, `result.image_digest`, and `result.ready`. `env up --scan` exits 0 with `result.namespace`, `result.url`, `result.image_digest`, `result.timings`, and `result.comment_url`. Locally, an image build needs Docker/Buildx or Podman; a previously built image can be selected with `--digest sha256:<digest>`. The root `up` command does not request the production scan, so use the scanned workflow/command for production deploys.
6. Before a destructive change, export production data to a new local file:

   ```sh
   set -a; . environments/production.env; set +a
   install -d -m 700 /secure
   backup_dir="$(mktemp -d /secure/backstage-aws-dump.XXXXXX)"
   backstage-aws db dump --file "$backup_dir/production.dump" --approve
   ```

   Expected: exit 0 and JSON `result.bucket`, `result.key`, `result.database`, and `result.file`. This is a custom-format dump of the production namespace without owner or ACL entries; it is on-demand, not continuous. To use it as the seed for future namespaces, run:

   ```sh
   set -a; . environments/production.env; set +a
   backstage-aws seed --file "$backup_dir/production.dump" --approve
   ```

   Expected: exit 0 with JSON `result.bucket`, `result.key`, `result.operation_id`, and `result.resumed`. It replaces the shared seed for future namespaces only; it does not restore production or any existing namespace.
7. RDS has encrypted storage, deletion protection, seven-day automated backups, and a final-snapshot request; state is versioned in a separate encrypted bucket. Preserve the state bucket, both KMS keys, snapshots, and secrets. The CLI has no RDS restore/cutover, state restore, or secret export/import command. A healthy ECS/ALB target does not verify DNS, sign-in, or application behavior. See [Back up and restore](backup-and-restore.md) before relying on recovery.
