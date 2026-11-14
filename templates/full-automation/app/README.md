# Backstage application

This application was generated using @backstage/create-app 0.9.3 and pinned to the official Backstage 1.56.5 release manifest. See [the deployment guide](../README.md) for the complete AWS installation sequence.

## Included

- One Node service serves the frontend and backend on port 7007.
- GitHub OAuth signs in only users whose immutable numeric GitHub ID appears in the trusted catalog.
- PostgreSQL provides plugin schemas and search.
- Only the platform-operators group can request the configured Step Functions rollout.
- The rollout action accepts an optional short reason, derives its request ID from the scaffolder task, and returns an AWS execution link. The workflow initializes the database role and rolls out the current ECS task definition; it does not build or select a new image.
- Catalog mutation, inline template dry runs, arbitrary scaffolder actions, and unknown permissions are denied.

## Build the image

Replace the quoted placeholder in catalog/org.yaml with the intended operator's immutable numeric GitHub ID. More operators require additional trusted User entities with the platform-operators membership. These files are included in the image.

The deployment guide's publishing script checks the catalog owner and builds this image. To build directly, run from this directory:

```sh
docker build --platform linux/amd64 -f packages/backend/Dockerfile -t backstage .
```

The Dockerfile installs the locked dependencies, checks TypeScript, runs the custom backend and database-bootstrap unit tests, builds the frontend and backend, and installs only production dependencies into the runtime. It pins Node 24.22.1 by the published image digest and uses bundled Yarn 4.13.0. Building requires network access for the base image, operating-system packages, and npm dependencies; it does not require AWS, database, or OAuth credentials.

Native Node modules can require Python and C++ during dependency compilation. The Dockerfile includes these tools and libsecret headers in build stages, so they do not need to be installed on the host. The runtime contains no Python service or TechDocs builder. Allow several gigabytes of memory for the frontend build; the builder's Node heap limit is 4 GiB.

See [the validation record](provenance/VALIDATION.md) for the actual local checks and the checks that still require Docker or AWS.

## Optional host development

Use Node 24.22.1, bundled Yarn 4.13.0, and native build dependencies (on Debian/Ubuntu: Python 3, make, g++, pkg-config, and libsecret development headers). The equivalent host checks are:

```sh
export NODE_OPTIONS="--no-node-snapshot --max-old-space-size=4096"
corepack yarn install --immutable
corepack yarn tsc
corepack yarn test --watch=false --runInBand
corepack yarn build:backend
```

The production bundle builds without runtime credentials. Starting the application requires the PostgreSQL connection, GitHub OAuth settings, session secret, and configured rollout ARN described below. There is no development guest-login bypass.

## Production configuration

The ECS service supplies APP_BASE_URL, BACKEND_BASE_URL, POSTGRES_HOST, POSTGRES_PORT, POSTGRES_DB, POSTGRES_USER, POSTGRES_PASSWORD, AUTH_GITHUB_CLIENT_ID, AUTH_GITHUB_CLIENT_SECRET, AUTH_SESSION_SECRET, and AWS_ROLLOUT_STATE_MACHINE_ARN. Never commit live secrets. APP_BASE_URL and BACKEND_BASE_URL should be the same HTTPS portal origin.

The GitHub OAuth callback is BACKEND_BASE_URL followed by /api/auth/github/handler/frame. Numeric ID resolution requires an existing catalog User; guest access and uncatalogued sign-in are disabled.

RDS TLS verification uses certs/aws-rds-global-bundle.pem. Refresh this public AWS trust bundle when AWS announces CA changes.

ALB health checks should use /.backstage/health/v1/readiness. All application API requests retain Backstage's default authentication requirement.

The only accepted Step Functions target is the configured unqualified Standard state-machine ARN in commercial AWS. The ECS task role also restricts that target. Direct AWS credentials use the normal SDK credential chain.
