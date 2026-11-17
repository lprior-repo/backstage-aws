# CLI reference

`backstage-aws` is non-interactive. Settings, defaults and validation: [settings](settings.md). Stdout/errors: [output contract](output-and-errors.md). Global options work before or after any subcommand; flags override environment variables. `setup`, `check`, and project-local `plugin`, `config`, and `dev` do not need deployment settings; other commands validate them.

## Commands

`setup` discovers prerequisites without deployment settings; `check` runs offline. Project-local commands also bypass deployment settings and Terraform. Other commands validate deployment settings. Terraform 1.17.2 is required except for `init`, `setup`, `doctor`, standalone `check --release-archive`, and project-local commands (discovery checks it when reached).

| Command | Options | Approval | Result |
|---|---|---:|---|
| `setup` | `--aws-cli PATH`, `--profile NAME` | No | Read-only ordered discovery; one next action and resumable argv. Ready means run `doctor`, not deployed. |
| `init --output DIR` | required | No | Render project; destination must be new. Writes `environments/<environment>.env`. |
| `check` | `--app`, `--runtime`, `--postgres-bin DIR` (requires `--runtime`), `--release-archive FILE` | No | Offline Terraform validation/tests; optionally build the app, rehearse native PostgreSQL/app behavior, or smoke-test a release archive. See below. |
| `doctor` | — | No | Check Terraform, the selected container engine (Docker Buildx or Podman), Git, AWS identity/account, GitHub admin access. No changes. |
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
| `db dump` | `--file FILE` optional | Yes | Run VPC `pg_dump`; save custom dump to a unique `backups/<namespace>/<timestamp>-<operation-id>.dump` key. Optional `--file` downloads it to a new owner-only local file; existing files are never overwritten. |
| `publish` | — | Yes | Create GitHub repository if needed; push to `main`. |
| `up` | `--seed-file DUMP`, `--identity-file FILE` optional | Yes | First install: doctor, bootstrap, base, optional seed, optional identity setup, environment deploy, publish. Configure identity before deployment when GitHub sign-in is enabled. |

## Project-local commands

Run these from a generated project root (the directory containing `app/`), or use global `--project DIR`. They edit or run the local `app/`; they do not deploy it and do not need AWS credentials, deployment settings, or Terraform.

```text
backstage-aws plugin list
backstage-aws plugin add [IDS...] [--frontend PKG[@VER]]... [--backend PKG[@VER]]... [--set KEY=VALUE]... [--param KEY=VALUE]... [--install] [--dry-run]
backstage-aws plugin remove [IDS...] [--frontend PKG[@VER]]... [--backend PKG[@VER]]... [--set KEY=VALUE]... [--param KEY=VALUE]... [--install] [--dry-run]
backstage-aws config get PATH [--file FILE]
backstage-aws config set PATH VALUE [--file FILE]
backstage-aws [--container-engine docker|podman] dev [--port N] [--no-install] [--setup-only] [--reset]
```

`plugin list` reports available and installed catalog plugins. `plugin add` and `plugin remove` accept catalog IDs and/or arbitrary frontend/backend package names. Package versions resolve from the pinned release provenance; a package absent there needs an explicit exact `PKG@VERSION`. `--set KEY=VALUE` supplies config values and `--param KEY=VALUE` supplies catalog parameters. `--install` refreshes the Yarn lockfile; `--dry-run` previews edits without writing. The `notifications` entry automatically adds its `signals` dependency; see the [plugin guide](../guides/plugins.md) for catalog IDs and workflows.

`config get` and `config set` use dotted YAML paths. The default file is `app-config.yaml`; `--file` names a file inside `app/`. `set` parses `true`/`false` and integers as typed scalars, treats other values as text, creates missing parent keys, and preserves comments/layout. Use `${VAR}` for an environment variable resolved by Backstage at runtime; do not put secrets in YAML.

