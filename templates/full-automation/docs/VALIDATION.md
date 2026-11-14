# Validation for this automation bundle

This document describes the current bundle, not the inherited baseline's previous validation history. A test pass below is local evidence in its stated scope, not a live AWS result.

## Completed locally

- Terraform 1.17.2 parsed/formatted the bootstrap and core HCL successfully.
- Provider-free Terraform console evaluated the **actual** rollout and maintenance definition expressions into four deterministic test fixtures (published image and infrastructure-only variants). Fixtures use synthetic AWS identifiers and must not be deployed.
- The exact AWS-documented JSONata 2.0.6 evaluator compiled/evaluated the rendered expressions. Twenty-one workflow tests cover JSONata-only declarations, rejected JSONPath fields, input validation, exact task/deployment receipts, rollback not being mistaken for success, bounded observations, conditional lock ownership, isolated maintenance routing and drain-before-rotation behavior.
- Thirty-five Node tests cover existing operator guards, DB bootstrap/SCRAM behavior, rotation persistence/recovery, config/target validation, secret file permissions and file-reference upload, scan gates, lock recovery and CLI release ordering. The real CLI was run against deterministic fake AWS/Terraform executables: a blocked secret/lock/scan prevented snapshot/apply/activation, and a failed remote rollout produced retained blocked evidence after the expected snapshot/apply/start sequence.
- Bash entry-point syntax, automation JavaScript syntax and GitHub workflow YAML parsing passed.
- The official Backstage latest-release API returned v1.56.5; the response is retained under app/provenance. Runtime `latest` performs a fresh lookup rather than trusting this captured response forever.

Logs are under `evidence/preparation/`. Tests with fake APIs do not exercise AWS IAM, eventual consistency, VPC routing, container boot or the Step Functions service implementation.

## Blocked in this environment

The HashiCorp AWS provider 6.73.0 installed and its signed checksum lockfile was retained, but `terraform validate` cannot start the provider here. Its diagnostic identifies `listen unix /tmp/plugin...: socket: operation not permitted`. Therefore **full provider schema validation and Terraform mock-provider tests were not completed locally**. HCL formatting and provider-free definition evaluation do not substitute for these checks.

The protected deployment workflow includes provider validation/mock tests and the AWS `ValidateStateMachineDefinition` API before release activation. Those CI jobs have not been executed by this preparation session. Run `scripts/check-automation.sh` on your normal Linux/macOS host before your first apply.

## Not performed

- No AWS account credentials were used to provision or change resources.
- No real GitHub deployment environment, OIDC role, OAuth registration or secret value was created by this preparation session.
- No Docker daemon is available here. The complete Docker app build, app TypeScript/Jest suite and native PostgreSQL integration were not rerun for this bundle. Only the changed standalone DB/bootstrap Node logic and operator code were executed locally.
- The inherited optional Rust helper was not changed or recompiled in this session; its original provenance is retained separately.
- No live ECR scan, ECS rollout, Step Functions execution, secret injection/rotation, TLS/DNS test, alert delivery, authenticated login, role-policy evaluation, restore drill or load test is claimed.

A release must remain unknown/needs verification until those live checks have evidence. CI failure is a blocker, not an invitation to remove a gate or relax findings/authorization checks without a reviewed change.

## Reproduce

```bash
./scripts/check-automation.sh
# Requires the pinned Terraform provider to start on your machine.
# After account setup and applying the reviewed infrastructure:
./platform validate-workflow
./platform validate-live
```

Before production use, test success, failure, timeout/abort and stale-lock recovery in an isolated account; trigger an alarm and confirm actual delivery; test real GitHub login as both an operator and nonoperator; validate migration compatibility and restored application data. Review the privileged infrastructure role and allowlist/egress policy against your account's requirements.
