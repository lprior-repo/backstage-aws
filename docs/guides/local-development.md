# Local development

These project-local commands work on a generated project without AWS credentials, AWS settings, or Terraform. Run them from the project root (the directory containing `app/`), or pass `--project DIR`. They edit or run the local project only; they do not deploy it. See the [CLI reference](../reference/cli.md#project-local-commands) and the [plugin guide](plugins.md).

## Start Backstage

Requirements: Node.js 24.22.1, a working Docker or rootless Podman engine with a Compose provider, and native Node build tools (Python 3, make, C++, pkg-config, and libsecret development headers). Select the engine with global `--container-engine docker|podman` or `BACKSTAGE_AWS_CONTAINER_ENGINE`; the default is `docker`, with no automatic fallback. No AWS settings, credentials, Terraform, native PostgreSQL tools, GitHub token, or Corepack are needed. `dev` starts PostgreSQL 16 using the selected engine, waits for its health check, creates `app/app-config.local.yaml` from the example if absent, installs locked dependencies using the bundled Yarn, and starts Backstage at <http://localhost:3000> (backend <http://localhost:7007>). Choose guest sign-in in the browser. Press Ctrl-C to stop the app and remove its containers/network; the database volume persists.

```sh
cd /path/to/your/project
backstage-aws --container-engine podman dev
```

For rootless Podman, `podman info` and `podman compose version` must succeed. When `podman compose` uses Docker Compose as its provider, start the user API socket with `systemctl --user start podman.socket`; do not run Podman as root. Docker users can omit the engine flag and must have working `docker info` and Compose v2 or later. The CLI does not change daemon permissions or start a system service.

`--port N` selects the PostgreSQL port (default `54320`); only `127.0.0.1` is exposed. An existing local config must use the same database port; the command fails rather than silently connecting to the wrong database. `--no-install` requires existing `app/node_modules`. `--setup-only` prepares config/dependencies, starts the database until healthy, then removes its containers without starting the app. `--reset` deletes only this project's selected-engine database volume before starting a fresh database; it preserves config and dependencies. Docker and Podman have separate volumes: switching engines does not migrate data. Old native data under `app/.local` is not migrated or deleted. The former dev `--postgres-bin` option is removed; `check --runtime` still uses native PostgreSQL tools.

The local config uses development-only guest sign-in with catalog/template read permissions, not operator privileges. It loads `catalog/org.yaml` and `catalog/entities.yaml` from disk, so an unpublished project works without GitHub access. Add remote catalog locations and GitHub integration separately if needed; never commit tokens.

### Verify and stop

In another terminal while `dev` is running:

```sh
curl --fail http://localhost:3000/ >/dev/null
curl --fail http://localhost:7007/.backstage/health/v1/readiness
token="$(curl --fail --silent http://localhost:7007/api/auth/guest/refresh | jq -er .backstageIdentity.token)"
curl --fail --silent -H "Authorization: Bearer $token" http://localhost:7007/api/catalog/entities
unset token
```

After Ctrl-C, `podman ps --filter label=com.docker.compose.project --format '{{.Names}}'` should show no containers for this project (other projects may still be running). The command's final JSON includes `postgres.engine` and `postgres.project`, the Compose project name. For recovery after a forced kill, run `podman compose -f app/compose.yaml -p PROJECT_NAME down` with that name. Substitute `docker` when that was the selected engine. Do not use `down --volumes` unless you intend to delete its database.

If `docker info` reports permission denied on `/var/run/docker.sock`, ask the workstation owner to grant Docker access, then log in again. Membership of the `docker` group grants root-equivalent access; do not make the socket world-writable. An owner can use `sudo usermod -aG docker USERNAME`. If Docker is stopped, the owner must start it. The CLI fails before starting containers when the daemon is unavailable.

## Human workflow

```sh
cd /path/to/your/project
backstage-aws plugin list
backstage-aws config get auth.providers.guest --file app-config.local.yaml
# No AWS or GitHub credentials are needed.
backstage-aws --container-engine podman dev
```

For plugin changes, use the examples in the [plugin guide](plugins.md). To change YAML without losing comments or layout, use `config set`, for example:

```sh
backstage-aws config set app.title "Developer Portal" --file app-config.local.yaml
```

## Agent and script workflow

Commands do not prompt. Each command writes one JSON object to stdout; progress and child-process logs go to stderr. Capture stdout separately, check the process exit status, and parse the JSON before relying on its contents. Stop on a nonzero exit code or an `ok: false` result. For example:

```sh
if backstage-aws plugin list >result.json; then
  jq -es 'length == 1 and (.[0] | type == "object")' result.json >/dev/null
else
  status=$?
  exit "$status"
fi
```

Exit codes are `0` for success, `2` for invalid input, `3` for a blocked precondition or missing approval, `4` for a tool/API failure, and `5` when remote work continues after a timeout. These local commands do not start AWS work; the general CLI contract is documented in the [CLI reference](../reference/cli.md#output-and-exit-codes).

## What “offline” means

`plugin list`, `config get`, and `config set` operate on local project files. `plugin add` and `plugin remove` edit local app files; adding `--install` updates the dependency lockfile and needs package-registry access. `dev` does not use AWS or Terraform, but pulling the PostgreSQL image and installing dependencies require network access on first run. The default local catalog does not contact GitHub. This is not an air-gapped mode or an AWS deployment test.
