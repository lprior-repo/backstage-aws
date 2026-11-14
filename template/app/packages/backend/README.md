# Backstage backend

The backend serves the Backstage frontend and APIs on port 7007. Its plugins are registered in `src/index.ts`; authentication providers are enabled by the selected runtime config file.

## Local development

From the application root, install the locked dependencies and start the backend:

```sh
node .yarn/releases/yarn-4.13.0.cjs install --immutable
node .yarn/releases/yarn-4.13.0.cjs workspace backend start
```

Local configuration can be provided through Backstage's usual config files. The deployed service combines the shared application config with the production config and one authentication-mode config.

## Authentication and permissions

The GitHub provider resolves sign-ins to catalog users by immutable numeric GitHub ID. The guest provider is used only when explicitly selected for an environment. The permission policy grants catalog and template reads to signed-in users; only members of `group:default/platform-operators` may manage scaffolder tasks. Unknown permissions and catalog mutations are denied.
