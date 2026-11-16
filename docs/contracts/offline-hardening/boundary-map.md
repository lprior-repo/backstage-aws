# Boundary map

| Boundary | Pure decision | Imperative owner / risk |
|---|---|---|
| CLI -> core | Parse names, IDs, approved request and check mode | cli/settings/onboarding; hostile args/env |
| S3 -> operation | Validate schema/identity/state; derive legal next transition | `db/operation.rs`, `db/seed.rs`, `ops/rotation/state.rs`, `ops/rotate.rs`; version history and ETag CAS |
| Secrets Manager | Select pinned version and legal stage transition | ops/rotate; CLI plaintext memory only; remote task-scoped secret volume, sanitized errors |
| ECS | Classify observed attempt terminal/uncertain outcome | db; idempotency token and bounded polling |
| PostgreSQL | Select catalog recovery action | generated base scripts; real database transaction/rename semantics |
| Terraform | Validate ownership and select stack inputs | terraform; private operation directories and bounded state reads |
| Local file publication | Decide no-clobber completion | ops/dump; private temp ownership, sync and atomic persistence |
| Child execution | Classify status/output/timeout | `process.rs`; `process-wrap` process-group supervision and descendant cleanup are Unix-only; cancellation guard |
| Release archive | Validate members/limits/expected protocol | release_check; filesystem parser and trusted executable boundary |
| Runtime check | Evaluate actual readiness/isolation/restore outcomes | local_check; PostgreSQL 16 and built Backstage application |

State-transition modules encode domain decisions. `RotationOperation::new` currently generates UUIDv4 in `ops/rotation/state.rs`; AWS, clock, process, and filesystem effects are handled by adapters. Async cancellation is an ownership boundary. On Unix, maintained `process-wrap` handles group control; elsewhere cleanup targets the direct child only. Transitive native process APIs are a trusted platform boundary, not a Rust proof.

Secret sidecar delivery follows [AWS ECS guidance](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/specifying-sensitive-data.html#security-secrets-management-recommendations-mount-secret-volumes): owner-only creation before writing, consumer read-only mount, task-scoped shared volume automatically encrypted on Fargate and deleted when the task stops. This narrow remote-volume exception does not permit local host or persistent password files. SQL failure paths must not echo expanded secret-bearing statements.

Offline mocks/scripts establish local behavior only. They do not establish AWS IAM correctness, RDS cutover acceptance, GitHub sign-in, disaster recovery, or startup-time SLOs. Native release execution is not archive provenance verification; install attestation is a separate boundary.
