# Tear down

`env down` permanently deletes the selected namespace's service and database. It does not delete shared VPC, ALB, RDS, seed, ECR, state bucket, or KMS resources.

Every successful `backstage-aws` command exits 0 and prints one JSON document. Exit 2 means invalid input, 3 blocked/precondition, 4 external or state failure, and 5 timeout; remote work may continue after exit 5.

## Remove one preview namespace

1. Load the correct account settings, check the target, and export any data to a new local file path before teardown:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws env status --pull-request 42
   backstage-aws db dump --pull-request 42 --file ./pr-42.dump --approve
   ```

   Expected: `env status` exits 0 with JSON `result.namespace`, `result.url`, `result.service_status`, `result.running_count`, `result.image_digest`, and `result.ready`. `db dump` exits 0 with `result.bucket`, `result.key`, `result.database`, and `result.file`. The custom-format dump contains that namespace's database without owner or ACL entries.
2. If you intend the data to seed future namespaces, load the target settings and run this before removal:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws seed --file ./pr-42.dump --approve
   ```

   Expected: exit 0 with JSON `result.bucket`, `result.key`, `result.operation_id`, and `result.resumed`. This updates only the shared seed; it does not restore an existing namespace.
3. Remove the namespace:

   ```sh
   backstage-aws env down --pull-request 42 --approve
   ```

   Expected: exit 0 and JSON `result.namespace`, `result.destroyed`, and `result.comment_url`. The CLI destroys the namespace stack, drops its database, then deletes the current state object. The versioned S3 bucket retains prior object versions; the CLI has no state restore command. If `destroyed` is false, no namespace state was removed.
4. Confirm cleanup with `backstage-aws env list`. Expected: exit 0 with JSON `result.namespaces` no longer containing `pr-42`. The generated GitHub close workflow does the same teardown for eligible same-repository PRs; it checks out the base branch and never executes PR code.

## Production and account removal

5. Do not use `env down` as production rollback or a pause; it deletes the production namespace database. Before any deliberate production removal, confirm the target account and a retained data export. The supported restore is `seed --file`, which affects future namespaces only.
6. There is no one-command account decommission. The CLI does not restore RDS snapshots, prior Terraform state versions, or secrets. Keep shared resources, state versions, KMS keys and policies, and AWS backup retention intact until an operator-led decommission plan accounts for them.

## Timeout and concurrency

7. Serialize all operations for a namespace and never cancel an active apply. If a command exits 5, remote work may still be running: inspect that operation's AWS/Terraform state before retrying. Do not delete state, active checkpoints, or locks to bypass a failed operation. See [Troubleshooting](../troubleshooting.md).
