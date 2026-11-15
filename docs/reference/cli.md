# CLI reference

`backstage-aws` is non-interactive. Settings, defaults and validation: [settings](settings.md). Stdout/errors: [output contract](output-and-errors.md). Global options work before or after any subcommand; flags override environment variables.

## Commands

`setup` discovers prerequisites without deployment settings; `check` runs offline. Other commands validate deployment settings. Terraform 1.17.2 is required except for `init`, `setup` and `doctor` (discovery checks it when reached).

| Command | Options | Approval | Result |
|---|---|---:|---|
| `setup` | `--aws-cli PATH`, `--profile NAME`, repeated `--confirm CHECK` | No | Read-only ordered discovery; one next action and resumable argv. Confirmations: `organization-email`, `identity-center-mfa`. Ready means run `doctor`, not deployed. |
| `init --output DIR` | required | No | Render project; destination must be new. Writes `environments/<environment>.env`. |
| `check [--app]` | `--app` optional | No | Offline Terraform validation/tests for access, bootstrap, base and env; with `--app`, also installs, typechecks, tests, and builds the Backstage app. No AWS credentials. |
| `doctor` | — | No | Check Terraform, Docker Buildx, Git, AWS identity/account, GitHub admin access. No changes. |
| `bootstrap` | — | Yes | Configure Terraform state, GitHub OIDC deploy role and optional account baseline; write env settings. Repeatable. |
| `base` | — | Yes | Create/update shared network, load balancer, database, registry, secrets; requires ingress CIDR. Repeatable. |
| `seed --file DUMP` | required | Yes | Load custom-format PostgreSQL dump as shared seed database. Accepts `db dump --file` output. |
| `build` | `--tag TAG` optional | Yes | Build and push image; default tag is namespace + current time. |
| `env up` | `--digest DIGEST`, `--scan` optional | Yes | Build unless digest supplied; copy seed for new namespace, deploy, wait for healthy target. Scan enforces finding limits. With PR number and `GITHUB_TOKEN`/`GH_TOKEN`, create/update PR comment. Repeatable. |
| `env down` | — | Yes | Remove namespace service, database, Terraform state. Repeatable after interruption. |
| `env list` | — | No | List account namespaces. |
| `env status` | — | No | Report namespace address, image, readiness. |
| `secrets set-identity` | `--file FILE` optional | Yes | Store OAuth JSON `{"clientId":"…","clientSecret":"…"}`; otherwise read `BACKSTAGE_AWS_OAUTH_CLIENT_ID` and `BACKSTAGE_AWS_OAUTH_CLIENT_SECRET`. Values not printed. |
| `db rotate-password` | — | Yes | Rotate application DB password; restart every environment (brief connection errors possible). Never prints/logs/passes password as argument. |
| `db dump` | `--file FILE` optional | Yes | Run VPC `pg_dump`; save custom dump to `backups/<namespace>/<timestamp>.dump` in seed bucket. Also download to new owner-only file; never overwrite existing file. |
| `publish` | — | Yes | Create GitHub repository if needed; push to `main`. |
| `up` | `--seed-file DUMP`, `--identity-file FILE` optional | Yes | First install: doctor, bootstrap, base, optional seed and identity, deploy/sign-in, publish. |

Mutations require `--approve` (or `BACKSTAGE_AWS_APPROVE=true`); no interactive confirmation. `--help` and `--version` are plain text. Commands otherwise emit one JSON document on stdout; progress and child output go to stderr.

```sh
backstage-aws --project ./portal env up --namespace pr-42 --pull-request 42 --approve
```

CI loads `environments/<environment>.env` and assumes the bootstrap role via GitHub OIDC. No GitHub deployment Environments are used or required; see [OIDC troubleshooting](../troubleshooting.md#github-oidc-and-permissions).
