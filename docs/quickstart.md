# First preview

This creates billable AWS resources. Start in a dedicated preview account, not production. AWS deployment and recovery have not been live-tested.

## 1. Prepare the account, DNS and GitHub

Complete the [prerequisites](prerequisites.md) first. You need a non-root IAM Identity Center profile, optionally a public Route 53 zone delegated to its assigned name servers (needed only for a domain), GitHub repository access, Docker Engine with Buildx or Podman, and Terraform 1.17.2. The owner must create the account prerequisites before the CLI can provision resources.

Generated GitHub workflows download and verify the matching CLI release. Confirm that release `v0.1.0` exists before deploying:

```sh
gh release view v0.1.0 --repo lprior-repo/backstage-aws
```

This checks that the release is present; the generated workflows verify its attestation. If the release is missing or invalid, stop and ask the maintainer to publish or repair it.

## 2. Install the CLI

```sh
git clone https://github.com/lprior-repo/backstage-aws.git
cd backstage-aws
cargo install --locked --path crates/backstage_aws
```

The repository pins Rust 1.99.1. This installs the CLI from source; it does not replace the published release required by generated workflows.

## 3. Set deployment settings

Log in to GitHub first. Set the values below for your account and the **new Backstage repository**, not this CLI repository. The AWS profile must be configured for the scoped non-root Identity Center user.

```sh
export AWS_PROFILE=backstage-preview
export BACKSTAGE_AWS_NAME=portal
export BACKSTAGE_AWS_AWS_ACCOUNT_ID=YOUR_12_DIGIT_ACCOUNT_ID
export BACKSTAGE_AWS_AWS_REGION=us-east-1
# Optional pair: set both domain settings, or omit both for domain-free mode.
export BACKSTAGE_AWS_DOMAIN_NAME=preview.example.com
export BACKSTAGE_AWS_ROUTE53_ZONE_ID=YOUR_PUBLIC_ZONE_ID
export BACKSTAGE_AWS_GITHUB_REPOSITORY=OWNER/portal
export BACKSTAGE_AWS_OWNER_GITHUB_ID="$(gh api user --jq .id)"
export BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS=YOUR_PUBLIC_IPV4/32
export BACKSTAGE_AWS_BUDGET_EMAIL=you@example.com
export BACKSTAGE_AWS_ALERT_EMAIL=you@example.com
export GITHUB_TOKEN="$(gh auth token)"
```

The two domain settings are optional as a pair: set both or neither. With them, use a public hosted zone for `preview.example.com` and delegate its assigned name servers at the parent DNS provider. Without them (leave both unset), `base` creates no certificate, record or HTTPS listener and the load balancer serves plain HTTP only to the allowed CIDRs, with hostnames `<namespace>.<name>.internal`; use this for evaluation only. Supply at least one reachable IPv4 CIDR (`/8` through `/32`), not `0.0.0.0/0`. The budget email is optional, but without it no monthly budget alert is created. `GITHUB_TOKEN` and `AWS_PROFILE` are credentials/context, not settings to commit.

## 4. Generate the project

```sh
backstage-aws init --output ../portal
cd ../portal
set -a
. environments/preview.env
set +a
```

The generated file contains non-secret settings. Source only this trusted file. Keep `AWS_PROFILE` and `GITHUB_TOKEN` exported in the shell; reload the settings file in any new shell. The output directory must not exist before `init` runs.

## 5. Follow the guided setup

```sh
backstage-aws setup --profile "$AWS_PROFILE"
```

`setup` is read-only and reports one next action at a time. It validates network CIDR syntax first; invalid CIDRs return `INVALID_INPUT` with exit code 2 before discovery. An incomplete guided check returns `SETUP_BLOCKED` with exit code 3; that is expected. Follow `error.details.next`, complete the action, then run the exact argv in `error.details.resume`. The two `--confirm` values are owner attestations: use `organization-email` only after verifying the Organizations management-account email, and `identity-center-mfa` only after confirming the Identity Center policy requires MFA. These are not API-verified facts.

After CIDR validation, the probe order is AWS CLI v2, credentials/profile, target account, root MFA and absence of root access keys, Organizations, the email attestation, IAM Identity Center, the MFA attestation, remaining deployment settings, DNS delegation, generated project directories, and local tools. When complete, `setup` gives `backstage-aws doctor` as the next command. Run it; it checks tools, AWS identity and GitHub repository access.

## 6. Validate locally and approve provisioning

```sh
backstage-aws check
backstage-aws up --approve
```

`check` validates Terraform offline; it does not test AWS. `up --approve` authorizes immediate remote changes and charges, without a confirmation prompt. It runs the first-install sequence: doctor, bootstrap, base, optional seed/sign-in setup, environment deployment and repository publish. It builds the initial image locally, so Docker Engine with Buildx, or Podman (`--container-engine podman`), must work. The default environment is `preview`; the default sign-in mode is guest. Review the settings and [architecture](concepts.md) before approving.

The budget threshold defaults to $100 only when a budget email is configured; it is an alert, not a cap. Shared resources remain billable after preview environments are removed.

## 7. Open a same-repository pull request

After `up` publishes the project to `main`, create a branch and pull request:

```sh
git switch -c test-preview
printf 'Preview acceptance test\n' > preview-check.txt
git add preview-check.txt
git commit -m 'Exercise preview deployment'
git push -u origin test-preview
gh pr create --base main --title 'Test preview' --body 'Exercise deployment.'
```

Watch the repository's Actions run. The workflow uses OIDC, builds the image, clones the seed database, deploys the namespace, waits for ALB health, then comments with the URL. Only same-repository pull requests deploy. Open the URL from an allowed IPv4 network.

Closing the pull request removes that namespace's service, database and Terraform state. It leaves shared AWS resources billable; use the [teardown guide](guides/teardown.md) when removing the whole deployment. See [troubleshooting](troubleshooting.md), [production](guides/production.md), and [backup and restore](guides/backup-and-restore.md) next.
