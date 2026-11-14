# Back up and restore

Configured recovery paths are not a tested disaster-recovery service. No live AWS restore has been exercised; no RTO/RPO is promised. Rehearse in an isolated account.

## Recovery inventory

| Plane | What is retained | Recovery boundary |
| --- | --- | --- |
| Git/project | Versioned repository: app, Terraform, workflows, committed non-secret settings | Does not contain Terraform state, data, secret values or image layers |
| Terraform state | Private, versioned, encrypted S3 state bucket; separate customer-managed KMS key | S3 versions recover overwritten/deleted objects while retained; not an independent account backup. Preserve bucket, versions, key and policy. |
| RDS | Encrypted DB; seven-day automated backups/PITR, deletion protection, final-snapshot request | Restore creates a new DB instance; final snapshot is not tested. |
| Database dumps | `db dump` writes custom-format dumps to the versioned seed bucket under `backups/<namespace>/`; noncurrent objects expire after 30 days | On-demand only. `--file` downloads a new owner-readable local file. `seed --file` targets seed, never an existing namespace. |
| Secrets | Secrets Manager versions; seven-day deletion recovery window | State has metadata, not secret values. Recreate/regenerate values where possible. |
| KMS | Separate state and application customer-managed keys; automatic rotation, 30-day deletion window | Preserve both keys, policies and access. Losing a key can make ciphertext irrecoverable. |
| ECR | Lifecycle policy retains the 50 newest images | No cross-account/region replication. Retained image digest can be deployed; otherwise rebuild from source. |

The seed bucket stores dumps; the state bucket stores Terraform state. Neither is a backup of the other. Confirm live retention, key state and access before relying on them.

## Export data and secrets

```sh
backstage-aws db dump --namespace production --file /secure/production.dump --approve
umask 077
aws secretsmanager get-secret-value --secret-id "$SECRET_ARN" \
  --query SecretString --output text > /secure/secret-backup
```

Repeat secret export for identity, session and plugins; encrypt offline copies. Regenerate RDS master credentials after account recovery; rotate/reconcile the application role password. Current dump objects persist indefinitely; **only noncurrent versions** expire after 30 days.

## Recover state

Stop every writer first. S3 Console → state bucket → **Show versions**. For accidental deletion, delete only the object's delete marker. For overwrite, copy a verified previous version to current:

```sh
aws s3api copy-object --bucket "$STATE_BUCKET" --key "$STATE_KEY" \
  --copy-source "$STATE_BUCKET/$STATE_KEY?versionId=$VERSION_ID"
```

Use a URL-encoded version ID. Preserve the state KMS key and compare a Terraform plan with actual resources before applying. Restoring state does not roll back AWS resources.


## Restore an RDS instance (AWS Console)

Use the correct account and region. Restore a **new** instance; keep the source intact.

**Point-in-time recovery**

1. Open **RDS → Databases**, select the source DB, then **Actions → Restore to point in time**.
2. Choose **Latest restorable time** or **Custom** and select the recovery time. Enter a unique DB instance identifier.
3. Review instance class and storage; under connectivity choose the intended VPC, DB subnet group and security groups. Preserve encryption with the original application KMS key.
4. Choose **Restore to point in time** and wait in **RDS → Databases** until the new instance is **Available**. Record its endpoint and validate the data.

**Snapshot restore**

1. Open **RDS → Snapshots**, select the verified manual/final snapshot, then **Actions → Restore snapshot**.
2. Enter a unique DB identifier; review instance class, VPC, subnet group, security groups and original KMS key.
3. Choose **Restore DB instance**; wait for **Available**, then validate the endpoint and data.

Console labels/options vary by AWS release and engine. Check current AWS guidance and your change controls.

## Cutover limits

A restored DB does not update Terraform outputs, ECS task configuration, Secrets Manager, namespace databases, DNS or services. Freeze writes; preserve the original; validate restored data; then plan a reviewed endpoint/credentials/security-group change and restart services. The CLI has **no** RDS restore, endpoint migration, namespace restore or cutover command.

A dump can refresh only the shared `seed`:

```sh
backstage-aws seed --file ./backup.dump --approve
```

This affects future namespaces, not existing ones. Namespace dump recovery needs an operator-managed `pg_restore` path inside the VPC, to a verified target, with traffic stopped and a separately reviewed cutover. Do not infer a namespace restore CLI option or edit Terraform state manually.

## Protect and rehearse

- Keep repo access, account/region, state bucket/key, both KMS ARNs/policies and break-glass access in a protected operator inventory.
- Restrict dumps, state, snapshots and recovery credentials as sensitive production data.
- Monitor RDS backup status, S3 versions, key state/policy and ECR retention.
- Set your own recovery objectives; practice data validation and cutover in isolation.
