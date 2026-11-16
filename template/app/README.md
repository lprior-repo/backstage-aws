# Backstage application

This Backstage 1.56.5 application is built from the checked-in dependency lockfile and deployed by the `backstage-aws` CLI.

## Included

- One Node service serves the frontend and backend on port 7007.
- PostgreSQL provides plugin schemas and search.
- Authentication is selected at runtime: GitHub OAuth for production or guest sign-in for preview environments.
- Guest sign-in receives ordinary catalog/template read permissions, but no operator or administrative privileges. Preview ingress is restricted by the load balancer.
- All authenticated users may read catalog entities and template parameters/steps. Members of `group:default/platform-operators` may also create and read scaffolder tasks. All other permissions—including catalog mutation, template dry runs, scaffolder cancellation, and unknown permissions—are denied.

## Build the image

The CLI renders the application name and catalog owner's immutable numeric GitHub ID from the placeholders in `app-config.yaml` and `catalog/org.yaml`. To build directly, run from this directory:

```sh
docker build --platform linux/amd64 -f packages/backend/Dockerfile -t backstage .
```

The Dockerfile installs locked dependencies, builds the frontend and backend, and installs production dependencies into the runtime. TypeScript checks and application tests run separately in the validation workflow. It pins Node 24.22.1 by image digest and uses the bundled Yarn 4.13.0 release. Building requires network access for the base image, operating-system packages, and npm dependencies; it does not require AWS, database, or OAuth credentials.

Native Node modules can require Python and C++ during dependency compilation. The Dockerfile includes these tools and libsecret headers in build stages, so they do not need to be installed on the host. The runtime contains no Python service or TechDocs builder. Allow several gigabytes of memory for the frontend build; the builder's Node heap limit is 4 GiB.

## Optional host development

Use Node 24.22.1, bundled Yarn 4.13.0, and native build dependencies (on Debian/Ubuntu: Python 3, make, g++, pkg-config, and libsecret development headers). From this directory, run:

```sh
node .yarn/releases/yarn-4.13.0.cjs install --immutable
node .yarn/releases/yarn-4.13.0.cjs tsc
node .yarn/releases/yarn-4.13.0.cjs test --watch=false --runInBand
node .yarn/releases/yarn-4.13.0.cjs build:backend
```

Load the project settings as described in the [project README](../README.md). From the generated project's root, `backstage-aws check --runtime --postgres-bin /path/to/postgresql16/bin` runs the build checks plus an isolated PostgreSQL/application rehearsal. Install PostgreSQL 16 client/server tools and put Node 24.22.1 on `PATH`; no Docker or existing database is used. This is not an AWS deployment or GitHub OAuth acceptance test.

## Run locally

From the generated project's root, run `backstage-aws --container-engine podman dev` for rootless Podman, or `backstage-aws dev` for the default Docker engine. The global `BACKSTAGE_AWS_CONTAINER_ENGINE` setting also selects `docker` or `podman`; flags override it, and there is no automatic fallback. You need Node 24.22.1, native Node build tools (listed above), and the selected engine with a Compose provider. For rootless Podman with a Docker Compose provider, first run `systemctl --user start podman.socket`. Both `podman info` and `podman compose version` must succeed. No AWS settings, credentials, Terraform, Corepack, native PostgreSQL tools, or GitHub token are needed. The command starts PostgreSQL 16 on `127.0.0.1:54320`, waits for its health check, creates `app/app-config.local.yaml` from the example if absent, installs locked dependencies with bundled Yarn, and starts Backstage at <http://localhost:3000> (backend <http://localhost:7007>). Press Ctrl-C to stop the app and remove its containers; a project-specific engine volume preserves the database.

```sh
backstage-aws --container-engine podman dev
```

Choose guest sign-in in the browser. The local config enables guest sign-in in development mode only and reads `catalog/org.yaml` and `catalog/entities.yaml` from disk, with no GitHub calls. `--port N` selects the database port and must match an existing local config. `--setup-only` prepares and health-checks the database, then stops it without starting the app. `--no-install` requires existing dependencies. `--reset` deletes this project's selected-engine database volume before starting fresh; it preserves config, dependencies, and any old native data under `app/.local`. Docker and Podman store separate volumes; switching does not migrate data. The former dev `--postgres-bin` option is removed; the separate `check --runtime` command remains native.

Check readiness with `curl --fail http://localhost:7007/.backstage/health/v1/readiness` while the app runs. After Ctrl-C, check `podman ps` for this project's containers. The final JSON contains `postgres.engine` and `postgres.project`; recover after a forced kill with `podman compose -f app/compose.yaml -p PROJECT_NAME down`. Substitute `docker` when selected. Add `--volumes` only to deliberately erase its database.

List and add plugins from the generated project root:

```sh
backstage-aws plugin list
backstage-aws plugin add notifications --install
```

Adding notifications also adds its signals dependency. `plugin add` edits the local app; it does not deploy it. `--install` refreshes the Yarn lockfile and needs package-registry access. Run `backstage-aws plugin add --help` to see all options.

## Runtime configuration

The container starts with `app-config.yaml` and `app-config.production.yaml`, plus exactly one authentication config selected by the `env` stack. The GitHub config reads `AUTH_GITHUB_CLIENT_ID` and `AUTH_GITHUB_CLIENT_SECRET`; both modes read `AUTH_SESSION_SECRET`. The production config reads `APP_BASE_URL`, `BACKEND_BASE_URL`, `POSTGRES_HOST`, `POSTGRES_PORT`, `POSTGRES_DB`, `POSTGRES_USER`, and `POSTGRES_PASSWORD`. Never commit live secrets. The app and backend base URLs should be the same HTTPS portal origin.

The GitHub OAuth callback is `BACKEND_BASE_URL/api/auth/github/handler/frame`. GitHub sign-in resolves users through the immutable ID annotation in the catalog. Guest access is enabled only when the guest provider config is selected; it is intended for network-restricted preview environments.

RDS TLS verification uses `certs/aws-rds-global-bundle.pem`. Refresh this public AWS trust bundle when AWS announces CA changes. ALB health checks should use `/.backstage/health/v1/readiness`.
