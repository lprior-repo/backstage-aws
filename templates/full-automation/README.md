# Backstage on AWS — full deployment automation

This repository contains the Backstage application **and** its AWS deployment implementation. It is not the planning Site. The implementation uses Terraform, GitHub Actions, JSONata-only AWS Step Functions and ECS on Fargate. No CloudFormation, Control Tower, landing zone or CodeBuild is used.

The bundled application is pinned to Backstage **1.56.5**. A retained upstream GitHub release response records it as latest stable at preparation time; `./platform latest` checks again instead of silently deploying a moving `latest` tag.

## What is implemented

- New-account baseline: root-MFA preflight, account S3 public-access blocking, password policy, multi-region CloudTrail with validated logs, IAM Access Analyzer and budget notifications.
- Encrypted/versioned S3 Terraform state with native locking; bootstrap state migrates to a separate remote key.
- A two-AZ VPC, public HTTPS ALB with an explicit ingress allowlist, private Fargate tasks, isolated RDS PostgreSQL, NAT egress, flow logs and ALB access logs.
- GitHub OIDC roles: a narrow ECR image builder and a separate **privileged infrastructure** role bound to reviewed GitHub environments. No stored AWS access keys.
- Multi-stage app image build, existing app tests, immutable ECR digests and a fail-closed image-scan gate.
- JSONata rollout and maintenance workflows: fixed task definitions, bounded polling/retries, exact deployment-ID checks and a conditional DynamoDB release lock.
- Secrets Manager setup, least-privilege application credential bootstrap, ECS injection, OAuth/plugin-secret upload, identity rotation and explicit database-password maintenance.
- Pre-release DB snapshots; reviewed image rollback; isolated DB restore drill; retained workflow/task versions and release evidence.
- CloudWatch dashboards, alarms and SNS notifications; live HTTP, ECS, ALB, RDS and notification checks with explicit incomplete states.

No AWS deployment or security result is claimed by shipping the source. See [docs/VALIDATION.md](docs/VALIDATION.md) for the actual local checks and environment restrictions.

## Start with a new account

Use an application account, not the AWS Organizations management account. Account creation, root MFA, DNS delegation, GitHub OAuth registration and notification confirmation require your real identities; the automation cannot invent these.

Prerequisites: Linux or macOS, Node **24.22.1**, Terraform **1.17.2**, AWS CLI v2, Docker, `jq`; optional GitHub CLI `gh` for repository setup. Use AWS SSO or short-lived assumed credentials in the intended account. Docker needs enough memory for the Backstage build. Native Node build dependencies are installed only inside the build image; there is no first-party Python code or Python in the application runtime.

```bash
cp deployment.example.json deployment.json
# Edit deployment.json: real account, hostname, public Route53 zone,
# allowed VPN/corporate CIDRs, owner GitHub numeric ID, repository and emails.

aws sso login --profile YOUR_PROFILE
export AWS_PROFILE=YOUR_PROFILE
./platform configure
./platform doctor
./platform secrets-init --file .secrets/github-identity.json
```

Register a GitHub OAuth App with homepage `https://YOUR_HOST` and callback `https://YOUR_HOST/api/auth/github/handler/frame`. Fill its real `clientId` and `clientSecret` into the generated private identity file. Keep the generated session secret. Do not commit credentials.

```bash
./platform up --identity .secrets/github-identity.json
```

`up` generates Terraform inputs, provisions the account/state/network/runtime, uploads identity credentials, builds and scans the app, registers the digest-pinned tasks, starts the JSONata rollout, and records live-check results. Terraform provisioning prompts for plan approval. The release phase shows a saved plan and requires `yes` before applying it locally. Initial services stay at zero tasks until the DB bootstrap has succeeded. Infrastructure creation is billable even before the portal starts.

The first production release is expected to remain **needs verification** until you verify real login, authorization and recovery. `blocked` means an actual check failed; it is not a successful deployment. Confirm the SNS email subscription and run tests from a network allowed through the ALB.

## Daily commands

```bash
./platform release                         # build, scan, snapshot, apply, deploy, check
./platform release --digest sha256:...     # promote an existing image, without rebuilding
./platform rollout                         # re-run the currently approved fixed revision
./platform rollback --digest sha256:... --database-compatible
./platform secrets-rotate --identity .secrets/github-identity.json
./platform db-rotate --maintenance-window  # deliberate downtime; drain, rotate, restart
./platform validate-live
./platform validate-workflow               # local JSONata contract + AWS ASL validation
./platform snapshot
./platform latest
./platform upgrade                         # prepare/test a clean Git upgrade branch
./platform help
```

A snapshot does not prove that an older binary understands a newer DB schema. `--database-compatible` is an explicit operator acknowledgement after reviewing migrations. No automatic DB downgrade is attempted.

## Enable GitHub deployment

After the first local setup:

```bash
./platform export-target
./platform github-setup
# Commit reviewed source to your repository; generated configuration/secrets/state
# are ignored. github-setup stores only nonsecret configuration as repo variables.
gh workflow run deploy.yml --repo YOUR_ORG/YOUR_REPO
```

The workflow builds with an ECR-only role, then deploys through a separate reviewer-protected infrastructure environment. Both environments allow only the `main` branch. The configured owner may approve their own manual workflow run; use a separate reviewer for team separation of duties. GitHub repository/plan support for environment protections is required; setup fails if enforcement is unavailable.

Review the infrastructure role in `infra/bootstrap/github.tf`: it has broad service control-plane permissions needed for Terraform inside this dedicated account. It is **not** a low-privilege application role or a security sandbox. Bootstrap state/CI roles remain outside its IAM mutation scope. Its trusted repository and approval environment are security boundaries.

The workflow is manually dispatched, serializes deployments without cancelling an active run and uploads nonsecret evidence. Do not run independent Terraform writers while a release/maintenance execution is active. The DynamoDB mutex serializes state-machine service operations, not every possible administrator action.

## Layout

| Path | Contents |
| --- | --- |
| `app/` | Full Backstage source, lockfile, Dockerfile, GitHub identity, default-deny permissions, DB bootstrap |
| `infra/bootstrap/` | State, account baseline and GitHub OIDC roles |
| `infra/core/` | Networking, IAM, RDS, secrets, ECS, JSONata workflows, observability |
| `automation/` | Operator CLI, config validation, image gate, release and evidence handling |
| `.github/workflows/` | Source validation and protected deployment pipelines |
| `tools/portalctl/` | Optional inherited Rust helper; not needed for deployment |
| `docs/` | Automation, secrets, recovery, security, design and validation |

See [docs/AUTOMATION.md](docs/AUTOMATION.md), [docs/SECRETS.md](docs/SECRETS.md), [docs/RECOVERY.md](docs/RECOVERY.md) and [docs/CHOICES.md](docs/CHOICES.md).
