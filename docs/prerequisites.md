# Prerequisites

Start with a dedicated **preview sandbox**. Production belongs in a second account.

| Required | Check |
|---|---|
| Short-lived, non-root AWS administrator session | `aws sts get-caller-identity` |
| Public Route 53 zone in that account | Zone ID; delegated name servers |
| GitHub token and repository push access | `gh auth status` |
| Terraform **1.17.2**, Git, AWS CLI v2, GitHub CLI | `terraform version` |
| Allowed public IPv4 CIDR | Your address `/32`, or office/VPN range |
| Published, attested CLI release for generated CI | `gh release view v0.1.0 --repo lprior-repo/backstage-aws` |

## AWS identity

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

Docker/Buildx is needed only for local builds, `doctor`, and `up`; the walkthrough builds in CI. Podman is not supported.

## Costs

**[INFERENCE] Unmeasured us-east-1 planning range:** $40–60/month shared infrastructure, plus $10–15/month per continuously running Spot preview or $35–45 on-demand. Traffic, logs, credits, storage and region change the bill. [Calculate your workload](https://calculator.aws/).

The ALB's [non-exportable public ACM certificate is free](https://aws.amazon.com/certificate-manager/pricing/), including renewal. ALB, public IPv4, RDS, KMS, secrets, DNS, logs, backups, registry and runners are not.

The $100 budget is an **alert, not a cap**. Shared resources keep billing with zero previews. [Teardown](guides/teardown.md).

Next: [Quickstart](quickstart.md).

