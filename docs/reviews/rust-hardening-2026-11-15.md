# Rust hardening review — 2026-11-15

## Status as of 2026-11-15

| Area | Disposition |
|---|---|
| BH01–BH11, BH13–BH21, CF01 | Current source has the repairs described below; offline evidence is recorded, not independently rerun here. |
| BH12 | Open: Unix process-group descendant cleanup is implemented; non-Unix cleanup currently targets only the direct child. |
| TE01, TE03, TE05–TE08, TE10–TE13, TE15–TE16 | Consumer-facing regression evidence exists in source/tests or the logged runtime rehearsal. |
| TE02, TE04, TE09, TE14, TE17 | Open test gaps: rotation service batching, state-delete failure, comprehensive no-approval side effects, direct environment parsing, and mixed-container task failure. |
| Live boundaries | IAM/KMS, ECS/Fargate/RDS, GitHub OAuth, DNS/TLS, recovery, startup SLO and cross-account restore remain unverified. |

No independent patch-review approval, formal proof, coverage percentage, or performance improvement is claimed. Offline mocks do not establish live AWS behavior.

## Scope and provenance

Baseline: `3b04b77`. Each independent reviewer reported an empty scoped diff at review start. Findings below therefore describe baseline defects, not patch-introduced defects. Reviewers performed source review and LSP navigation only, deliberately skipping builds/tests. Their independent roles covered core/onboarding, lifecycle/process/Terraform/publication, database/secrets, and test effectiveness. Parent integration is GPT-6.1 Sol.

## Correctness finding dispositions

Paths use `crates/backstage_aws/src/` unless prefixed `template/`.

| ID | Severity | Finding | Status | Current evidence |
|---|---|---|---|---|
| BH01 | P1 | Genuine Terraform version JSON rejected because `terraform_outdated` is missing | Fixed | `onboarding/probe/local.rs::parse_version`; `terraform_official_version_json_accepts_the_outdated_field` |
| BH02 | P2 | Permanently pending probe defeats poll-count bounds | Fixed | `ctx.rs::poll`; `deadline_times_out_a_probe_that_never_completes` |
| BH03 | P2 | Resume argv loses leading-hyphen option values | Fixed | `onboarding/model/actions.rs::append_pair`; `resume_arguments_roundtrip_through_the_real_cli_with_leading_hyphen_values` |
| BH04 | P2 | Identity Store validation rejects supported UUID IDs and accepts truncated prefixes | Fixed | `onboarding/probe/account.rs::{valid_identity_store_id,valid_instance_arn}`; `identity_store_ids_accept_supported_shapes_and_reject_prefixes`, `identity_center_instance_arns_require_a_complete_identifier` |
| BH05 | P1 | Buildx padded Driver output rejects a valid container builder | Fixed | `ops/build.rs::ensure_builder`; `accepts_the_documented_padded_buildx_driver_field` |
| BH06 | P1 | Inherited Git push URLs can publish to an unintended repository | Fixed | `github.rs::push_main`; `publish_push_targets_the_approved_url_not_origin_pushurl` |
| BH07 | P1 | Substring state-ownership checks can force-copy foreign-account bootstrap state | Fixed | `terraform/stacks.rs::{held_locally,belongs_to}`; `foreign_account_is_rejected_even_when_its_bucket_name_contains_this_account` |
| BH08 | P1 | Namespace operations share mutable Terraform backend/variables/plan working files | Fixed | `terraform/stacks.rs::{Workspace,workspace}`; `concurrent_namespaces_apply_their_own_backend_and_variable_plan` |
| BH09 | P2 | Relative Terraform executable paths change meaning after chdir | Fixed | `settings.rs::Settings::terraform_path`; `explicit_relative_terraform_path_executes_from_stack_working_directory` |
| BH10 | P2 | Actions environment incorrectly implies bot ownership for PAT-authored comments | Fixed | `github.rs::comment_identity`; `actions_pat_uses_the_authenticated_user_instead_of_the_actions_bot` |
| BH11 | P2 | Subprocess stdin delivery happens outside its deadline | Fixed | `process.rs::{execute,collect_io,read_stdout}`; `blocked_stdin_timeout_terminates_direct_child` |
| BH12 | P2 | Timeout/cancellation does not own descendants on every target | Open; Unix mitigation only | `process.rs::{spawn_owned,stop_child}`; Unix descendant tests `timeout_terminates_live_leader_and_descendant`, `caller_cancellation_kills_the_entire_process_group`. Other targets kill only the direct child. |
| BH13 | P1 | Concurrent rotations and mutable AWSPENDING can diverge database/current credentials | Fixed | `ops/rotate.rs::{reserve_role_change,stage_new_password,promote_version,restart_services}`, `ops/rotation/state.rs::validate_attempt_transition`; three rotation entrypoint regressions below |
| BH14 | P2 | Expanded psql arguments expose the application password | Fixed | `template/infra/base/scripts/rotate-db.sh` feeds stdin to `psql \password`; actual SQL rotation/error rehearsal in `runtime/check-v8.json` |
| BH15 | P2 | Archive-listed cleanup leaves obsolete seed schemas/data | Fixed | `template/infra/base/scripts/restore-seed.sh` guarded stage/cutover and old `seed_previous_*` cleanup; actual restore rehearsal in `runtime/check-v8.json` |
| BH16 | P2 | Shared seed upload key can restore another invocation's bytes | Fixed | `db/seed.rs::{upload_operation,seed_overrides}`; operation identity pins the upload object and restore input |
| BH17 | P2 | Same-second namespace dumps collide | Fixed | `ops/dump.rs::key_for_operation`; `operation_keys_differ_for_same_namespace_and_fixed_timestamp` |
| BH18 | P2 | Failed/interrupted downloads publish empty or partial final files | Fixed | `ops/dump.rs::{publish_body,download}`; stream-error, cancellation, private-file, and no-clobber regressions |
| BH19 | P2 | Download/storage failures are mislabeled `INVALID_INPUT` | Fixed | `ops/dump.rs::download`; `get_object_failure_leaves_destination_absent`, `a_fallible_sdk_byte_stream_leaves_no_download_file` |
| BH20 | Resource | Captured process output and remote state ingestion are unbounded | Fixed | `process.rs::read_stdout`, `terraform/stacks.rs::held_locally`/`MAX_STATE_BYTES`, `db/operation.rs::read_checkpoint_body` |
| BH21 | Harness | GitHub mock accept/read/join paths can hang indefinitely | Fixed | `github/tests.rs::{missing_expected_request_fails_after_a_bounded_accept_deadline,refuses_to_create_after_bounded_full_pagination}` |
| CF01 | Unranked | Deleting the latest version of a rotation lock can resurrect an older phase | Fixed | `ops/rotate.rs::put_rotation_operation` writes with ETag CAS; `ops/rotation/state.rs::RotationState::complete` persists `Completed`; `rotation_pins_versions_and_checkpoints_before_effects_then_promotes_restarts_and_completes` |

