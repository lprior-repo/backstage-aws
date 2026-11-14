# Research and upstream references

Verified 2026-11-13 UTC. These are primary project or vendor sources. Version
claims apply to this date; the source bundle deliberately pins versions rather
than resolving `latest` during deployment.

## Existing repositories: this pattern already exists

| Project | What it supplies | How this starter uses it |
| --- | --- | --- |
| [AWS IA ECS Blueprints](https://github.com/aws-ia/ecs-blueprints) | AWS-maintained ECS examples. Its catalog includes a Terraform Backstage + Aurora example that uses a shared ECS cluster, and separate Step Functions/ECS patterns. | Architecture reference. It is the strongest vendor-owned starting point to investigate if you prefer an existing large examples repository. The repository describes community, best-effort support. |
| [SourceFuse ARC Backstage ECS app](https://sourcefuse.github.io/arc-docs/arc-iac-docs/modules/arc-backstage-ecs-app/) | A reusable Terraform ECS service module with ALB, Route53, secret wiring, and autoscaling. Requires upstream database/service account, ECS cluster, HTTPS listener, and image. | Dependency/coverage comparison. Its published documentation still shows Terraform ~1.3, AWS provider ~4.0, and an old default image, so adopting it still requires integration and version review. |
| [StratusGrid walkthrough](https://github.com/StratusGrid/backstage-aws-walkthrough) | A working topology reference with private Fargate tasks, public ALB, and PostgreSQL RDS. | Reference only. The project explicitly describes evaluation use, guest authentication, and additional work for production. No unlicensed source was copied into this starter. |
| [AWS Labs Backstage plugins](https://github.com/awslabs/backstage-plugins-for-aws) | AWS catalog/resource plugins and scaffolder actions. | An extension path for ECS/ECR catalog views and EventBridge actions. The initial app uses a small fixed-target Step Functions action so it can restrict the exact workflow and reconcile submission retries. |

An existing repository can remove initial plumbing work. It does not remove the
need to build a current Backstage application, configure trusted login, define
runtime permissions, initialize the database correctly, and choose a deployment
workflow. This source bundle combines those parts into one small deployable
project. It is an original integration built from the official application
generator; it is not presented as an upstream-supported AWS product.

## Release provenance

| Component | Pin | Primary source |
| --- | --- | --- |
| Backstage | 1.56.5 | [Official release](https://github.com/backstage/backstage/releases/tag/v1.56.5), published 2026-11-12. The release manifest is saved under `app/provenance/`. |
| Backstage generator | 0.9.3 | [npm registry metadata](https://registry.npmjs.org/@backstage%2fcreate-app/0.9.3). Package versions are selected from the Backstage release manifest, not all set to `1.56.5`. |
| Terraform CLI | 1.17.2 | [Official release](https://github.com/hashicorp/terraform/releases/tag/v1.17.2). |
| AWS Terraform provider | 6.73.0 | [Official release](https://github.com/hashicorp/terraform-provider-aws/releases/tag/v6.73.0). |
| Node | 24.22.1 | Exact Node 24 release matching host and image; [official images](https://hub.docker.com/_/node). |
| Yarn | 4.13.0 | `app/package.json` and checked-in Yarn executable/configuration. |
| Optional Rust helper | Rust 1.100.0 | [Official release announcement](https://blog.rust-lang.org/2026/11/12/Rust-1.100.0/). |
| PostgreSQL | RDS PostgreSQL 16 | A major-version pin; RDS selects an available minor and applies compatible maintenance upgrades. This is intentionally not a claim to pin one worldwide minor build. |

The CI-only PostgreSQL 16 service is pinned to the official Docker manifest
`sha256:63e83b5a3dc36a917dccb76fe1bd2ac51f1ec9c1deb47ff20d4f04233bf6eae9`,
resolved from [the official image tag metadata](https://hub.docker.com/v2/repositories/library/postgres/tags/16)
on the verification date. This does not select the RDS minor version.

## Design decisions and their sources

1. **One application image, port 7007.** Backstage's backend serves the frontend
   bundle, so a separate frontend service is unnecessary for this deployment.
   See [production deployment](https://backstage.io/docs/golden-path/deployment/)
   and [Docker deployment](https://backstage.io/docs/deployment/docker/).
2. **Private managed PostgreSQL.** Persistent storage is required beyond an
   ephemeral evaluation. Schema-based plugin isolation allows one database
   without granting the application permission to create databases. See
   [PostgreSQL configuration](https://backstage.io/docs/tutorials/switching-sqlite-postgres/).
3. **PostgreSQL search.** Reuse the existing database rather than add OpenSearch
   solely for an initial small portal. See
   [search engines](https://backstage.io/docs/features/search/search-engines/).
4. **Separate RDS administrator and application credentials.** RDS can manage
   and rotate its administrator secret. ECS-injected environment values do not
   refresh in running tasks when a secret changes. See
   [RDS-managed credentials](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/rds-secrets-manager.html)
   and [ECS secret injection](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/secrets-envvar-secrets-manager.html).
5. **Catalog-backed identity and explicit permissions.** OAuth configuration
   alone is not authorization. The numeric GitHub identity must map to a trusted
   catalog User, and the permission policy controls who can start operations.
   See [GitHub authentication](https://backstage.io/docs/auth/github/provider/),
   [identity resolvers](https://backstage.io/docs/auth/identity-resolver/), and
   [permission setup](https://backstage.io/docs/permissions/getting-started/).
6. **Standard Step Functions with a fixed ECS task.** The optimized
   `ecs:runTask.sync` integration waits for the database job. The workflow then
   calls ECS UpdateService/DescribeServices and verifies its own deployment ID.
   See [ECS integration](https://docs.aws.amazon.com/step-functions/latest/dg/connect-ecs.html).
7. **Explicit submission idempotency.** A stable execution name and payload
   enable retry reconciliation; a closed Standard execution cannot simply be
   started again with that name. See
   [StartExecution API](https://docs.aws.amazon.com/step-functions/latest/apireference/API_StartExecution.html).
8. **Native S3 state locking.** Use encrypted, versioned S3 state with lockfiles,
   and no extra DynamoDB dependency. See
   [Terraform S3 backend](https://developer.hashicorp.com/terraform/language/backend/s3).

## Deliberate boundaries

The included Step Functions workflow deploys this Backstage portal. It does not
run arbitrary Terraform from a user-supplied repository or provision arbitrary
AWS accounts. A general Terraform service needs a separate design for trusted
modules, plan/apply binding, remote-state tenancy, approval semantics, artifact
integrity, target roles, cancellation, and cleanup. Adding those capabilities to
the portal's IAM role would substantially increase its exposure.

TechDocs, Kubernetes, CloudFormation, CodeBuild, Control Tower, OpenSearch,
cross-account vending, and repository write tokens are outside this initial
runtime. Native Node dependencies may use third-party build tools during image
construction; there is no authored Python application or deployment runner.

