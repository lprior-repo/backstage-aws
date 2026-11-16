# Offline hardening domain model

Status: current source reconciled; offline evidence is recorded in `traceability-matrix.jsonl`. Unix process-group descendant cleanup is implemented; non-Unix cleanup only targets the direct child. This is not a proof or review approval.

## Ubiquitous language

| Term | Meaning |
|---|---|
| Operation | One approved seed replacement or password rotation, identified independently of ECS task attempts |
| Task attempt | One persisted ECS idempotency token; uncertain outcomes reuse it |
| Active record | Latest S3 checkpoint for one operation; an aggregate, not a disposable mutex |
| Completed record | Durable terminal checkpoint; never delete a version to release an operation |
| Baseline | Secret version observed and persisted before staging its replacement |
| Staged seed | Fully restored isolated database, not yet the live seed |
| Published dump | Complete private local file installed without overwriting an existing destination |
| Check scope | Project validation, application validation/runtime, or standalone release validation |

## Aggregates and policies

Seed and rotation records own validated operation identity, immutable input identity, lifecycle state, and task-attempt state. `RotationOperation::new` generates its UUIDv4 in `ops/rotation/state.rs`; AWS, clock, process, and filesystem effects are handled by adapters. S3 ETag is optimistic-concurrency authority; object version is history, not release authority. (See `db/operation.rs`, `db/seed.rs`, and `ops/rotation/state.rs`.)

Commands: acquire/resume, pin baseline, stage payload, launch/observe attempt, promote payload/seed, restart services, complete, publish dump, validate project/release. Events are observations or successful checkpoints, not assumed success after issuing a request.

A rerun with an Active record resumes its pinned operation even if a new seed file was supplied. A new operation may replace only a Completed record. Concurrent resumptions reload after CAS conflict; they must not continue from stale state. Approval is required before remote mutation; resumption does not bypass it.

Forbidden states: pending rotation without a baseline, changed operation input during resume, uncertain task with a fresh token, completed seed before successful restore, secret password in argv/output, deleted active checkpoint, success before all requested checks pass.