Rotation operation identity is separate from task-attempt identity. `ops/rotate.rs::reserve_role_change` and `ops/rotation/state.rs::validate_attempt_transition` encode reuse after uncertainty and a new token only after observed terminal failure followed by explicit rerun. See [the Rust contract](../contracts/offline-hardening/contract.md).

## Independent test-effectiveness findings

Required consumer-visible oracles, not source-text or wiring assertions:

1. Exact dump DB_NAME/BACKUP_KEY values, downloaded key/bytes, permissions and non-overwrite.
2. Full rotation entrypoint: pinned version, effect ordering, failure/recovery, service restart batching.
3. Actual rollout/health request consumption, pending-to-ready, rollback and unhealthy failures.
4. Database-drop failure preserves state; state-deletion errors propagate.
5. Complete scan SDK boundary, freshness edges and rejection before Terraform apply.
6. Actual secret storage ARN/document/version and sanitized failures.
7. Unsafe account evidence blocks public setup; later probes do not run.
8. Public DNS delegation rejection and malformed/failed resolver distinctions.
9. Every remote mutation includes dump approval and observes no forbidden mutation/output creation.
10. Full Docker/ECR workflow, stdin-only password, push, builder validation and failure short-circuit.
11. Real Terraform output contract includes the dump task definition.
12. Process termination, blocked stdin, cancellation and descendant-held stdout.
13. Bounded GitHub server failure when expected requests are absent/incomplete.
14. Isolated environment parsing and summary output; no concurrent ambient environment mutation.
15. Complete Terraform command trace and public application/runtime check dispatch.
16. Production GitHub-auth default and exact diagnostic/error sentinels.
17. Mixed-container ECS task failure: a nonzero container fails the attempt even when another exits successfully.

The IDs below map to the numbered oracles above in order.

