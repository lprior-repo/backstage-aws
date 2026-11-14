# syntax=docker/dockerfile:1
# Build from the app directory:
# docker build --platform linux/amd64 -f packages/backend/Dockerfile -t backstage .
# The complete source build happens inside Docker. Native compilation tools are
# confined to build stages; the final image runs one Node process as user node.
ARG NODE_IMAGE=node:24.22.1-trixie-slim@sha256:89c5244bc22a9bc3287d89ebc7e49b9cb5bb84cd40e6175ac3421d5a9f68b95a

FROM ${NODE_IMAGE} AS build-tools
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 make g++ pkg-config libsecret-1-dev git ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /app \
    && chown node:node /app
ENV NODE_OPTIONS="--no-node-snapshot --max-old-space-size=4096"
ENV npm_config_python=/usr/bin/python3
ENV npm_config_nodedir=/usr/local
USER node
WORKDIR /app

FROM build-tools AS build
# Dependencies get their own layer so a source-only change reuses it from the
# registry cache. Type checks and tests run in the validate workflow, not here.
COPY --chown=node:node .yarn ./.yarn
COPY --chown=node:node .yarnrc.yml backstage.json package.json yarn.lock ./
COPY --chown=node:node packages/app/package.json packages/app/package.json
COPY --chown=node:node packages/backend/package.json packages/backend/package.json
RUN --mount=type=cache,target=/home/node/.yarn,sharing=locked,uid=1000,gid=1000 \
    node .yarn/releases/yarn-4.13.0.cjs install --immutable
COPY --chown=node:node . .
RUN node .yarn/releases/yarn-4.13.0.cjs build:backend

FROM build-tools AS production-dependencies
ENV NODE_ENV=production
ENV YARN_ENABLE_IMMUTABLE_INSTALLS=true
COPY --from=build --chown=node:node /app/.yarn/releases ./.yarn/releases
COPY --from=build --chown=node:node /app/.yarnrc.yml /app/backstage.json /app/yarn.lock /app/package.json ./
COPY --from=build --chown=node:node /app/packages/backend/dist/skeleton.tar.gz ./
RUN tar xzf skeleton.tar.gz && rm skeleton.tar.gz
RUN --mount=type=cache,target=/home/node/.yarn,sharing=locked,uid=1000,gid=1000 \
    node .yarn/releases/yarn-4.13.0.cjs workspaces focus --all --production

FROM ${NODE_IMAGE} AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends libsecret-1-0 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /app \
    && chown node:node /app
ENV NODE_ENV=production
ENV NODE_OPTIONS=--no-node-snapshot
USER node
WORKDIR /app

COPY --from=production-dependencies --chown=node:node /app /app
COPY --from=build --chown=node:node /app/packages/backend/dist/bundle.tar.gz /app/app-config.yaml /app/app-config.production.yaml /app/app-config.auth-guest.yaml /app/app-config.auth-github.yaml ./
RUN tar xzf bundle.tar.gz && rm bundle.tar.gz
COPY --from=build --chown=node:node /app/catalog ./catalog
COPY --from=build --chown=node:node /app/certs ./certs

EXPOSE 7007
CMD ["node", "packages/backend", "--config", "app-config.yaml", "--config", "app-config.production.yaml"]
