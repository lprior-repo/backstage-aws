# Rollback and recovery

## Failed deployment

The ECS circuit breaker is enabled. A failed rollout may cause ECS to restore its prior healthy deployment. The Step Functions check does **not** mistake that replacement deployment for the requested release. Inspect execution history, ECS events, container exit codes and logs before changing anything else.

Release receipts retain the previous task definition and requested digest. Tagged images and prior portal task definitions are retained for deliberate rollback. They accumulate cost/history; retire them only after the agreed rollback window.

## Image rollback

```bash
./platform rollback --digest sha256:PREVIOUS_RETAINED_DIGEST --database-compatible
```

Use an actual 64-character digest. The command scans the selected image, snapshots the current database, reviews/applies its task configuration and rolls it out. The compatibility flag is a human acknowledgement, not a test result. Consult Backstage/plugin migrations first. A snapshot is a recovery point, not proof that an old app can use the current schema.

## Database restore drill

```bash
./platform snapshot
./platform restore-drill --snapshot YOUR_AVAILABLE_SNAPSHOT_ID --new-id backstage-recovery-unique
```

The command starts restoration into an isolated, private RDS instance using the source subnet group, security groups and parameter group, with deletion protection. It does not overwrite the active database or switch traffic. It records `needs verification` and the recovery DB identifier. Restored instances incur charges and are intentionally not part of the active Terraform DB resource.

1. Wait for the restored instance to report `available`; inspect its encryption, TLS, network and backup configuration.
2. Recover/reset its admin credential using the RDS managed-password operation as appropriate. Restored application role verifiers come from the snapshot; the live Secrets Manager application's AWSCURRENT value may have rotated since that snapshot. Do not assume they match.
3. Run an isolated bootstrap and portal task against the restored endpoint using separately scoped recovery credentials, never the active service. Verify users, catalog, plugin schemas and task history. The automated restore command does not claim this drill passed.
4. Measure restoration and validation times and record RTO/RPO evidence.
5. If an actual cutover is required, use a reviewed infrastructure recovery change that adopts/imports the restored DB and updates the endpoint/secret/task bindings. Preserve the original protected DB and KMS key until the recovery is accepted. Do not run an ordinary apply that recreates/destroys the wrong database.
6. Remove an unused drill instance through the normal reviewed decommissioning process after capturing the results; the automation never destroys it automatically.

No general-purpose script can establish application migration/data compatibility from an RDS API success response. Application validation and live cutover remain explicit decisions.

## Credential-maintenance failure

If DB password maintenance fails after draining tasks, leave the service in maintenance. Inspect the exact bootstrap task; ensure no task continues to mutate the DB. Recover a stale release lock only after the execution is terminal, and perform a normal `rollout` to reconcile AWSCURRENT. That path reuses the saved credential instead of generating another one. Verify DB authentication before returning traffic.

## Terraform state and retained keys

Keep bootstrap/core state keys distinct. S3 versioning and lockfiles are enabled. Losing local backend settings is not permission to create a second independent state: restore the backend configuration first. The state bucket and encryption keys use prevent-destroy guards. Retained RDS snapshots, secrets and logs need their KMS key for their entire retention window.

Teardown is intentionally not an unattended destructive command. Use the original protected resource instructions in docs/BASELINE-README.md and your normal infrastructure retirement process. No aws-nuke configuration targets this permanent Backstage account.