| ID | Status | Current evidence or remaining gap |
|---|---|---|
| TE01 | Fixed | `ops/dump.rs::dump_request_contains_exact_database_and_operation_key_values`, `an_existing_user_file_is_not_overwritten_and_downloads_are_private`, and dump stream-failure regressions |
| TE02 | Open | Rotation entrypoint coverage: `ops/rotation/behavior_tests.rs::{rotation_pins_versions_and_checkpoints_before_effects_then_promotes_restarts_and_completes,rotation_observed_failure_returns_without_retry_and_later_approval_retries_same_versions,rotation_resumes_an_uncertain_reserved_attempt_without_restaging_secret_or_changing_token}`. `ops/rotate.rs::restart_services` batching has no multi-batch test. |
| TE03 | Fixed | `ops/env.rs::{wait_rollout,rollout,wait_healthy,has_healthy_target}`; `ops/env/tests/rollout.rs` and `ops/env/tests.rs::{environment_status_is_not_ready_when_ecs_is_not_active_even_with_a_healthy_target,environment_status_is_not_ready_when_active_ecs_has_no_healthy_targets,environment_status_is_not_ready_for_an_unhealthy_target,environment_status_is_ready_only_when_ecs_and_the_target_are_ready}` |
| TE04 | Open | `ops/env/tests.rs::a_database_drop_failure_after_destroy_preserves_the_state_object` verifies failed DB drop preserves state. `ops/env.rs::env_down` propagates DeleteObject errors, but no state-deletion failure test was found. |
| TE05 | Fixed | `ops/scan.rs::{run,gate}` and scan SDK/severity/freshness regressions, including `scan_uses_the_requested_digest_and_maps_actual_severity_counts` |
| TE06 | Fixed | `ops/secrets.rs::{set_identity_stores_exact_arn_camel_case_credentials_and_version,set_identity_sanitizes_the_service_error}`; rotation secret version/storage assertions in `ops/rotation/behavior_tests.rs` |
| TE07 | Fixed | `onboarding/probe/tests.rs::account_and_identity_center_safety_evidence_blocks_unsafe_states` |
| TE08 | Fixed | `onboarding/probe/tests.rs::dns_delegation_distinguishes_required_actions_from_external_failures`; `onboarding/probe/dns.rs::{zone_data,public_zone,nameservers}` |
| TE09 | Open | `ctx.rs::require_approval` guards the remote mutation entrypoints; `terraform/tests.rs::nothing_runs_without_approval` verifies Terraform has no command or file side effect. No corresponding full no-side-effect oracle covers every mutator. |
| TE10 | Fixed | `ops/build.rs::{build_with_docker,ensure_builder,login}`; `build_pushes_the_requested_tag_and_looks_up_that_exact_ecr_image`, `docker_build_failure_prevents_the_ecr_digest_lookup`, and builder validation regressions |
| TE11 | Fixed | `terraform/tests/contract.rs::the_stacks_declare_every_output_read_from_them` asserts the dump task-definition output |
| TE12 | Fixed | `process.rs::{execute,stop_child}`; blocked-stdin, timeout, cancellation, and descendant-held-stdout tests in `process.rs::tests` |
| TE13 | Fixed | `github/tests.rs::{missing_expected_request_fails_after_a_bounded_accept_deadline,refuses_to_create_after_bounded_full_pagination}` |
| TE14 | Open | `settings/derived.rs::the_settings_file_roundtrips_literal_values_without_pinning_a_namespace` checks summary output under an isolated environment. CLI inputs use `#[arg(env = ...)]`; no direct environment-input parsing test was found. |
| TE15 | Fixed | `cli.rs::check_request`, `lib.rs::run`, `check.rs::check`; `terraform/tests.rs::apply_runs_exactly_the_plan_it_wrote_against_the_namespace_state`; local runtime evidence `runtime/check-v8.json` and `runtime/check-v8.stderr` |
| TE16 | Fixed | `settings/derived.rs::production_without_explicit_auth_defaults_to_github`, `onboarding/probe/tests.rs::error_codes_are_extracted_without_stderr_payload`, and `ops/secrets.rs::set_identity_sanitizes_the_service_error` |
| TE17 | Open | `db.rs::task_succeeded` requires a nonempty container list and every exit code zero; no mixed-container failure regression was found. |


The original broader mutation-survival statements were source-based inference, not executed mutation evidence.

## Baseline file coverage

