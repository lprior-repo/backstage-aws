# Type contracts

The names and source paths below match the current implementation. Persisted DTO compatibility is separate from core validity.

| Value/state (source) | Construction and invariant |
|---|---|
| `OperationId` (`db/operation.rs`) | UUIDv4; generated once per operation and validated when deserialized |
| `TaskAttemptId` (`db/operation.rs`) | UUIDv4 distinct from `OperationId`; identifies one ECS attempt |
| `SecretVersionId` (`db/operation.rs`) | Parsed value is 32–64 bytes; the replacement version ID is the operation UUID text |
| `SeedDumpKey` (`db/operation.rs`) | Derive from `OperationId`; persisted key must match that operation |
| `Checkpoint<T>` (`db/operation.rs`) | Record plus required nonempty S3 version ID and ETag |
| `TaskAttempt` (`db/operation.rs`) | `Ready`; `InFlight { attempt_id, launch: Reserved|Submitted(task_arn) }`; `Failed { attempt_id, task_arn }`; or `Succeeded { attempt_id, task_arn }` |
| `RotationState` (`ops/rotation/state.rs`) | `Prepared / BaselinePinned / SecretStaged / RoleChange / SecretPromoted / ServicesRestarted / Completed`; states after `BaselinePinned` carry the prior `SecretVersionId` |
| `SeedState` (`db/seed.rs`, private) | `Active { object_key, attempt }` or `Completed { object_key, task_arn }` |
| `ApplicationCheck` (`check.rs`) | `None / Build / Runtime { postgres_bin }` |
| `CheckRequest` (`check.rs`) | `Project { application, release_archive }` or `Release { archive }` |

Rotation state transitions are adjacent enum transitions, not ordinal comparisons. A CAS conflict returns the newly loaded state for re-evaluation, not permission to overwrite it. (See `ops/rotation/state.rs` and `ops/rotate.rs`.)

CLI booleans and strings are boundary DTOs. Construct the semantic `CheckRequest` once: runtime includes app checks; `postgres-bin` is legal only with Runtime; standalone release requires an archive. The execution core accepts `CheckRequest`/`ApplicationCheck` rather than independent booleans. Offline evidence is listed in `traceability-matrix.jsonl`; live AWS behavior remains unverified. (See `cli.rs`, `check.rs`, and `local_check.rs`.)

Private dump staging owns a temporary file until atomic no-clobber publication. Child-process ownership spans launch, stdin, output, wait, timeout, and cancellation. Unix process-group cleanup is implemented; non-Unix cleanup only targets the direct child. A release-validation child is trusted executable code, not sandboxed data. (See `ops/dump.rs` and `process.rs`.)
