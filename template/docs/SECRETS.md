# Database credentials and application secrets

The `base` stack owns the account-shared Secrets Manager values; environment stacks create none. Database credentials are generated automatically. Default previews use guest auth and need no GitHub OAuth value. Production GitHub sign-in needs OAuth credentials, and any configured plugin secret must be stored before using the plugin that needs it. The GitHub deploy role cannot read secret values.

## Database credentials

RDS generates, stores, and rotates the master password in Secrets Manager. Only the db-admin ECS task and execution roles can retrieve it; the portal task cannot. It is not written into Terraform state.

The application-role password is generated without entering Terraform state and stored in Secrets Manager under the stack KMS key. ECS injects it into containers through the task definition's `secrets`. Keep preview and production accounts separate; within one account, every environment shares this role, so preview namespaces are not isolated at the role level.

Database TLS is forced by the server and verified by the application. Do not weaken certificate validation to work around a connection failure.

Rotate the application password with:

```sh
backstage-aws db rotate-password --approve
```

The command generates a replacement password, updates the PostgreSQL role through a db-admin task, and restarts every service in the cluster. Expect brief connection errors while services restart. It is an account-wide operation; coordinate it with active deployments. If it fails part-way, run it again.

## GitHub sign-in

Preview environments use guest sign-in, limited by the load balancer's ingress allowlist. Production uses GitHub OAuth. `base` creates an empty identity secret; store the OAuth application's credentials in it after `base` and before the first `env up` with GitHub sign-in, or the service cannot start:

```sh
backstage-aws secrets set-identity --file FILE --approve
```

`FILE` is JSON with `clientId` and `clientSecret`. Without `--file`, the command reads `BACKSTAGE_AWS_OAUTH_CLIENT_ID` and `BACKSTAGE_AWS_OAUTH_CLIENT_SECRET` from the environment. Secret values are never accepted as arguments and never printed.

## Plugin secrets

Set `BACKSTAGE_AWS_PLUGIN_SECRETS` to comma-separated `name=ENV_VAR` entries (for example, `github-token=GITHUB_TOKEN`); append `:json_key` to inject one field from a JSON secret. For each `name`, `base` creates an empty secret named `$BACKSTAGE_AWS_NAME/plugin/<name>`. Include every name used by an environment in the settings supplied to `base`; otherwise that environment cannot inject it. Store each value after creation:

```sh
aws secretsmanager put-secret-value \
  --region "$BACKSTAGE_AWS_AWS_REGION" \
  --secret-id "$BACKSTAGE_AWS_NAME/plugin/github-token" \
  --secret-string file://token.txt
```

Plugin environment-variable names starting with `AWS_`, `POSTGRES_`, `AUTH_`, `NODE_`, `APP_`, `BACKEND_`, or `PGSSL` are reserved.
