# Secrets

Secret values belong in AWS Secrets Manager, not Git, Terraform variables, workflow YAML, command-line arguments, `environments/*.env`, or CI logs. Settings files contain non-secret configuration and role ARNs. Operators use short-lived AWS credentials; GitHub Actions assumes the deploy role through OIDC.

Every successful `backstage-aws` command exits 0 and prints one JSON document. Exit 2 means invalid input, 3 blocked/precondition, 4 external or state failure, and 5 timeout; remote work may continue after exit 5.

## GitHub OAuth identity

1. After `base`, create a GitHub OAuth application with callback `https://HOST/api/auth/github/handler/frame`. Production defaults to GitHub auth; preview defaults to guest. Store the JSON source outside the repository:

   ```sh
   set -a; . environments/production.env; set +a
   chmod 600 /secure/oauth.json
   backstage-aws secrets set-identity --file /secure/oauth.json --approve
   ```

   Use JSON keys `clientId` and `clientSecret`. The command writes a Secrets Manager version and returns JSON `result.secret_arn` and `result.version_id`; it does not print values or restart running tasks. Tasks receive the secret at startup. This CLI has no command to force task replacement after changing the secret; use a deployment process that replaces tasks, then verify sign-in. The CLI does not verify authentication.
2. If you retain the original OAuth JSON securely, load the target account settings and restore that value with:

   ```sh
   set -a; . environments/production.env; set +a
   backstage-aws secrets set-identity --file /secure/oauth.json --approve
   ```

   Expected: exit 0 with `result.secret_arn` and `result.version_id`. There is no CLI command to export secret values or restore an AWS Secrets Manager version.

## Plugin secrets

3. Project settings declare plugin secret names and task-environment mappings; Terraform creates the Secrets Manager resources and ECS injects the values when tasks start. The CLI has no command to write plugin secret values. Populate those resources only through your approved secret-management process; the settings file is not a value store.

## Rotate the database password

4. The application role password is separate from the RDS master credentials. To rotate the application password, run:

   ```sh
   set -a; . environments/production.env; set +a
   backstage-aws db rotate-password --approve
   ```

   Expected: exit 0 and JSON `result.secret_arn`, `result.version_id`, `result.services_restarted`, and `result.timings`. The command stages a random replacement, updates the database role, promotes the exact version, and restarts services in the account. It affects every namespace. An interrupted operation checkpoints under `operations/rotation/active.json`; rerun the same command to resume. Do not delete active checkpoints or secret versions to force a new rotation.

## Backup and restore boundary

5. A database dump contains one selected database in custom format without owner or ACL entries; it contains no OAuth or plugin secret values. Export it to a new local file path using the source namespace settings:

   ```sh
   set -a; . environments/production.env; set +a
   backstage-aws db dump --file ./namespace.dump --approve
   ```

   Expected: exit 0 and JSON `result.bucket`, `result.key`, `result.database`, and `result.file`. To restore the dump as the seed for future namespaces, load the target account's settings and run:

   ```sh
   set -a; . environments/production.env; set +a
   backstage-aws seed --file ./namespace.dump --approve
   ```

   Expected: exit 0 with `result.bucket`, `result.key`, `result.operation_id`, and `result.resumed`. It does not restore an existing namespace, RDS instance, Terraform state, or Secrets Manager values. Keep secure source files for secret values separately; see [Back up and restore](backup-and-restore.md).
