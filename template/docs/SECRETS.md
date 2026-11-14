# Database credentials and application secrets

Every secret lives in the `base` stack and is shared by the environments in that AWS account. An environment creates no secrets, so a preview starts without anyone filling in values for it, and the GitHub deploy role has no Secrets Manager permission at all.

## Database credentials

RDS generates, stores, and rotates the master password in Secrets Manager. Only the db-admin task roles can read it; it is not written into Terraform state or exposed to the application.

The application-role password is generated without entering Terraform state and stored in Secrets Manager under the stack KMS key. ECS injects it into containers through the task definition's `secrets`. One application role is shared by all environments in an account: previews do not share an account with production, but preview namespaces are not isolated from each other at the role level.

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

Name each plugin secret in `BACKSTAGE_AWS_PLUGIN_SECRETS` as `name=ENVIRONMENT_VARIABLE` or `name=ENVIRONMENT_VARIABLE:json_key`, separated by commas, for example `github-token=GITHUB_TOKEN`. `base` creates an empty secret called `<project>/plugin/<name>`; store its value once:

```sh
aws secretsmanager put-secret-value --secret-id <project>/plugin/github-token --secret-string file://token.txt
```

Every environment then receives the value in that environment variable. Run `base` again after changing the list, and store the value before the next deployment. Names starting with `AWS_`, `POSTGRES_`, `AUTH_`, `NODE_`, `APP_`, `BACKEND_`, or `PGSSL` are reserved.
