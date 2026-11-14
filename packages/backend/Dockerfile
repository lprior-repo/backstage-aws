# This dockerfile builds an image for the backend package.
# It should be executed with the root of the repo as docker context.
#
# Before building this image, be sure to have run the following commands in the repo root:
#
# yarn install --immutable
# yarn tsc
# yarn build:backend
#
# Host build steps (yarn install, yarn build:backend) must use the same
# Node version as the FROM image below. Mismatched versions break native modules.
#
# Once the commands have been run, you can build the image using `yarn build-image`
#
# Alternatively, there is also a multi-stage Dockerfile documented here:
# https://backstage.io/docs/deployment/docker#multi-stage-build

FROM node:24.14.0-trixie-slim

# Install sqlite3 dependencies. You can skip this if you don't use sqlite3 in the image,
# in which case you should also move better-sqlite3 to "devDependencies" in package.json.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && \
    apt-get install -y --no-install-recommends libsqlite3-dev ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p /app/certs && \
    curl --fail --silent --show-error https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem \
      -o /app/certs/rds-global-bundle.pem

# From here on we use the least-privileged `node` user to run the backend.
USER node

# This should create the app dir as `node`.
# If it is instead created as `root` then the `tar` command below will fail: `can't create directory 'packages/': Permission denied`.
# If this occurs, then ensure BuildKit is enabled (`DOCKER_BUILDKIT=1`) so the app dir is correctly created as `node`.
WORKDIR /app

# Copy files needed by Yarn
COPY --chown=node:node .yarn ./.yarn
COPY --chown=node:node .yarnrc.yml ./
COPY --chown=node:node backstage.json ./

# This switches many Node.js dependencies to production mode.
ENV NODE_ENV=production
ENV NODE_OPTIONS=--no-node-snapshot
ENV RDS_CA_BUNDLE=/app/certs/rds-global-bundle.pem

# Copy repo skeleton first, to avoid unnecessary docker cache invalidation.
# The skeleton contains the package.json of each package in the monorepo,
# and along with yarn.lock and the root package.json, that's enough to run yarn install.
COPY --chown=node:node yarn.lock package.json packages/backend/dist/skeleton.tar.gz ./
RUN tar xzf skeleton.tar.gz && rm skeleton.tar.gz

RUN --mount=type=cache,target=/home/node/.cache/yarn,sharing=locked,uid=1000,gid=1000 \
    yarn workspaces focus --all --production && rm -rf "$(yarn cache clean)"

# This will include the examples, if you don't need these simply remove this line
COPY --chown=node:node examples ./examples
COPY --chown=node:node catalog ./catalog
COPY --chown=node:node packages/backend/scripts/bootstrap-database.cjs ./packages/backend/scripts/bootstrap-database.cjs

# Then copy the rest of the backend bundle, along with any other files we might want.
COPY --chown=node:node packages/backend/dist/bundle.tar.gz app-config*.yaml ./
RUN tar xzf bundle.tar.gz && rm bundle.tar.gz

EXPOSE 7007
CMD ["node", "packages/backend", "--config", "app-config.production.yaml"]
