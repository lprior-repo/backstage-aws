# Recovery and teardown

The CLI returns one JSON document and a nonzero exit on failure. Progress and raw child logs use stderr. Exit 5 means remote polling timed out, not that work stopped. A child-process timeout is exit 4; Terraform may leave a lock behind.

## Interrupted provisioning

Bootstrap starts with `infra/bootstrap/terraform.tfstate` and migrates it to the encrypted S3 bucket `<name>-<account>-<region>-tfstate`. Preserve local state after a partial bootstrap; retry with the same account/settings. It refuses local state for another account. Never delete state to bypass an error.

Never clear a lock while another writer might still be active. After confirming no local or CI process is using that exact state, an operator can remove its `<state-key>.tflock` object and retry. Prefer a reviewed recovery operation; do not automate lock deletion on failure.

## Namespace removal

`env down --approve` destroys the namespace stack, drops its database with active sessions disconnected, and deletes the current state object. It stops on failure; repeat to finish an interrupted removal. Versioned state can remain recoverable, but restoring state does not restore a dropped database.

Shared base/bootstrap resources are not removed by the CLI. Before decommissioning, stop CI, export needed data, preserve keys/state, and deliberately handle ALB/RDS deletion protection, final snapshots, nonempty buckets and `prevent_destroy`.

## Database and image recovery

RDS has seven-day automated backups/PITR, deletion protection and a final-snapshot setting. `db dump --approve` exports the selected namespace into the encrypted versioned seed bucket; `--file` also downloads it to a new owner-readable file. No scheduler is configured. Current dumps persist; replaced/deleted versions expire after 30 days.

`seed --file DUMP --approve` restores only the template for future namespaces, not an existing namespace. Full namespace/PITR restoration and endpoint cutover require an explicit operator recovery procedure. Preserve both KMS keys; retained ciphertext is unusable if its key is destroyed.

Seed replacement uses an immutable operation-specific dump and an empty staging database. It preserves the live seed if restoration fails before cutover. An interrupted approved rerun resumes the checkpoint's operation and dump, not a newly supplied file. Ambiguous database cutover state fails closed; inspect it rather than deleting databases or checkpoints.

ECR retains fifty recent images, not every deployed release forever. Preserve release images separately if rebuilding from source is insufficient.

## Password rotation

`db rotate-password --approve` pins the previous version, stages a replacement under its operation ID, alters the PostgreSQL role using that exact version, promotes `AWSCURRENT`, then restarts services. Failures may leave the database changed before promotion/restarts complete. Inspect the failed step and rerun; expect brief connection errors. Do not assume the old credential remains valid after every failure.

Seed and rotation keep versioned checkpoints under `operations/seed/active.json` and `operations/rotation/active.json` in the seed bucket. Approved reruns resume an active operation. Uncertain task launches reuse their persisted attempt; only an observed terminal failure permits a fresh attempt on a later explicit invocation. Completion remains a durable conditional checkpoint update: never delete an active record or its versions to bypass an error.

Full [backup and restore procedures](https://github.com/lprior-repo/backstage-aws/blob/main/docs/guides/backup-and-restore.md). No AWS restore or teardown has been live-verified.
