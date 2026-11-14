# Recovery and teardown

The CLI emits one JSON document on stdout, NDJSON progress and child output on stderr, and exit codes 0 (success), 2 (invalid input), 3 (blocked/precondition/approval), 4 (external failure), and 5 (remote work continues after timeout). A timeout is not proof that remote work stopped; inspect the reported operation and AWS resource state before retrying.

Terraform, Docker, or Git that exceeds its time limit is stopped and reported as a tool failure (exit 4), as is a job that GitHub cancels or times out. Terraform stopped this way can leave its lock behind, and the next run for that stack then fails with a lock error. Once you are sure no other run is active, delete the lock object, `<state key>.tflock` in the state bucket `<name>-<account>-<region>-tfstate` (for a preview, `env/pr-<number>.tfstate.tflock`), and run the command again.

`bootstrap` decides from the account, not from the checkout, whether it has run before: until the state bucket exists it applies with state in `infra/bootstrap/terraform.tfstate`, then moves that file into the bucket. If it fails part-way, run it again; it continues from the state on disk. It refuses state on disk that belongs to another account.

Every mutation requires `--approve` or `BACKSTAGE_AWS_APPROVE=true`. `env down --approve` is strictly ordered: destroy the namespace Terraform stack if its state object exists, drop its database with `DROP DATABASE IF EXISTS ... WITH (FORCE)`, then delete its state object. A failure stops later actions, and every step is safe to repeat, so finish an interrupted teardown by running the command again. Do not manually delete state to bypass a failed destroy or database drop.

`env up` fails when ECS rolls a deployment back or the load balancer never reports a healthy target. The previous version, if there was one, keeps serving. Read the service events and the task logs in the namespace's log group, fix the cause, and run it again.

For an application-password change, run `backstage-aws db rotate-password --approve`. It stages a new password in Secrets Manager as `AWSPENDING`, sets it on the shared application role through a db-admin task, makes it `AWSCURRENT`, and restarts every environment service in the account. A failure before the last step leaves the role and the current secret unchanged; after any failure, run the command again. Expect brief connection errors during service restarts. Preview and production accounts are separate; namespaces in one account share that account's application role.

The database keeps seven days of automated backups with point-in-time restore, and a final snapshot when the instance is deleted. The CLI has no restore command; restore with RDS to a new instance. The registry keeps the 200 most recent images.

Terraform state and saved plans are sensitive. Preserve the account's encrypted state bucket and KMS key while any stack or retained resource depends on them. Never run competing writers against the same stack state.
