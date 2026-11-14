> Historical baseline evidence from the original source archive. It does not establish validation of the changed automation bundle. See docs/VALIDATION.md at the repository root for current results.

# Application validation

Checked on 2026-11-13 in Linux x64 with Node **24.22.1** and the checked-in Yarn **4.13.0** release. The application was generated with `@backstage/create-app@0.9.3`; direct Backstage package versions were then pinned from the official **1.56.5** release manifest recorded in this directory.

## Completed checks

All commands below ran from `app/`. They use the bundled Yarn entry point directly; `corepack yarn` selects the same checked-in Yarn release when Corepack is available.

| Check | Actual result |
| --- | --- |
| Full dependency install, including native setup | Passed, exit 0. The successful complete run took 12 minutes 27 seconds in this environment. |
| TypeScript compilation | Passed, exit 0. |
| Backend unit tests | Passed, exit 0: 2 suites, 12 tests, 0 snapshots. |
| Production frontend and backend bundle | Passed, exit 0, with all application runtime configuration variables explicitly unset. |
| Subsequent immutable dependency check | Passed, exit 0. No lockfile change was required. |
| Bootstrap runtime package imports | Passed, exit 0. `pg` 8.23.1 and `@aws-sdk/client-secrets-manager` 3.1164.0 resolve from `scripts/bootstrap-db.mjs`. |
| Separate production-dependency installation from the generated skeleton | Passed, exit 0, with `NODE_ENV=production` and immutable installs enabled. The lockfile remained byte-identical. |
| Imports from that production-only installation | Passed, exit 0. Bootstrap resolves `pg` and Secrets Manager; the backend resolves SFN. |

The full install was:

```sh
NODE_OPTIONS='--max-old-space-size=4096' \
  YARN_NETWORK_CONCURRENCY=50 \
  YARN_HTTP_PROXY="$HTTP_PROXY" \
  YARN_HTTPS_PROXY="$HTTPS_PROXY" \
  node .yarn/releases/yarn-4.13.0.cjs install
```

The proxy and network-concurrency overrides were specific to the validation environment. The checked-in configuration uses network concurrency 16. The install generated the real 913,350-byte `yarn.lock`, including package integrity checksums; the deliverable does not use the generator's initial lockfile stub.

Type checking and tests were:

```sh
NODE_OPTIONS='--no-node-snapshot --max-old-space-size=4096' \
  node .yarn/releases/yarn-4.13.0.cjs tsc

NODE_OPTIONS='--no-node-snapshot --max-old-space-size=4096' \
  node .yarn/releases/yarn-4.13.0.cjs test --watch=false --runInBand
```

The eight rollout tests cover deterministic execution naming; the shared 64-character request-ID limit; reason bounds and forbidden control characters; target ARN validation; submission to only the configured state machine; reconciliation of a completed duplicate request; rejection of an existing execution with different inputs; and propagation of authorization failure. The four permission tests cover anonymous denial, ordinary-member restrictions, the operator-only rollout action, and denial of inline templates, catalog mutation, and unknown privileges.

The production build was deliberately run without credentials or deployed resources:

```sh
env -u POSTGRES_HOST -u POSTGRES_PORT -u POSTGRES_DB \
  -u POSTGRES_USER -u POSTGRES_PASSWORD \
  -u AUTH_GITHUB_CLIENT_ID -u AUTH_GITHUB_CLIENT_SECRET \
  -u AUTH_SESSION_SECRET -u AWS_ROLLOUT_STATE_MACHINE_ARN \
  -u APP_BASE_URL -u BACKEND_BASE_URL \
  NODE_OPTIONS='--no-node-snapshot --max-old-space-size=4096' \
  node .yarn/releases/yarn-4.13.0.cjs build:backend
```

This compiled the bundled frontend and produced `packages/backend/dist/skeleton.tar.gz` and `packages/backend/dist/bundle.tar.gz`. The observed backend bundle was 9,634,732 bytes. These generated outputs are excluded from the Docker build context; Docker rebuilds them from source.

The final immutable check reused the successfully installed native modules:

