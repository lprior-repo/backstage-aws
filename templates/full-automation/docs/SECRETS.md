# Secret creation, storage, injection and rotation

## Fixed credentials

| Credential | Created by | Read by | Stored in Terraform? |
| --- | --- | --- | --- |
| RDS admin password | RDS managed secret | DB bootstrap/rotation task role only | No plaintext |
| Backstage database password | Bootstrap task, cryptographic random 48-byte base64url | Bootstrap task; portal ECS execution role for startup injection | No plaintext |
| GitHub OAuth ID, client secret, session secret | Real OAuth registration + local template | Portal ECS execution role for startup injection | No plaintext |
| Optional plugin secrets | Operator uploads to created secret container | Portal ECS execution role for startup injection | No plaintext |

Terraform creates secret metadata, KMS keys and secret-ARN references. There are no `aws_secretsmanager_secret_version` resources and no credential-valued tfvars. Step Functions inputs contain only a validated operation enum and request ID; neither workflow carries a password, arbitrary command, role or task ARN.

The ECS execution role obtains application secrets and injects `POSTGRES_USER`, `POSTGRES_PASSWORD`, `AUTH_GITHUB_CLIENT_ID`, `AUTH_GITHUB_CLIENT_SECRET` and `AUTH_SESSION_SECRET`. The running portal task role cannot read Secrets Manager or the RDS admin credential. A process/container compromise can still read its injected environment: environment injection does not protect secrets from the app that needs them.

The separate bootstrap task retrieves the RDS administrator credential at runtime, connects with certificate verification, creates a restricted application principal, revokes public DB access and creates a SCRAM verifier. It persists an application credential before SQL reconciliation so a partial failure can be repaired using the same selected credential. SQL does not contain its plaintext password; protect DB logs and SCRAM verifiers as sensitive.

## Set identity

```bash
./platform secrets-init --file .secrets/github-identity.json
# Fill in real OAuth clientId/clientSecret, retaining generated sessionSecret.
./platform secrets-set --identity .secrets/github-identity.json
./platform rollout
```

Uploads use `--secret-string file://...` and restrict local file permissions. Values are never placed in the command-line argument, stdout or a release record. Store your private file in your normal secret manager; remove the local copy once no longer needed.

`secrets-set` does not restart running tasks. `secrets-rotate` uploads the selected version and then waits for an ECS rollout. Allow overlap between old/new OAuth credentials at GitHub while replacing tasks. Replacing the session secret invalidates sessions; this is not a zero-downtime session-key migration. If a rollout fails, inspect the retained VersionId before retrying and restore the previous AWSCURRENT label if necessary.

## Optional plugin credentials

Add descriptors to `deployment.json`; descriptors contain no values:

```json
{
  "additionalSecrets": {
    "github-integration": {
      "environmentVariable": "GITHUB_TOKEN",
      "jsonKey": "token",
      "description": "Read-only GitHub discovery token"
    }
  }
}
```

Merge this property into the existing config. Run `./platform configure`, review/apply core Terraform, then set the value from a private file:

```bash
./platform secrets-put --name github-integration --file .secrets/github-token.json
# File content: {"token":"your actual token"}
./platform rollout
```

Add the corresponding `${GITHUB_TOKEN}` reference to the relevant Backstage integration configuration before building. The automation does not enable plugins merely by defining an environment variable. Omit `jsonKey` to inject a whole text secret. Reserved environment names and duplicate mappings are rejected. Release preflight verifies an AWSCURRENT version exists for every configured plugin secret, without reading values. For a first deployment with additional secrets, provision infrastructure, upload those values, then run `release`; `up` intentionally stops before build if a configured value is missing.

## Database password rotation

```bash
./platform db-rotate --maintenance-window
```

This creates a DB snapshot, starts a separate JSONata maintenance state machine, acquires the shared release lock, drains the ECS service to zero, runs a fixed credential-rotation task and restarts the approved portal revision. The portal task role cannot start this maintenance state machine.

Rotation changes the application password, not the RDS-managed administrator credential. The task takes a DB advisory lock, validates the existing application role, writes a new secret version and reconciles that version into PostgreSQL. The CLI waits for the workflow and records live-check status.

There is deliberate downtime. If the secret write succeeds but SQL fails, the selected secret is recoverable; leave the portal drained and run a **normal rollout** to reconcile AWSCURRENT without generating another password. If a task is still running after an ambiguous timeout, inspect/stop that specific task and verify DB state before recovery. Do not start a competing rotation.

Secret rotation alone does not change an existing container. Completion requires replacement tasks, working DB authentication and retained validation evidence.

References:
- https://docs.aws.amazon.com/AmazonECS/latest/developerguide/secrets-envvar-secrets-manager.html
- https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/rds-secrets-manager.html
- https://docs.aws.amazon.com/secretsmanager/latest/userguide/manage_update-secret-value.html
