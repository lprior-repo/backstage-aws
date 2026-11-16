# Security overview

GitHub Actions authenticates to AWS with OIDC. Preview role trust is scoped to the configured repository; production is restricted to `ref:refs/heads/main`. Both require the STS audience. There are no GitHub deployment Environments, reviewer resources, or environment-scoped variables. Protect `main`, workflow files, and committed settings through repository policy.

Keep preview and production in separate AWS accounts. Within one account, namespaces share the RDS instance, database application role, and base secrets; a namespace is not a tenant security boundary. Use sanitized seed data only.

The ALB's IPv4 CIDR allowlist gates access; app ingress is ALB-only. RDS stays isolated. Default tasks have public IPs. `fck-nat=true` gives app/admin tasks private IPs behind one shared, single-AZ instance; `nat-gateways=1/2` uses managed NAT. Do not combine them. fck-nat has no SSH key or IAM role; operators own replacement and AMI patch updates.

Database TLS is required and the app verifies the server certificate. RDS manages the master credential; only the db-admin ECS task and execution roles can retrieve it, not the portal task. The application password stays out of Terraform state and enters tasks through ECS secret injection. The CI deploy role cannot read secret values; ECS execution roles retrieve container-injected secrets.

Anyone able to push a branch and change a workflow can exercise the preview role, including overriding a permitted db-admin clone task's command. Treat branch authors as administrators within the preview sandbox. Do not grant the preview role production access.

Terraform IAM policy sources live in each stack's `policies/*.json.tftpl`, with explicit Terraform inputs and JSON-encoded substitutions. Review the rendered plan as well as the template before applying permission changes. Keep owner-managed `access` changes separate from deployment, and avoid out-of-band IAM edits that drift from Terraform.

Terraform state, saved plans, database dumps, logs, snapshots and secret exports can contain sensitive data. Preserve the state and application KMS keys and their policies. State/dump versioning is not an independent cross-account backup or key backup.

Review actual IAM permissions, trust, quota/capacity, seed contents and ingress in a sandbox. AWS-independent checks are not a security certification. AWS policies, OIDC, TLS, deployments and restore paths have not been live-tested.