```sh
NODE_OPTIONS='--max-old-space-size=4096' \
  YARN_HTTP_PROXY="$HTTP_PROXY" \
  YARN_HTTPS_PROXY="$HTTPS_PROXY" \
  node .yarn/releases/yarn-4.13.0.cjs install --immutable --mode=skip-build
```

It passed in 9.34 seconds. `--mode=skip-build` was used only for this repeat lockfile/dependency consistency check. The preceding full installation executed native setup, and the Dockerfile's clean installation does not skip build scripts.

The production-dependency stage was also exercised independently in a fresh temporary directory. It contained the actual generated skeleton, the root `package.json`, `yarn.lock`, `backstage.json`, `.yarnrc.yml`, and bundled Yarn release, with no development `node_modules`. It reused the machine's Yarn archive cache and ran:

```sh
NODE_ENV=production \
  YARN_ENABLE_IMMUTABLE_INSTALLS=true \
  NODE_OPTIONS='--no-node-snapshot --max-old-space-size=4096' \
  YARN_HTTP_PROXY="$HTTP_PROXY" \
  YARN_HTTPS_PROXY="$HTTPS_PROXY" \
  node .yarn/releases/yarn-4.13.0.cjs workspaces focus --all --production
```

This passed in 19.36 seconds, including the production native setup. The original full development install was left untouched. Imports checked from this production-only layout resolved `pg` 8.23.1 and `@aws-sdk/client-secrets-manager` 3.1164.0 from the bootstrap script's directory, and `@aws-sdk/client-sfn` 3.1164.0 from the backend directory. No AWS request or database connection was made. The original and focused-install lockfiles both had SHA-256 `0d5889931414305aa8b6c34d8a54d761c9624eda998b0d246a050978aaec1cab`.

## Warnings and native dependencies

Yarn reports peer-range warnings for React 18.3.1, React DOM 18.3.1, and `@types/react` 18.3.31 against dependencies including Material-UI 4. These are inherited upstream compatibility ranges. They were not silenced or hidden by replacing the dependency graph; TypeScript, the custom tests, and the production frontend build passed with this graph. No browser-based user-interface validation was performed.

The build emitted harmless `NO_COLOR`/`FORCE_COLOR` environment warnings. No application compile error remained. The initial type-check failure exposed that the current `scaffolderActionsExtensionPoint` is a stable export from `@backstage/plugin-scaffolder-node`; the source was corrected to use the installed release's API before the passing checks.

The successful dependency installation performed native-package setup for packages including `@swc/core`, `keytar`, `better-sqlite3`, `esbuild`, `@parcel/watcher`, `cpu-features`, and `ssh2`. Native installation can use a prebuilt binary or fall back to local compilation. The Docker build stages therefore include Python 3, make, g++, pkg-config, and libsecret development headers. The final image stage installs only the libsecret runtime library and certificate authorities in addition to the pinned Node base. No authored Python application, Python service, or TechDocs build is included.

AWS SDK **3.1164.0** was selected to respect the generated Yarn three-day package-age gate. The latest registry version at generation time was newer than this gate allowed. The age gate remains enabled; Backstage's generated preapproved package scope is preserved. The exact installed SDK and all transitive versions are recorded in the lockfile.

## Container and deployment limits

The Dockerfile now performs a complete source build, including immutable install, TypeScript, the custom backend tests, the eight database-bootstrap unit tests, the frontend/backend bundle, and a separate production-dependency installation. It uses the verified published Node 24.22.1 image digest recorded in `versions.json`, rebuilds native dependencies for the image environment, and runs as the nonroot `node` user. The publisher targets `linux/amd64` to match the ECS task architecture.

**Docker was unavailable in this validation environment.** The complete multi-stage image was not built or started here; the Debian image package installation and runtime shared-library behavior remain container checks for CI or the deployment workstation. The production focus command and imports were validated separately on the host, as described above. A passing host bundle and production install are not a claim that an ECS deployment was tested.

No AWS resources were deployed and no live GitHub OAuth sign-in, RDS connection, ALB health check, or Step Functions rollout was executed. The custom SDK behavior is covered by unit tests, not by calls to an AWS account. The owner ID placeholder must be replaced with the intended catalog user's quoted numeric GitHub ID before publishing for deployment. See the repository deployment guide and its CI configuration for the infrastructure, database integration, and image gates.
