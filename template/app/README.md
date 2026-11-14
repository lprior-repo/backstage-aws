# Backstage application

This Backstage 1.56.5 application is built from the checked-in dependency lockfile and deployed by the `backstage-aws` CLI.

## Included

- One Node service serves the frontend and backend on port 7007.
- PostgreSQL provides plugin schemas and search.
- Authentication is selected at runtime: GitHub OAuth for production or guest sign-in for preview environments.
- Guest sign-in receives ordinary catalog/template read permissions, but no operator or administrative privileges. Preview ingress is restricted by the load balancer.
- Members of `group:default/platform-operators` may create and read scaffolder tasks. Catalog mutation, inline template dry runs, arbitrary scaffolder actions, and unknown permissions are denied.

## Build the image

The CLI renders the application name and catalog owner's immutable numeric GitHub ID from the placeholders in `app-config.yaml` and `catalog/org.yaml`. To build directly, run from this directory:

```sh
docker build --platform linux/amd64 -f packages/backend/Dockerfile -t backstage .
```

The Dockerfile installs locked dependencies, checks TypeScript, runs the application tests, builds the frontend and backend, and installs production dependencies into the runtime. It pins Node 24.22.1 by image digest and uses the bundled Yarn 4.13.0 release. Building requires network access for the base image, operating-system packages, and npm dependencies; it does not require AWS, database, or OAuth credentials.

Native Node modules can require Python and C++ during dependency compilation. The Dockerfile includes these tools and libsecret headers in build stages, so they do not need to be installed on the host. The runtime contains no Python service or TechDocs builder. Allow several gigabytes of memory for the frontend build; the builder's Node heap limit is 4 GiB.

## Optional host development

Use Node 24.22.1, bundled Yarn 4.13.0, and native build dependencies (on Debian/Ubuntu: Python 3, make, g++, pkg-config, and libsecret development headers). From this directory, run:

```sh
node .yarn/releases/yarn-4.13.0.cjs install --immutable
node .yarn/releases/yarn-4.13.0.cjs tsc
node .yarn/releases/yarn-4.13.0.cjs test --watch=false --runInBand
node .yarn/releases/yarn-4.13.0.cjs build:backend
```

## Runtime configuration

The container starts with `app-config.yaml` and `app-config.production.yaml`, plus exactly one authentication config selected by the `env` stack. The GitHub config reads `AUTH_GITHUB_CLIENT_ID` and `AUTH_GITHUB_CLIENT_SECRET`; both modes read `AUTH_SESSION_SECRET`. The production config reads `APP_BASE_URL`, `BACKEND_BASE_URL`, `POSTGRES_HOST`, `POSTGRES_PORT`, `POSTGRES_DB`, `POSTGRES_USER`, and `POSTGRES_PASSWORD`. Never commit live secrets. The app and backend base URLs should be the same HTTPS portal origin.

The GitHub OAuth callback is `BACKEND_BASE_URL/api/auth/github/handler/frame`. GitHub sign-in resolves users through the immutable ID annotation in the catalog. Guest access is enabled only when the guest provider config is selected; it is intended for network-restricted preview environments.

RDS TLS verification uses `certs/aws-rds-global-bundle.pem`. Refresh this public AWS trust bundle when AWS announces CA changes. ALB health checks should use `/.backstage/health/v1/readiness`.
