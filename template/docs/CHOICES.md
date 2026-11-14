# Architecture choices

| Concern | Design |
| --- | --- |
| Operator | One Rust `backstage-aws` binary; all orchestration is an explicit CLI command. |
| Account setup | `bootstrap` once per account creates encrypted remote state and GitHub OIDC trust. |
| Shared resources | `base` owns networking, ALB/TLS/DNS, ECS cluster, ECR, PostgreSQL, every secret, seed storage, and db-admin tasks. |
| Environments | `env` owns a namespace's target group, listener rule, log group, task definition, and service. Production and previews use this same stack. |
| Preview isolation | Previews are in an AWS account separate from production. Preview namespaces in one account share a database role. |
| Database | PostgreSQL 16 on a shared account instance. Each environment has a database cloned from the operator-supplied `seed` dump. |
| Identity | Guest sign-in for previews; GitHub OAuth for production. |
| Delivery | GitHub Actions OIDC and short-lived deploy credentials. Image build and seed clone run concurrently on the preview fast path. |

The five-minute PR-to-URL target assumes a warm image cache and has not been measured. No real-account apply or end-to-end deployment has been performed.