Fully reviewed core files: settings.rs; settings/{derived,validation,tests}.rs; error.rs; ctx.rs; cli.rs; cli/tests.rs; onboarding.rs; onboarding/{model,probe}.rs; onboarding/model/{actions,tests}.rs; onboarding/probe/{account,aws,command,dns,local,parse,tests}.rs.

Fully reviewed lifecycle files: process.rs; terraform.rs; terraform/{stacks,tests}.rs; terraform/tests/contract.rs; github.rs; github/tests.rs; ops/{build,env,scan,up,doctor,publish,mod}.rs; ops/env/tests.rs; ops/env/tests/rollout.rs.

Fully reviewed database files: db.rs; ops/{dump,rotate,secrets}.rs. Independent suite review covered tests/{cli,setup}.rs and all baseline inline/dedicated suites above. Parent covered main.rs, lib.rs, check.rs and template.rs. Coverage is source inspection, not execution coverage. Newly added modules/regressions require integration reconciliation; the baseline inventory does not imply they were independently approved.

Consumer context included generated base/env Terraform, database/backup definitions, app production configuration, workflows, command/error/settings references, and operational recovery/secrets guides.

## Evidence and acceptance boundaries

Baseline evidence already executed: strict production Clippy, dependency audit and unused-dependency checks; workflow actionlint; genuine Terraform 1.17.2 version JSON observation. Logs: `.cache/rust-audit-20261115/`. These pre-repair checks are not post-change verification.

Red Queen generations 2–4 completed after integration: process/hostile-release regressions, seed/HCL regressions, then replay of all 21 hostile CLI cases. All selected checks passed; the blackboard records four consecutive zero-survivor generations including the earlier baseline generation and nine permanent checks. Its workflow verdict remains UNKNOWN, not an independent approval. Actual focused mutation execution caught all six `validate_digest` mutants.

All nine contract artifacts are nonempty; thirteen unique clauses, proof seeds and requirements have exact joins. Proof seeds remain unexecuted planning inputs, not proofs. No live AWS IAM/KMS authorization, Fargate/RDS acceptance, GitHub OAuth sign-in, disaster recovery, five-minute startup SLO or cross-account restore has been established. Offline mocks do not discharge those boundaries.

Strict skill concerns such as function length, primitive domain values, synchronous filesystem work in async orchestration and mutable accumulation are recorded separately from concrete defects; they do not by themselves establish a panic, security exploit or failed deployment. No benchmarks were found or run.

## Executed evidence

Executor: parent GPT-6.1 Sol in the active local workspace. Raw logs are local, ignored artifacts under `.cache/rust-audit-20261115/`; they are not committed or portable assurance attachments.

| Command / scenario | Observed result | Raw artifact |
|---|---|---|
| `cargo test --locked --workspace --all-targets --all-features` | Exit 0; 164 library + 4 CLI + 1 core CLI + 12 release + 6 setup tests; no failures or ignored tests | `integrated-tests-current.log` |
| Strict production Clippy command below | Exit 0; no warnings | `strict-clippy-current.log` |
| `cargo test --locked --lib process::tests` after final cleanup repair | Exit 0; 13 passed | `process-final.log` |
| Real CLI project initialization, then `check --runtime --postgres-bin .cache/postgresql16/install/bin` using Terraform 1.17.2 and Node 24.22.1 | Exit 0; `ok:true`, all 29 selected check labels returned | `runtime/init-v2.json`, `runtime/check-v8.json`, `runtime/check-v8.stderr` |
| Generated access/bootstrap/base/env `fmt`, `init`, `validate`, `test` | All passed; 18 Terraform tests | `runtime/check-v8.stderr` |
| Generated app immutable install, typecheck, tests, frontend/backend build | All passed | `runtime/check-v8.stderr` |
| Local PostgreSQL fixture rehearsal of canonical clone/dump/restore scripts, stale-schema cleanup and password/SQL handling; not disaster-recovery acceptance | Exercised against local test database only | `runtime/check-v8.json`, `runtime/check-v8.stderr` |
| Built Backstage readiness and catalog-authorization HTTP rehearsal; no sign-in path or production identity provider was verified | Passed local HTTP checks only | `runtime/check-v8.json`, `runtime/check-v8.stderr` |
| `cargo mutants --gitignore true --file crates/backstage_aws/src/ops/build.rs --re validate_digest --baseline skip --jobs 1 --timeout 60 --build-timeout 600 --output .cache/rust-audit-20261115/mutation -- --locked --lib` | Exit 0; 6 tested, 6 caught | `mutation-run.log`, `mutation/mutants.out/` |
| Red Queen post-integration generations | Zero survivors in selected checks, not universal correctness | `generation-2.jsonl`, `generation-3.jsonl`, `generation-4.jsonl`, `red-queen-state.json` |

