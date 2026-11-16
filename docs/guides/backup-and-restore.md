# Back up and restore

The CLI provides on-demand database dumps and one restore target: the shared seed used by future namespaces. It does not restore an existing namespace, RDS instance, Terraform state, or secret. No live recovery, sign-in, RTO, or RPO has been verified.

Every successful `backstage-aws` command exits 0 and prints one JSON document. Exit 2 means invalid input, 3 blocked/precondition, 4 external or state failure, and 5 timeout; remote work may continue after exit 5.

## Retained data

1. **Project:** Git retains source, Terraform, workflows, and committed non-secret settings. It does not retain database data, secret values, Terraform state, or image layers.
2. **Terraform state:** the private S3 state bucket is versioned and encrypted with its own customer-managed KMS key. The CLI owns and retains the bucket; no lifecycle rule expires its versions. Terraform protects the state KMS key from destroy. This CLI has no state restore command.
3. **RDS:** PostgreSQL 16 is encrypted, deletion-protected, and configured with seven-day automated backups and a final-snapshot request (`<name>-final`). This CLI has no RDS restore or endpoint cutover command.
4. **Database dumps:** `db dump` creates a custom-format dump of one namespace in the versioned, application-KMS-encrypted seed bucket under `backups/<namespace>/`. Current dump objects have no configured expiration; noncurrent versions expire after 30 days. Dumps are on-demand, not continuous.
5. **Secrets and keys:** Secrets Manager has a seven-day deletion-recovery window; values are not stored in Terraform state. The separate application and state KMS keys rotate automatically and have a 30-day deletion window. Preserve the keys and policies.
6. **Images:** ECR retains the 50 newest images; untagged images expire after 14 days. There is no configured cross-account or cross-region replication.

Before remote bootstrap state exists, `bootstrap` retains any state written by its first Terraform apply in `infra/bootstrap/terraform.tfstate`, including after a failed apply. Preserve this private, Git-ignored file and rerun the same command to migrate it into the encrypted, versioned bucket. Do not delete the file, bucket, KMS key, or active locks while recovering an interrupted bootstrap.

## Export a namespace

1. Load the settings for the account and namespace you intend to export. This example uses `production`; use `preview.env` for a preview:

   ```sh
   set -a
   . environments/production.env
   set +a
   install -d -m 700 /secure
   backup_dir="$(mktemp -d /secure/backstage-aws-dump.XXXXXX)"
   backstage-aws db dump --file "$backup_dir/production.dump" --approve
   ```

   Expected: exit 0 and JSON with `ok:true`, `command:"db dump"`, and `result.bucket`, `result.key`, `result.database`, and `result.file`. The file path must not already exist; the CLI writes the dump locally and to the seed bucket.
2. The dump contains the selected database in PostgreSQL custom format, created without owner or ACL entries. Treat both the S3 object and local file as sensitive data. Do not commit or publicly attach them.

## Restore supported by the CLI

3. To make the dump the seed for **future namespaces** in the target account, load that account's settings and run:

   ```sh
   set -a
   . environments/production.env
   set +a
   backstage-aws seed --file "$backup_dir/production.dump" --approve
   ```

   Expected: exit 0 and JSON with `ok:true`, `command:"seed"`, and `result.bucket`, `result.key`, `result.operation_id`, and `result.resumed`. The operation restores into a staging database before replacing the shared seed. Existing namespaces are unchanged.
4. Do not use `seed --file` to recover a running namespace. The CLI does not restore RDS snapshots/PITR, prior Terraform state versions, or Secrets Manager values; the configured AWS retention is not a tested recovery procedure. Retain state buckets, object versions, keys, snapshots, and any secure source for secret values separately.
