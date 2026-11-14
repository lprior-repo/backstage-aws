# Security overview

GitHub Actions receives short-lived AWS credentials by OIDC, assuming the environment's deploy role. Configure `bootstrap` with the exact repository and GitHub environment. Protect production with required owner review; do not store AWS access keys in GitHub.

Three stacks separate account setup (`bootstrap`), shared durable services (`base`), and fast per-namespace services (`env`). Previews use a separate AWS account from production; namespaces within an account share its PostgreSQL instance and application role. Preview guest sign-in is limited by the configured ALB CIDR allowlist and is not a substitute for stronger network controls.

Fargate services use the same environment stack for production and previews. Database TLS is forced and verified. The RDS master credential is available only to the db-admin task roles. The application credential is outside Terraform state, stored in Secrets Manager under the stack KMS key, and injected through ECS `secrets`. Every secret belongs to the `base` stack; the GitHub deploy role has no Secrets Manager permission and no IAM write permission.

Review the generated Terraform policies, ingress ranges, GitHub environment protection, data supplied to the seed database, and application/plugin permissions before applying. Offline validation is not a security certification or live account evidence.

## Known residual risks

Anyone who can push a branch can change the preview workflow and run anything the preview deploy role allows, including starting a db-admin task with an overridden command. That makes branch pushers effective database administrators of the preview database. Mitigations are a separate preview account, seed data only, and production protection requiring review and deployment from `main`.

The deploy policy has not been exercised against AWS; the first live run may fail with `AccessDenied`. Add only the exact missing action reported. Run IAM Access Analyzer policy validation once in a real account.
