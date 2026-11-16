# Prerequisites

Use a dedicated **preview sandbox account**; keep production in a separate account. `setup` is read-only discovery: it does not create an Organization, IAM Identity Center, DNS, credentials or deployment resources. Run it only after generating a project and loading its required settings.

## Account and access

Have the following ready before deployment:

- A 12-digit target AWS account ID and deployment region.
- An AWS Organization with the target account as its management account; the owner must verify that account's management email.
- Root-user MFA enabled and no root access keys. Use a short-lived, non-root role for CLI work.
- IAM Identity Center enabled with exactly one active instance owned by the target account, plus an existing user whose authentication policy requires MFA.
- A reviewed, scoped provisioner permission set assigned to that user. An account owner applies the separate `access` stack below; it does not enable Identity Center.
- Optional: a public Route 53 hosted zone for the deployment domain and its delegated name servers. Without one, set both domain settings empty for domain-free mode (plain HTTP to the ingress allow-list only; add a domain later by re-applying `base`).
- A GitHub token with repository and workflow-file access, plus a configured Git author name and email.

`backstage-aws setup --profile backstage-preview` first validates network CIDRs; invalid values return `INVALID_INPUT` before the guided checks. It then checks the CLI and profile, exact target account, root-account safeguards, Organizations and Identity Center, then asks for two manual attestations. Only after those does it validate remaining settings, DNS delegation, generated project files and local tools. It stops at the first incomplete step and returns `error.details.next` plus `error.details.resume`. The confirmations are human attestations, not AWS API evidence.

## Create the scoped Identity Center permission set

An account owner with an existing short-lived owner session applies this stack once. Use the CLI source checkout's `template/infra/access`; from a generated project use `infra/access`. IAM Identity Center and its user must already exist.

```sh
# From the CLI source checkout:
export ACCESS_DIR=template/infra/access
# From a generated project, use: export ACCESS_DIR=infra/access

export TF_VAR_name=portal
export TF_VAR_aws_account_id=YOUR_12_DIGIT_ACCOUNT_ID
export TF_VAR_aws_region=us-east-1
export TF_VAR_identity_center_region=us-east-1
export TF_VAR_identity_center_instance_arn=YOUR_EXISTING_INSTANCE_ARN
export TF_VAR_principal_id=YOUR_EXISTING_IDENTITY_CENTER_USER_ID
# Only with a domain; set both or neither. Without them the role gets no DNS or certificate permissions.
export TF_VAR_hosted_zone_id=YOUR_PUBLIC_ZONE_ID
export TF_VAR_domain_name=preview.example.com
export TF_VAR_enable_account_controls=true
terraform -chdir="$ACCESS_DIR" init -input=false
terraform -chdir="$ACCESS_DIR" plan -input=false -out=access.tfplan
```

Review the plan, then apply it:

```sh
terraform -chdir="$ACCESS_DIR" apply -input=false access.tfplan
```

The stack creates a one-hour scoped provisioner permission set and a workload boundary; it is not an administrator role. Preserve and back up its owner-managed Terraform state separately. `enable_account_controls=true` temporarily allows the default bootstrap account baseline; after bootstrap, set it to `false`, plan and apply again. If the account is already managed and you will skip that baseline, leave this value `false` and set `BACKSTAGE_AWS_ACCOUNT_BASELINE=false`.

Configure a short-lived AWS profile for the assigned user, then sign in:

```sh
aws configure sso --profile backstage-preview
aws sso login --profile backstage-preview
export AWS_PROFILE=backstage-preview
aws sts get-caller-identity
```

Confirm the caller is the intended account and assumed-role identity, not root. Never store AWS access keys in the repository or GitHub.

## DNS (optional; only with a domain)

If you use a domain, create a **public hosted zone** for your deployment domain in Route 53. Add the zone's assigned `NS` records at the parent DNS provider. Verify the delegated records and the Route 53 zone's nameservers agree:

```sh
export BACKSTAGE_AWS_ROUTE53_ZONE_ID=YOUR_PUBLIC_ZONE_ID
dig +short NS preview.example.com
aws route53 get-hosted-zone --id "$BACKSTAGE_AWS_ROUTE53_ZONE_ID" \
  --query 'DelegationSet.NameServers' --output text
```

`setup` compares the public zone with its delegation before deployment. Certificate validation cannot finish until delegation is correct. Avoid conflicting apex or wildcard records.

## GitHub

The examples use GitHub CLI. Sign in and expose a token to the CLI:

```sh
gh auth login --scopes repo,workflow
gh auth setup-git
export GITHUB_TOKEN="$(gh auth token)"
gh api user --jq .id
```

Use the numeric user ID for `BACKSTAGE_AWS_OWNER_GITHUB_ID`. `up` can create a private repository if it does not exist; an existing repository requires admin access. The token must be able to push workflow files. Organization SSO or token policies may apply. The CLI accepts `GITHUB_TOKEN` or `GH_TOKEN`; do not commit either.

Check for an existing GitHub OIDC provider before bootstrap:

```sh
aws iam list-open-id-connect-providers
```

If `https://token.actions.githubusercontent.com` already exists, set `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN` to its ARN. Otherwise bootstrap creates the provider. No GitHub deployment Environments or environment-scoped variables are required.

## Local tools and versions

| Tool | Version or requirement | Used for |
|---|---|---|
| Rust and Cargo | `rust-toolchain.toml` pins Rust 1.99.1 | Installing the CLI from this checkout |
| Terraform | Exactly 1.17.2 | Terraform-using checks and operations reject other versions |
| AWS CLI | Version 2 | Read-only setup checks and AWS operations |
| Docker Engine with Buildx, or Podman | Docker: Buildx available, daemon reachable. Podman (`--container-engine podman`): `podman info` succeeds (rootless is fine) | Setup/doctor checks and the initial image build in `up` |
| Git | Available; configure `user.name` and `user.email` before the first commit | Setup/doctor check Git; `up` commits and publishes the project |
| `dig` | Available on `PATH` | Checking public DNS delegation |
| Node.js and Yarn | Node 24.22.1; bundled Yarn 4.13.0 | Local app development and `check --app` |
| PostgreSQL tools | PostgreSQL 16 client/server tools | Local development and `check --runtime`; not required for AWS deployment |
| GitHub CLI | Recommended for the examples | Login, token retrieval and pull-request creation |

Docker Buildx or Podman is required even though later pull-request workflows build images in GitHub Actions; `up` builds the first image locally with the engine selected by `--container-engine` / `BACKSTAGE_AWS_CONTAINER_ENGINE` (`docker` by default). Podman builds and pushes without Buildx's registry layer cache. `BACKSTAGE_AWS_TERRAFORM` may point to a specific executable, but it must still be Terraform 1.17.2.

The generated workflows require the CLI release matching this source version and verify its attestation. Check release `v0.1.0` in `lprior-repo/backstage-aws` before deploying. GitHub Actions pins Terraform 1.17.2 and Node 24.22.1; the app pins Yarn 4.13.0 in `package.json` and `provenance/versions.json`.

## Costs

Defaults avoid NAT gateways, use a shared `db.t4g.micro` database, 20-GB gp3 storage, one Fargate task per environment, and Spot outside production. This is a low-cost, non-HA design; ALB, RDS, public IPv4, storage, logs, secrets and registry remain billable even with no preview environments. Check the [AWS Pricing Calculator](https://calculator.aws/) for the target region and workload.

The default monthly budget threshold is $100, but no budget is created unless `BACKSTAGE_AWS_BUDGET_EMAIL` is set. A budget is an alert, not a cap. [Teardown](guides/teardown.md) · [Quickstart](quickstart.md).
