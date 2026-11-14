# Tear down

Mutations require `--approve` (or `BACKSTAGE_AWS_APPROVE=true`). Confirm account and namespace: `env down` permanently drops that namespace's database.

## Remove one preview

The generated trusted close workflow runs this for eligible same-repository PRs. Manually:

```sh
set -a; . environments/preview.env; set +a
backstage-aws env status --pull-request 42
backstage-aws env down --pull-request 42 --approve
```

The CLI destroys the namespace stack, drops its database, then deletes its state object. A failure stops later steps; rerun `env down` to resume. Do not delete state to bypass a failed destroy/drop. Export wanted data first; see [Back up and restore](backup-and-restore.md).

## Production

Production `env down` deletes its service and database; it is not rollback or a safe pause. Shared resources remain. Confirm account, backup and recovery plan before any deliberate removal.

## Full account decommission

There is no one-command teardown. Stop CI and writers; inventory namespaces/data; export dumps and snapshot RDS; preserve state bucket, versions, KMS keys and access. Then use an operator-led, dependency-ordered Terraform/resource decommission. Handle RDS deletion protection, final snapshots, Secrets Manager recovery, DNS, and image retention deliberately. State is needed to identify managed resources; deleting it first strands resources. Account-baseline audit resources may be account-wide; do not remove unrelated resources.

## Stale lock or timeout

Never run concurrent Terraform writers for one stack. For a stale `.tflock`, first prove no run is active for that exact stack; then remove only its lock object from the state bucket and retry. A timeout may leave remote work running—inspect Terraform, ECS and RDS before retrying. See [Troubleshooting](../troubleshooting.md).
