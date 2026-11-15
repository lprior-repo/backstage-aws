# Prerequisites

Start with a dedicated **preview sandbox**. Production belongs in a second account.

Start discovery: `backstage-aws setup --profile backstage-preview`. No deployment settings required. It changes nothing; follow `error.details.next`, then rerun `error.details.resume` (argv).

| Account step | Automation boundary |
|---|---|
| Organizations, users, permissions, infrastructure | APIs; approval required for changes |
| Organization Identity Center; MFA enforcement | Console setup |
| Email verification, password, MFA enrollment, login | Human |

Only after completing the console checks, pass `--confirm organization-email --confirm identity-center-mfa`. These record attestations, not API verification. Organization creation can change Free Tier billing and expire credits: [AWS guidance](https://docs.aws.amazon.com/singlesignon/latest/userguide/enable-identity-center.html).

| Required | Check |
|---|---|
| Short-lived, non-root AWS assumed-role session | `aws sts get-caller-identity` |
| Public Route 53 zone in that account | Zone ID; delegated name servers |
| GitHub token and repository push access | `gh auth status` |
| Terraform **1.17.2**, Git, AWS CLI v2, GitHub CLI | `terraform version` |
| Allowed public IPv4 CIDR | Your address `/32`, or office/VPN range |
| Published, attested CLI release for generated CI | `gh release view v0.1.0 --repo lprior-repo/backstage-aws` |

## AWS identity

First access: an owner applies [`infra/access`](../template/infra/access/) with Terraform 1.17.2 using an existing short-lived owner session. It creates a one-hour scoped Identity Center provisioner and an owner-controlled workload boundary; no access keys or administrator policy. IAM Identity Center and its user must already exist.

```sh
export TF_VAR_name=portal
export TF_VAR_aws_account_id=YOUR_12_DIGIT_ACCOUNT_ID
export TF_VAR_aws_region=us-east-1
export TF_VAR_identity_center_region=us-east-1
export TF_VAR_identity_center_instance_arn=YOUR_EXISTING_INSTANCE_ARN
export TF_VAR_principal_id=YOUR_EXISTING_USER_ID
terraform -chdir=template/infra/access init -input=false
terraform -chdir=template/infra/access plan -input=false -out=access.tfplan
terraform -chdir=template/infra/access apply -input=false access.tfplan
```

From a generated project, use `infra/access` instead of `template/infra/access`. Review the plan before applying. Preserve this owner-managed stack's state and keep it outside the scoped provisioner's control. Review its explicit regional/account API exceptions. Leave `hosted_zone_id` and `domain_name` empty until DNS exists; when known, supply both through `TF_VAR_…` variables and reapply. Set `TF_VAR_enable_account_controls=true` only for initial account hardening/audit-baseline writes, then set it to `false` and reapply after bootstrap.

```sh
aws configure sso --profile backstage-preview
aws sso login --profile backstage-preview
export AWS_PROFILE=backstage-preview
aws sts get-caller-identity
```

Confirm account ID and assumed-role ARN. Never use root or store AWS keys in GitHub.

Bootstrap optionally creates account-wide CloudTrail, Access Analyzer, password policy and S3 public-access block. In an already-managed account, review Terraform and set `BACKSTAGE_AWS_ACCOUNT_BASELINE=false`.

Check existing providers with `aws iam list-open-id-connect-providers`. If GitHub's provider exists, export its ARN as `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN`; otherwise bootstrap creates it.

## DNS

Create a **public hosted zone** for `preview.example.com` in Route 53. Copy its ID and four name servers. Add an `NS` record for `preview` at the parent DNS provider, then check:

```sh
dig +short NS preview.example.com
```

Servers must match before certificate validation can finish. Avoid domains with conflicting apex/wildcard records.

## GitHub

```sh
gh auth login --scopes repo,workflow
gh auth setup-git
export GITHUB_TOKEN="$(gh auth token)"
gh api user --jq .id
```

Save the numeric ID for the initial Backstage catalog user. `publish` creates a private repository if absent; existing repositories require admin access. Git must push workflow files. Organization SSO/token policy may apply.

No GitHub deployment Environments or environment-scoped variables are required. Private-repository Actions usage may incur charges.

## Local tools

Install [Terraform 1.17.2](https://developer.hashicorp.com/terraform/install), Git, AWS CLI and `gh`. Set Git's author name/email. The CLI rejects other Terraform versions; `BACKSTAGE_AWS_TERRAFORM` can point to the executable.

For source development, `rust-toolchain.toml` includes matching `rust-analyzer` and `rust-src` components for OMP/editor navigation. After repairing an existing installation with `rustup component add rust-analyzer rust-src`, reload OMP's LSP servers.

Docker/Buildx is needed for local builds, `setup`, `doctor`, and `up`; discovery also checks daemon access. Podman is not supported.

## Costs

**[INFERENCE] Unmeasured us-east-1 planning range:** $40–60/month shared infrastructure, plus $10–15/month per continuously running Spot preview or $35–45 on-demand. Traffic, logs, credits, storage and region change the bill. [Calculate your workload](https://calculator.aws/).

The ALB's [non-exportable public ACM certificate is free](https://aws.amazon.com/certificate-manager/pricing/), including renewal. ALB, public IPv4, RDS, KMS, secrets, DNS, logs, backups, registry and runners are not.

The $100 budget is an **alert, not a cap**. Shared resources keep billing with zero previews. [Teardown](guides/teardown.md).

Next: [Quickstart](quickstart.md).

