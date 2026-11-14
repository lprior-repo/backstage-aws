# Backstage on AWS

This template combines the Backstage application, three Terraform stacks, and the Rust `backstage-aws` operator CLI. GitHub Actions uses short-lived OIDC credentials; no long-lived AWS keys belong in GitHub. Run `backstage-aws --help` for command details.

## Infrastructure and first install

There are three stacks per AWS account: `bootstrap` provisions remote state, KMS, GitHub OIDC trust, the deploy role, GitHub environment, and Actions variables once per account; `base` provisions shared networking, load balancing, ECS, ECR, PostgreSQL, every secret, and the seed database; `env` provisions one namespace's target group, listener rule, log group, task definition, and service. Production uses namespace `production`. Preview namespaces are `pr-<number>` in a separate AWS account.

Install the attested `backstage-aws` release binary, Terraform 1.17.2 (the CLI refuses any other version and does not download it), Docker with Buildx, and Git, then configure the required `BACKSTAGE_AWS_*` settings. Provide a PostgreSQL custom-format dump for seed data. Run this exact sequence, stopping at the first failure:

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

Bootstrap must be applied once in each account before `base` or `env`. `seed` uploads the supplied dump and restores it into the shared `seed` database. Each environment gets a database cloned from that seed. Previews use guest sign-in, with network access limited by the configured ALB allowlist; production uses GitHub sign-in.

## Pull-request previews

The preview workflow handles same-repository pull requests: it creates `pr-<number>` on open/update and removes it when the request closes. The CLI builds/pushes the image and clones the seed database in parallel, applies the fast `env` stack, then waits for the ECS rollout to complete and for the load balancer to report a healthy target. A rollout that the deployment circuit breaker rolls back is reported as a failure. The workflow comments with the URL and timings.

The goal is a reachable preview URL in under five minutes from a PR push with a warm image cache. No timing has been measured, so this is a target rather than a proven service level. Preview and production accounts are separate; previews within their account share the database instance and application role.

## Commands and approval

Available commands: `init`, `check`, `doctor`, `bootstrap`, `base`, `seed --file DUMP`, `build`, `env up`, `env down`, `env list`, `env status`, `secrets set-identity`, `db rotate-password`, `publish`, and `up`. Use command `--help` for options. `check` needs no settings and no credentials. Any action changing AWS, GitHub, or the remote repository requires `--approve` (or `BACKSTAGE_AWS_APPROVE=true`); without it the command stops before doing anything, with `APPROVAL_REQUIRED` and exit code 3.
GitHub Actions runs `build`, `env up`, `env down`, `env status`, `env list`, and `check` with OIDC. Operators run `bootstrap`, `base`, `seed`, `db rotate-password`, `secrets set-identity`, and `up` from a workstation with short-lived credentials; never use root or long-lived AWS keys.

`db rotate-password --approve` stages a new application-role password in Secrets Manager outside Terraform state, sets it on the PostgreSQL role through a db-admin task, makes it current, then restarts each environment service; brief connection errors during restarts are expected. The RDS master password is generated, stored, and rotated by RDS in Secrets Manager and readable only by the db-admin task role. ECS injects the application password from Secrets Manager through ECS `secrets`; database TLS is forced and verified.

Settings use `BACKSTAGE_AWS_` plus upper snake case. Required values: `BACKSTAGE_AWS_NAME`, `BACKSTAGE_AWS_AWS_ACCOUNT_ID`, `BACKSTAGE_AWS_AWS_REGION`, `BACKSTAGE_AWS_DOMAIN_NAME`, `BACKSTAGE_AWS_ROUTE53_ZONE_ID`, `BACKSTAGE_AWS_GITHUB_REPOSITORY`, and `BACKSTAGE_AWS_OWNER_GITHUB_ID`. Optional values include ingress CIDRs, alert/budget email, environment, namespace, hostname, auth mode, account baseline, NAT/DB options, desired count, scan thresholds, and the ARN of an existing GitHub OIDC provider. OAuth credentials must be provided through environment secrets or private files, never flags.

## CLI output

Every command writes exactly one JSON document to stdout. Success is `{"ok":true,"command":"…","result":{…}}`; failure is `{"ok":false,"command":"…","error":{"code":"…","message":"…","retryable":false}}`. Only `--help` and `--version` print plain text. NDJSON progress events and raw child output go to stderr. Exit codes are 0 success, 2 invalid input, 3 blocked/precondition/approval, 4 external tool/API failure, and 5 timeout while remote work continues. Do not infer success from human-formatted logs.

## Verification limits

Offline checks are not live AWS evidence. No part of this design has been run against a real AWS account. IAM/OIDC trust, Terraform apply, Fargate startup, database connectivity, TLS, DNS/certificates, GitHub sign-in, and recovery remain unverified until exercised in a dedicated account.
