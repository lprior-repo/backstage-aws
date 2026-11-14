# Agent guide

`backstage-aws` is the only operator interface. It never prompts or reads a TTY. A successful invocation writes one JSON document to stdout; failures use the same one-document envelope. Progress and child-process output go to stderr. Exit codes are 0 success, 2 invalid input, 3 blocked/precondition/approval, 4 external tool/API failure, and 5 remote work continues after timeout. Stop on any nonzero exit or `ok:false` response.

## First install

Run from an operator machine with the CLI installed, Docker with Buildx, Git, Terraform 1.17.2 (the CLI refuses any other version and does not download it), AWS access to the intended account, and a GitHub token in `GITHUB_TOKEN` with repository administration rights. Image builds export a layer cache to the registry, which Docker's default builder cannot do: run `docker buildx create --use` once, or enable the containerd image store. Supply the required `BACKSTAGE_AWS_*` settings listed below; provide a PostgreSQL custom-format dump at `DUMP` for seed initialization. For GitHub sign-in, run `backstage-aws secrets set-identity --file FILE --approve` after `base` and before `env up`; see `docs/SECRETS.md`.

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

Run bootstrap once in each AWS account before that account's base and environment stacks. Configure GitHub environments and OIDC through bootstrap. Previews use a separate AWS account from production. `seed` loads the supplied `pg_dump` custom-format data into the shared seed database; each environment receives a database cloned from it.

## Preview

For a same-repository pull request, set the namespace and create its environment:

```sh
BACKSTAGE_AWS_NAMESPACE=pr-123 backstage-aws env up --approve
```

The preview workflow does this on pull-request open/update and reports the URL and elapsed timings in a PR comment. Closing the PR destroys the environment, drops its database, then removes its state object. Previews use guest sign-in and the configured ingress allowlist. Environments are otherwise production-shaped: shared PostgreSQL, the same environment stack, and a database cloned from seed.

The target is a reachable preview URL within five minutes after a PR push with a warm image cache. No timing has been measured; this remains a budget, not an observed result.

## Commands

`init --output DIR`, `check`, `doctor`, `bootstrap`, `base`, `seed --file DUMP`, `build [--tag TAG]`, `env up [--digest DIGEST] [--scan]`, `env down`, `env list`, `env status`, `secrets set-identity [--file FILE]`, `db rotate-password`, `publish`, and `up [--seed-file DUMP] [--identity-file FILE]`. `--help` gives full options. `check` needs no settings and no credentials. Any AWS, GitHub, or remote repository mutation requires `--approve` (or `BACKSTAGE_AWS_APPROVE=true`); without it the command stops before doing anything, with `APPROVAL_REQUIRED` and exit code 3. `up` with GitHub sign-in needs `--identity-file` on its first run, because the credentials can only be stored after `base` and must exist before the first deployment. `publish` pushes the checked-out commit to `main` on GitHub without renaming local branches. Namespaces `seed`, `postgres`, `template0`, `template1`, `portal`, `vpc-flow`, and any starting with `db-admin` are reserved.
`build`, `env up`, `env down`, `env status`, `env list`, and `check` run in GitHub Actions through OIDC. Operators run `bootstrap`, `base`, `seed`, `db rotate-password`, `secrets set-identity`, and `up` on a workstation with short-lived credentials, never root or long-lived keys.

## Settings and secrets

Required settings are `BACKSTAGE_AWS_NAME`, `BACKSTAGE_AWS_AWS_ACCOUNT_ID`, `BACKSTAGE_AWS_AWS_REGION`, `BACKSTAGE_AWS_DOMAIN_NAME`, `BACKSTAGE_AWS_ROUTE53_ZONE_ID`, `BACKSTAGE_AWS_GITHUB_REPOSITORY`, and `BACKSTAGE_AWS_OWNER_GITHUB_ID`. Optional settings include `BACKSTAGE_AWS_ALERT_EMAIL`, `BACKSTAGE_AWS_BUDGET_EMAIL`, `BACKSTAGE_AWS_ALLOWED_INGRESS_CIDRS`, `BACKSTAGE_AWS_ENVIRONMENT`, `BACKSTAGE_AWS_NAMESPACE`, `BACKSTAGE_AWS_HOSTNAME`, `BACKSTAGE_AWS_AUTH_MODE`, `BACKSTAGE_AWS_MONTHLY_BUDGET_USD`, `BACKSTAGE_AWS_ACCOUNT_BASELINE`, `BACKSTAGE_AWS_SINGLE_NAT_GATEWAY`, `BACKSTAGE_AWS_DATABASE_MULTI_AZ`, `BACKSTAGE_AWS_DESIRED_COUNT`, `BACKSTAGE_AWS_MAX_HIGH_FINDINGS`, `BACKSTAGE_AWS_MAX_CRITICAL_FINDINGS`, `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN`, `BACKSTAGE_AWS_TERRAFORM`, and `BACKSTAGE_AWS_PROJECT`. Set `BACKSTAGE_AWS_GITHUB_OIDC_PROVIDER_ARN` when the account already has a GitHub OIDC provider (an account can hold only one); otherwise `bootstrap` creates it. Terraform state lives in the bucket `<name>-<account>-<region>-tfstate`, which `bootstrap` creates. OAuth credentials are secrets or private files, never command-line values.

The RDS master password is generated and rotated by RDS in Secrets Manager; only the db-admin task role can read it. The application-role password is generated outside Terraform state, stored in Secrets Manager encrypted under the stack KMS key, and injected into containers through ECS `secrets`. `backstage-aws db rotate-password --approve` stages a replacement as `AWSPENDING`, sets it on the role through a db-admin task, makes it `AWSCURRENT`, and restarts every environment service; expect brief connection errors during restarts, and rerun the command after any failure. Database TLS is forced and verified. One application role is shared by all environments in an account; previews are isolated from production by account, not from each other.

## Verification boundary

Offline `check` does not prove live IAM or AWS behavior. No deployment or operation in this project has been run against a real AWS account. Do not claim OIDC, Terraform apply, Fargate, database, TLS, DNS, sign-in, or recovery has been live-verified.
