# Workflows

## Checkpoint acquisition and completion

Absent -> conditional create Active. Completed -> conditional replace Active using current ETag. Active -> resume that exact operation. Every mutation uses the current ETag. Conflict -> reload and re-evaluate; no stale continuation. Active -> Completed only after successful domain work. Completed remains current; no DeleteObject release. Unknown schema, malformed state, or contradictory identity -> fail closed.

## Rotation

Prepared -> BaselinePinned -> SecretStaged -> RoleChange -> SecretPromoted -> ServicesRestarted -> Completed.

Pin the current version before staging. Read/create the replacement only by operation-pinned VersionId. RoleChange owns its task attempt. Promote that exact version and remove only the pinned predecessor stage. Recognize already-completed external effects when resuming. Restart services before completion. A failed effect leaves the latest valid Active checkpoint recoverable.

## ECS attempt

Ready -> InFlight(token) is checkpointed before RunTask. Lost response, poll timeout, or RUNNING preserves the token. Observed STOPPED with all containers successful -> Succeeded. Observed terminal nonzero -> Failed and return failure. Only a later explicit approved invocation may CAS Failed -> InFlight(new token), while preserving operation/input/version identity. No automatic retry loop and no guessed success.

## Seed replacement

Pin the immutable dump object and operation identity. Restore into `seed_stage_<operation-id-without-hyphens>` with a transactional restore before touching live seed. Cut over through `seed_previous_<operation-id-without-hyphens>`. Catalog witnesses govern crash recovery: backup+missing live+stage -> promote; backup+live+no stage -> complete; backup+missing live+no stage -> fail preserving backup; backup+live+stage -> ambiguous, fail closed. Active -> Completed requires confirmed restore task success.

## Local dump and process lifecycle

Download -> private same-directory temporary file -> complete write/sync -> atomic no-clobber publish. Any error/cancellation removes unpublished temporary output. Process launch -> concurrent bounded input/output -> exit observation and cleanup under one deadline. On Unix, cancellation terminates the owned process group; other targets currently clean up only the direct child.

Source boundaries: `db/operation.rs`, `db/seed.rs`, `db.rs`, `ops/rotation/state.rs`, `ops/rotate.rs`, `ops/dump.rs`, and `process.rs`. These are source-level transitions, not formal proofs or live recovery acceptance; S3/ECS/Secrets Manager/RDS behavior remains unverified.
