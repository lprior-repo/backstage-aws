# Agent guide

Use the Rust `backstage-aws` binary as the operator interface; do not bypass it with direct AWS or Terraform mutation. It does not prompt or read a TTY. Each call writes exactly one JSON document to stdout; progress and child output go to stderr. Exit codes: 0 success, 2 invalid input, 3 blocked/precondition/approval, 4 external tool/API failure, 5 timeout while remote work continues. Treat any nonzero exit or `ok:false` as failure.

## First install

Configure required `BACKSTAGE_AWS_*` values and provide an operator-supplied PostgreSQL custom-format dump at `DUMP`. Run in order and stop on first failure:

```sh
backstage-aws init --output ./backstage-project
cd ./backstage-project
backstage-aws check
backstage-aws doctor
backstage-aws bootstrap --approve
backstage-aws base --approve
backstage-aws seed --file DUMP --approve
backstage-aws publish --approve
backstage-aws env up --approve
```

Bootstrap is a one-time operation per AWS account. It establishes encrypted Terraform state and the GitHub environment/OIDC role. Apply `base` once per account, then seed the shared `seed` database. Production uses namespace `production`; pull-request previews use `pr-<number>` in a separate AWS account.

## Preview

For a same-repository pull request, set the PR namespace and deploy:

```sh
BACKSTAGE_AWS_NAMESPACE=pr-123 backstage-aws env up --approve
```

The PR workflow comments with the URL and CLI timing table; close triggers environment teardown. Preview sign-in is guest access within the configured network allowlist. Each environment clones its database from the supplied seed. The goal is a reachable URL under five minutes after a PR push with a warm cache; no timings have been measured.

## Commands and settings

Commands: `init`, `check`, `doctor`, `bootstrap`, `base`, `seed --file DUMP`, `build`, `env up`, `env down`, `env list`, `env status`, `secrets set-identity`, `db rotate-password`, `publish`, and `up`. Consult `backstage-aws --help` for options. `check` needs no settings and no credentials. Every mutation of AWS, GitHub, or the remote repository requires `--approve` (or `BACKSTAGE_AWS_APPROVE=true`); without it the command stops before doing anything. Terraform 1.17.2 must be on `PATH` or passed with `--terraform`; the CLI does not download it.
`build`, `env up`, `env down`, `env status`, `env list`, and `check` run in GitHub Actions via OIDC. Run `bootstrap`, `base`, `seed`, `db rotate-password`, `secrets set-identity`, and `up` on an operator workstation with short-lived credentials, never root or long-lived keys.

Required environment settings: `BACKSTAGE_AWS_NAME`, `BACKSTAGE_AWS_AWS_ACCOUNT_ID`, `BACKSTAGE_AWS_AWS_REGION`, `BACKSTAGE_AWS_DOMAIN_NAME`, `BACKSTAGE_AWS_ROUTE53_ZONE_ID`, `BACKSTAGE_AWS_GITHUB_REPOSITORY`, and `BACKSTAGE_AWS_OWNER_GITHUB_ID`. Optional settings include alert/budget email, ingress CIDRs, environment, namespace, hostname, auth mode, monthly budget, account baseline, NAT/DB settings, desired count, scan thresholds, state bucket/KMS, Terraform path, and project path. Secret values belong in environment secrets or private files, not argv or Terraform inputs.

The RDS master password is generated and rotated by RDS in Secrets Manager, readable only by the db-admin task role. The application-role password stays out of Terraform state, is stored in Secrets Manager under the stack KMS key, and is injected through ECS `secrets`. `backstage-aws db rotate-password --approve` changes the shared application role through a db-admin task and restarts every environment's service. Expect brief connection errors during restart. Previews do not share an account with production, but namespaces within one account share the application role. Database TLS is forced and verified.

## Verification boundary

This repository's CI checks Rust, renders the template, and runs offline Terraform checks. No part of this design has been run against a real AWS account. Do not claim that OIDC, apply, Fargate, database, TLS/DNS, sign-in, or recovery has been live-verified.