Strict command:

```sh
cargo clippy --locked --workspace --lib --bins --all-features -- -D warnings -D unsafe_code -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic -D clippy::panic_in_result_fn -D clippy::todo -D clippy::unimplemented -D clippy::dbg_macro -D clippy::indexing_slicing -D clippy::string_slice -D clippy::get_unwrap -D clippy::arithmetic_side_effects -D clippy::as_conversions -D clippy::let_underscore_must_use -D clippy::await_holding_lock -D clippy::unused_async -D clippy::large_futures
```

The full Rust suite preceded the last runtime-harness corrections. Those corrections were compiled and exercised through the successful real CLI runtime scenario; the final process cleanup change additionally ran all thirteen process regressions. Earlier failed attempts remain represented by the runtime artifacts: quoted PostgreSQL listen-address arguments, missing local schema division, and incorrect permission request shape/types/resource reference were repaired, not bypassed.

## Requirement-to-evidence reconciliation

| Clauses | Production boundary | Evidence / limit |
|---|---|---|
| C01 | `cli.rs`, `check.rs`, settings | Semantic project/release and none/build/runtime requests; approval and real CLI regressions |
| C02–C04 | `db/operation.rs`, `db/seed.rs`, `ops/rotation/` | Validated identities, adjacent transitions, ETag CAS, durable completion and full seed/rotation entrypoint regressions; S3/ECS live behavior unverified |
| C05 | Rotation adapter and canonical database scripts | Pinned-version SDK tests and actual non-superuser local SQL rotation/failure checks; private Fargate sidecar delivery remains a live boundary |
| C06 | Seed adapter and `template/infra/base/scripts/restore-seed.sh` | Local PostgreSQL fixture rehearsed staging/cutover and failed-restore preservation; not disaster-recovery acceptance or live RDS evidence |
| C07 | `process.rs`, `process-wrap` | Thirteen executed process lifecycle regressions; Unix process-session isolation prevents inherited terminal prompts, while non-Unix cleanup targets only the direct child |
| C08 | `terraform/stacks.rs` | Ownership rejection and concurrent private-workspace regressions |
| C09 | `ops/dump.rs` | Actual filesystem/stream failure, cancellation, permissions and no-clobber regressions |
| C10 | `release_check.rs`, installer boundary | Twelve native release/archive cases; no sandbox or provenance claim from native execution |
| C11 | `check.rs`, `local_check.rs` | Successful generated-project build and actual PostgreSQL/Backstage rehearsal |
| C12 | `error.rs`, CLI/output | Stable semantic exit/JSON tests and 21 hostile input cases; offline/live distinction retained |
| C13 | `terraform/generate.rs`, template renderer | `hcl-rs` literal evaluation regression, upstream lossless HCL blueprint parsing and all four real generated stacks |

## Active-context Truth Serum decision

### Execution evidence

The command results above were observed by the parent, not accepted from worker summaries. The final runtime result was `{"command":"check","ok":true}` with all 29 check labels; the complete JSON is in `runtime/check-v8.json`. There is no formal-verifier execution or coverage measurement to package.

### End-user review

The local check builds the generated app and exercises canonical database scripts, readiness and catalog authorization; it does not verify live recovery, any sign-in flow, or production GitHub authentication. Child logs remain on stderr and stdout remains one JSON result.

### Skeptical QA review

Baseline source inventory and independent findings are retained above. Parent reconciliation covers the new operation/attempt types, seed CAS bridge, rotation transitions, semantic check dispatch and HCL generator. Tests cover consumer-visible effects rather than source strings. OS process cleanup is necessarily best effort if the kernel refuses a signal; unexpected failures are reported without panicking, and a failed diagnostic write cannot be propagated from Drop. Third-party Yarn peer/optional-binding and local-only Backstage warnings are not claimed absent.

### Mandated acceptance boundaries

Offline acceptance is complete for the executed scope. Live IAM/KMS, Fargate/RDS, GitHub OAuth, DNS/TLS, disaster recovery and startup SLO acceptance remain **UNVERIFIED**, not waived or silently counted as passed. No commit, push, deployment, paid service or formal proof was performed for this integration.
