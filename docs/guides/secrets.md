# Secrets

| Keep here | Never here |
| --- | --- |
| Secret values in AWS Secrets Manager | Git, Terraform variables, workflow YAML, CLI arguments, `environments/*.env`, CI logs |

Settings files contain configuration and role ARNs, not credentials. Operators use short-lived AWS credentials; GitHub Actions gets AWS access through OIDC.

## GitHub OAuth

Production defaults to GitHub auth; preview defaults to guest. After `base`, create a GitHub OAuth app with callback `https://HOST/api/auth/github/handler/frame`, then store credentials:

```sh
chmod 600 ./oauth.json
backstage-aws secrets set-identity --file ./oauth.json --approve
```

JSON keys: `clientId` and `clientSecret` (snake_case aliases accepted). Or provide `BACKSTAGE_AWS_OAUTH_CLIENT_ID` and `BACKSTAGE_AWS_OAUTH_CLIENT_SECRET` in the process environment. The command prints neither value. Deploy/restart the service and verify login.

## Plugin credentials

Declare each secret name and task environment mapping in project settings before `base`. Terraform creates the Secrets Manager resource and ECS injects its value at runtime; optional JSON-key selection is supported. Populate values through a restricted Secrets Manager workflow and match the plugin's expected name/JSON shape. See `template/infra/base/main.tf`.

## Database passwords

RDS master credentials are distinct from the application role password. Do not rotate the master password manually. Rotate the application password with:

```sh
backstage-aws db rotate-password --approve
```

The command stages a random secret, updates the DB role, promotes it to `AWSCURRENT`, and restarts every service in the account. Expect brief connection errors; rotation affects all namespaces. On failure, inspect the failed step and AWS task/service state, then follow CLI guidance.

## Preserve keys

Secrets and RDS use the base customer-managed KMS key; Terraform state has a separate key. Preserve both keys, policies and service/operator access for all encrypted data. Losing a key can make ciphertext unrecoverable. Secrets Manager deletion recovery is seven days, not a backup. See [Back up and restore](backup-and-restore.md). 
