# Seed database

The shared `seed` database is cloned only when a namespace is first created. Replacing the seed does not change existing preview or production databases.

Every successful `backstage-aws` command exits 0 and prints one JSON document. Exit 2 means invalid input, 3 blocked/precondition, 4 external or state failure, and 5 timeout; remote work may continue after exit 5.

## Create a dump

1. To export an existing namespace, load the settings for its account and run `db dump` with a new local file path:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws db dump --pull-request 42 --file ./pr-42.dump --approve
   ```

   Expected: exit 0 and JSON `result.bucket`, `result.key`, `result.database`, and `result.file`. The task runs `pg_dump` inside the VPC and writes `backups/<namespace>/<timestamp>-<operation-id>.dump` to the versioned, application-KMS-encrypted seed bucket. The local file is created atomically and an existing destination is not overwritten.
2. To create a dump outside the CLI, use PostgreSQL custom format without owner or ACL entries:

   ```sh
   pg_dump --format=custom --no-owner --no-acl --file=./source.dump "$SOURCE_DATABASE_URL"
   ```

   Expected: `pg_dump` exits 0 and creates `source.dump`. The Fargate restore task uses PostgreSQL 16 tooling; verify source/destination versions and extensions. Dumps may contain sensitive data; sanitize, encrypt, and restrict access. Never commit or attach them publicly.

## Restore the shared seed

3. Load settings for the account whose shared seed you intend to replace and run:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws seed --file ./source.dump --approve
   ```

   Expected: exit 0 and JSON `result.bucket`, `result.key`, `result.operation_id`, and `result.resumed`. The CLI uploads a pinned dump under `operations/seed/<operation-id>.dump`, restores into an empty staging database, and replaces the seed after restore succeeds. Schemas absent from the new dump do not survive in the seed.
4. Interrupted or failed restores retain `operations/seed/active.json`. Rerun the approved `seed --file` command to resume the pinned operation; a newly supplied file does not replace the active operation's input. Do not delete the checkpoint or object versions to bypass a failure.

   If ECS has expired the original task record, the CLI runs a read-only database reconciliation, not another restore. Completion requires the matching live database receipt and no staging database, or the unambiguous previous/live topology from older releases. An unknown outcome returns exit 3 and retains the checkpoint and pinned dump for inspection; it never treats missing task metadata as failure or authorizes another cutover.

## Scope and retention

5. Seed restore affects only namespaces created after the replacement. There is no CLI command to import a dump into an existing namespace, restore an RDS snapshot, or restore Terraform state. Current dump objects have no configured expiration; noncurrent seed-bucket versions expire after 30 days. See [Back up and restore](backup-and-restore.md) for the recovery boundary.