`dev` starts PostgreSQL 16 using the selected engine's Compose provider, waits for health, and runs Backstage at `http://localhost:3000` (backend `http://localhost:7007`). Global `--container-engine docker|podman` overrides `BACKSTAGE_AWS_CONTAINER_ENGINE`; the default is `docker`, with no automatic fallback. Rootless Podman with a Docker Compose provider needs `systemctl --user start podman.socket`. The command requires Node 24.22.1, native Node build tools, and a working selected engine/Compose provider, not AWS, Terraform, Corepack, or native PostgreSQL tools. `--port` selects the loopback-only PostgreSQL port (default `54320`); existing local config must match. Missing dependencies are installed with the bundled Yarn and immutable lockfile. `--no-install` requires existing dependencies. `--setup-only` prepares config/dependencies and health-checks the database, then removes containers without starting the app. Normal exit/Ctrl-C also removes the project's containers/network, preserving its named database volume. `--reset` deletes that project's selected-engine database volume before startup; it preserves config, dependencies, and old native `app/.local` data. Engine volumes are separate; switching engines does not migrate data. The former dev `--postgres-bin` flag is removed; `check --runtime` still accepts it. Final JSON includes `postgres.engine` and `postgres.project` for manual Compose recovery. Details: [local development guide](../guides/local-development.md).

## Output and exit codes

`--help` and `--version` are plain text; parsed commands and parse errors write one newline-terminated JSON envelope to stdout. Step events and child-process output go to stderr. Exit codes: `0` success, `2` invalid input, `3` approval/precondition/setup blocked, `4` tool/API/GitHub/state/database failure, `5` remote work continues after timeout. The envelope fields, error codes, and retryability are [documented here](output-and-errors.md). Remote AWS/GitHub mutations require `--approve` (or `BACKSTAGE_AWS_APPROVE=true`); local project commands such as `init`, `plugin`, `config`, and `dev` do not. After a successful `env up`, step timings append to `$GITHUB_STEP_SUMMARY` when set.

```sh
backstage-aws --project ./portal env up --namespace pr-42 --pull-request 42 --approve
```

CI loads `environments/<environment>.env` and assumes the bootstrap role via GitHub OIDC. No GitHub deployment Environments are used or required; see [OIDC troubleshooting](../troubleshooting.md#github-oidc-and-permissions).

## Offline acceptance

From a generated project, `backstage-aws check --app` installs locked dependencies, typechecks, runs application tests, and builds the backend/frontend bundle after the Terraform checks.

`backstage-aws check --runtime --postgres-bin /path/to/postgresql16/bin` also rehearses database and application behavior using an isolated temporary PostgreSQL 16 cluster. It implies `--app`, requires Node 24.22.1 on `PATH`, and never connects to an existing database. Native build prerequisites are listed in the generated application's README. Docker and AWS credentials are not needed.

`backstage-aws check --release-archive ./backstage-aws-x86_64-unknown-linux-gnu.tar.gz` checks archive structure and executes its CLI version/help/error contract. Alone, it needs neither a project nor Terraform. Combining it with `--app` or `--runtime` also runs project checks. This executes archive code: use trusted artifacts. It is not a signature or provenance check; generated CI verifies GitHub attestation before installing and checking the matching release.

Offline results do not establish working AWS permissions, Fargate/RDS behavior, DNS/TLS, GitHub OAuth, production backup/restore, or preview startup time.

## Terraform generation

`init` generates `.tf` and HCL files through `hcl-edit` syntax trees from the [hcl-rs project](https://github.com/martinohmann/hcl-rs), preserving reviewed blueprint comments, Terraform expressions and embedded scripts. Runtime backend blocks and native `terraform.tfvars` files use `hcl-rs` builders/serialization, not hand-assembled HCL or JSON variable files. Variables remain literal data, including interpolation-looking strings. Operation files stay in disposable private Terraform workspaces.

HCL syntax validation is not AWS-provider schema validation: `check` still runs Terraform validation and tests. Resource blueprints remain the single infrastructure definition; Rust does not duplicate every provider resource as a separate model.

