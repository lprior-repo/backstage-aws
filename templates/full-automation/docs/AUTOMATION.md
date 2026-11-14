# Operator workflow

## Execution sequence

`configure` validates the supplied account, region, DNS name, numeric GitHub owner and explicit ingress ranges, writes ignored Terraform inputs, and updates the trusted catalog identity. The scripts refuse root credentials and credentials for another account. `doctor` verifies local tools, the AWS identity and root MFA. DNS zone delegation, OAuth registration and required AWS permissions remain real prerequisites.

`up` calls the interactive provisioning script, migrates the bootstrap state to its separate remote S3 key, applies core Terraform, exports a nonsecret target, uploads identity credentials and enters `release`. A newly created service has zero tasks, avoiding an attempt to pull an image or credential that does not yet exist.

`release` performs these steps in order:

1. Verify account, identity/plugin-secret version metadata, service receipt and absence of an existing workflow lock.
2. Build/push the selected source commit, or use a requested existing digest from this stack's ECR repository.
3. Request/check ECR scanning with bounded observations. Missing/unsupported scans, stale/missing timestamps and findings exceeding configured limits block deployment. Default HIGH and CRITICAL limits are zero; edit thresholds explicitly if your reviewed risk policy differs.
4. Create and wait for an available RDS snapshot when the service is already in use.
5. Write the immutable digest, create a saved Terraform plan and apply that plan only after local approval or the protected CI environment approval.
6. Confirm Terraform selected the intended digest and refresh the fixed task/workflow receipt.
7. Start Step Functions and wait for the requested task revision. The workflow acquires its conditional release lock, bootstraps DB access, updates ECS, and requires the **exact deployment ID** to stabilize.
8. Record HTTP/ECS/ALB/RDS/notification checks and outstanding human checks as evidence.

All state machines declare `QueryLanguage = "JSONata"`. AWS requests have fixed resource arguments; the only dynamic DynamoDB argument is the current execution owner. JSONPath state fields and nested JSONPath overrides are rejected by automated tests. Workflow definitions are published as immutable AWS versions; the operator records the published version and task revision for each release. The running workflows use the approved current definition; saved execution history and version receipts retain provenance.

## Status meanings

| State | Meaning |
| --- | --- |
| complete | The named check returned the expected result in its stated scope |
| blocked | A real command/check failed or an expected receipt is missing |
| unknown | No result exists yet |
| needs verification | Work/result exists but required validation remains outstanding |

An ECS rollout can succeed while full release validation remains incomplete. The CLI never calls missing login, authorization or restore tests successful. Evidence JSON contains resource references and status, not secret values. It still contains nonpublic infrastructure identifiers: share it only with authorized operators.

## Concurrency and failure

The state machines use a DynamoDB conditional item keyed `portal`, owned by the exact execution ARN. They never expire a live owner's lock automatically. Success and handled failure release the lock only if ownership still matches. Global workflow timeout/abort, permission errors, or an ambiguous task can leave the lock for explicit recovery:

```bash
./platform recover-lock --execution EXACT_OWNER_EXECUTION_ARN
```

Recovery refuses active executions or an ECS deployment still in progress, then deletes using an owner condition to protect a newer lock. Before invoking it, inspect stopped/running bootstrap tasks too; an ECS `.sync` timeout does not establish that external work has stopped. Local CLI operations also use a local directory lock. After a local crash, inspect `.operator-lock/owner.json` before removing the directory.

The GitHub deployment workflow uses one concurrency group without cancellation. A workflow lock serializes ECS releases/maintenance; it is not a universal lock on Terraform or administrator API calls. Do not apply infrastructure from an independent process while an execution is active. The CLI checks before apply, but this check is not an atomic reservation across unrelated writers. Keep one infrastructure writer per environment.

The infrastructure role's fixed task-revision policy is updated by Terraform. Do not change it under an in-flight older workflow. Narrow role policies, saved plans and environment approvals cannot substitute for this single-writer rule.

## GitHub configuration

`github-setup` creates the exact protected environments referenced by the OIDC subjects, adds a `main`-only deployment branch rule, and writes nonsecret repository variables. It uses your authenticated GitHub CLI session and requires repository administration rights. It refuses to silently downgrade unsupported protection features. Review and protect the source branch through your normal GitHub policy as well.

The image builder cannot read Terraform state or secrets. The infrastructure role can manage the dedicated account's service control plane, core state and fixed app IAM roles, but cannot mutate bootstrap/CI IAM roles through its explicit IAM action grant. It is still powerful: it can deploy code that uses application task roles. Treat anyone able to approve and change infrastructure source as privileged.

CI scans/builds once and deploys the retained digest; staging and production should use different accounts or unique name/state keys and distinct protected environments. Never rebuild a promotion artifact. No automatic first-account creation or organization membership changes are performed.

## Upgrade

`latest` performs a bounded lookup against the official Backstage releases API and records the exact version/URL/date. `upgrade` requires a clean Git tree, creates an upgrade branch, runs the official Backstage CLI `versions:bump --release`, rechecks the lockfile, types and app tests, then leaves the changes for review. It does not silently upgrade a live deployment. Docker builds use the checked-in app lockfile. Update runtime/provider pins and image digests deliberately through review.

The low-level `scripts/*.sh` helpers are retained for compatibility; use `platform` for the full scan/snapshot/evidence path. Existing direct AWS/IaC access can bypass any operator wrapper, so enforce your deployment ownership policy outside this repository too.
