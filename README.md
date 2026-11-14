# backstage-aws

`backstage-aws` is a Rust CLI that renders and operates a Backstage-on-AWS project. The generated project contains the Backstage app, Terraform, GitHub Actions workflows, and concise operator guidance. Rust handles orchestration; Node is confined to `template/app`.

## Build and render

The checked-in `rust-toolchain.toml` selects the Rust toolchain. Build and render a new project using settings from the environment:

```sh
cargo build --locked --release
BACKSTAGE_AWS_NAME=example-portal \
BACKSTAGE_AWS_AWS_ACCOUNT_ID=123456789012 \
BACKSTAGE_AWS_AWS_REGION=us-east-1 \
BACKSTAGE_AWS_DOMAIN_NAME=preview.example.com \
BACKSTAGE_AWS_ROUTE53_ZONE_ID=Z123456789ABC \
BACKSTAGE_AWS_GITHUB_REPOSITORY=example-owner/example-portal \
BACKSTAGE_AWS_OWNER_GITHUB_ID=123456789 \
  ./target/release/backstage-aws init --output ../backstage-project
```

Replace fixture values with your own. The CLI's `--help` and command-specific help are the source of truth for options.

## Generated system

Three Terraform stacks are used per account. `bootstrap` is applied once per account to configure encrypted remote state and GitHub OIDC/environment; `base` creates shared network, ALB, ECS, ECR, PostgreSQL, secrets, seed storage, and db-admin tasks; `env` creates one namespace's target group, listener rule, log group, task definition, and service. Production is the `production` namespace. Pull-request previews are `pr-<number>` namespaces in an AWS account separate from production.

A PostgreSQL custom-format dump supplied by the operator is restored to the shared `seed` database. Environments clone their own database from that seed and use the same environment stack as production. Previews use guest sign-in behind the configured ingress allowlist; production uses GitHub OAuth.

The preview workflow builds/pushes an image and clones seed data concurrently, applies the `env` stack, then waits for the ECS rollout and for the load balancer to report a healthy target, and reports URL/timings. The target is a reachable URL within five minutes of a PR push with a warm cache. No timing has been measured; this is not an observed service level.

The CLI runs Terraform 1.17.2 from `PATH` (or `--terraform`) and refuses any other version; it does not download Terraform. Image builds use `docker buildx`.

## First install

Configure required `BACKSTAGE_AWS_*` settings and supply a custom-format database dump as `DUMP`. Run this sequence, stopping at any failure:

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

Commands include `init`, `check`, `doctor`, `bootstrap`, `base`, `seed`, `build`, `env up/down/list/status`, `secrets set-identity`, `db rotate-password`, `publish`, and `up`. Any AWS, GitHub, or remote repository mutation requires `--approve` or `BACKSTAGE_AWS_APPROVE=true`. `db rotate-password --approve` changes the shared application role and restarts every environment service in the account; brief connection errors during restart are expected.
`build`, `env up`, `env down`, `env status`, `env list`, and `check` run in GitHub Actions using the account's deploy role via OIDC. Run `bootstrap`, `base`, `seed`, `db rotate-password`, `secrets set-identity`, and `up` from an operator workstation using short-lived credentials; never use root or long-lived AWS keys.

## CLI wire contract

Every command writes exactly one JSON document to stdout: success `{"ok":true,"command":"…","result":{…}}`, failure `{"ok":false,"command":"…","error":{"code":"…","message":"…","retryable":false}}`. Only `--help` and `--version` print plain text. Progress is NDJSON on stderr along with child output. Exit codes are 0 success, 2 invalid input, 3 blocked/precondition/approval, 4 external tool/API failure, and 5 timeout while remote work continues.

## Verification boundary

CI runs Rust checks, initializes a rendered project, and invokes offline `check`. This does not exercise a real AWS account. No real-account OIDC, Terraform apply, Fargate deployment, database/TLS, DNS, sign-in, or recovery has been run or verified, and the generated workflows have never run on GitHub.
