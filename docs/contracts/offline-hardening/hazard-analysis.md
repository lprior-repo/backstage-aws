# Hazard analysis

| Hazard | Required guard | Remaining boundary | Current evidence |
|---|---|---|---|
| Versioned lock release resurrects earlier phase | CAS `Completed` record; never delete latest version | Real S3 versioning/conditional request behavior | `db/operation.rs`, `db/seed.rs`, `ops/rotation/state.rs` |
| Concurrent stale writer regresses phase | Validated adjacent transition + current ETag; reload conflict | Concurrent external effects must also be idempotent | `db/operation.rs`, `ops/rotate.rs`, `ops/rotation/state.rs` |
| Lost task response duplicates restore/role change | Persist/reuse attempt before launch | ECS token retention window and live API semantics | `db/operation.rs`, `db.rs`, `db/seed.rs`, `ops/rotate.rs` |
| Failed task can never retry | New attempt only on explicit rerun after observed terminal failure | Do not infer failure from timeout | `db/operation.rs`, `db.rs` |
| Replacement secret changes midway | Operation-pinned VersionId and required baseline | Secrets Manager stage APIs | `ops/rotate.rs`, `ops/rotation/state.rs` |
| Restore leaves stale schemas or destroys live seed | Empty stage + transactional restore + guarded cutover | PostgreSQL catalog witnesses and connection termination | `db/seed.rs`, `template/infra/base/scripts/restore-seed.sh` |
| Malformed checkpoint enters core | Schema/UUID/version/key validation at one boundary | Hostile persisted JSON size and parser limits | `db/operation.rs`, `db/seed.rs`, `ops/rotation/state.rs` |
| Child blocks on stdin or inherited stdout | Whole-operation deadline, bounded reads, owned cleanup | Unix process-group descendant cleanup only; non-Unix cleanup targets the direct child | `process.rs::execute`, `process.rs::spawn_owned` |
| Partial dump looks complete | Private staging, sync, atomic no-clobber publication | Filesystem atomicity/durability guarantees | `ops/dump.rs::publish_body` |
| Interleaved Terraform changes another namespace | Per-operation private stack/state working directories | Backend/state IAM remains live boundary | `terraform/stacks.rs` |
| Archive traversal/resource exhaustion | Strict regular root member, limits, private extraction | Executing trusted binary is not sandboxing | `release_check.rs` |
| Offline check is mistaken for production evidence | Explicit check scope and reported evidence boundary | AWS/recovery/sign-in/SLO acceptance remains unverified | `check.rs`, `local_check.rs`, `traceability-matrix.jsonl` |

Operation upload losers may leave unique orphan seed objects; this does not permit overwriting an Active operation's object. No cleanup may guess that an active object is unused. (See `db/seed.rs::upload_operation`.) No benchmark/performance improvement is claimed from library reuse. Full scope of accepted clauses is in `contract.md`; this analysis is not verifier coverage.
