# Backstage on AWS

This project contains the Backstage app, Terraform, and thin GitHub Actions workflows. Operate it with `backstage-aws`; GitHub OIDC supplies short-lived AWS credentials. There are **no GitHub deployment Environments**.

Full walkthrough and operator guides: [backstage-aws documentation](https://github.com/lprior-repo/backstage-aws/tree/main/docs).

## Start in a new shell

From this project directory:

```sh
set -a
. environments/preview.env
set +a
```

Use `production.env` only with credentials for the separate production account. The file is committed non-secret configuration. The CLI itself reads only flags and environment variables.

## Provision once, build in CI

With Terraform 1.17.2 and short-lived operator credentials:

```sh
backstage-aws check
backstage-aws bootstrap --approve
backstage-aws base --approve
# Optional: a sanitized PostgreSQL custom-format dump.
backstage-aws seed --file /secure/path/seed.dump --approve
backstage-aws publish --approve
```

Stop at the first nonzero exit. `base` requires at least one allowed ingress CIDR. Skip `seed` if starting empty. `publish` stages all files: keep dumps and credentials outside this project.

CI downloads the CLI's matching attested release; that release must exist before workflows can run. Open a same-repository PR to build/deploy it. The CLI creates `pr-N`, waits for ECS rollout and a healthy target, and comments the URL. Closing the PR runs cleanup. A cold image build may exceed the unmeasured five-minute target.

## Operate

```sh
backstage-aws env list
backstage-aws env status --pull-request 38
backstage-aws db dump --pull-request 38 --file /secure/path/pr-38.dump --approve
backstage-aws env down --pull-request 38 --approve
```

`env down` destroys the namespace database. Shared infrastructure remains billable. [Recovery](docs/RECOVERY.md) · [Security](docs/SECURITY.md) · [Automation](docs/AUTOMATION.md) · [Secrets](docs/SECRETS.md).

Defaults favor cost: no NAT, `db.t4g.micro`/20-GB gp3, 1-vCPU/2-GB tasks, Spot except production, no Container Insights. Namespaces share a database role: they are not tenant security boundaries.

## Automation contract

One JSON document on stdout; NDJSON progress and child logs on stderr. Remote mutations need `--approve`. Exit codes: 0 success, 2 invalid input, 3 blocked, 4 external failure, 5 remote timeout. `--help` lists all commands and settings.

Only offline checks have been verified. AWS apply, IAM/OIDC, DNS/TLS, database operations, sign-in, recovery, generated deployment workflows, and startup timing require a live sandbox acceptance run.
