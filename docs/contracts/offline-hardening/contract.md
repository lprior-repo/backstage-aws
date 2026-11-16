# Rust hardening contract

Status: accepted integration contract; current source and recorded offline evidence are reconciled, except for the platform limit in C07: descendant-group cleanup is Unix-only. No formal proof is claimed; live AWS behavior remains unverified.

| Clause | Requirement |
|---|---|
| C01 | Approved mutations only; validated IDs and parsed semantic check requests enter the core |
| C02 | One immutable operation/input identity survives all resumptions |
| C03 | Validated checkpoints progress through legal adjacent states using current ETag CAS; completion remains durable |
| C04 | Persisted ECS attempt identity survives uncertainty; fresh retry requires observed terminal failure and later explicit invocation |
| C05 | Rotation pins predecessor and replacement versions; plaintext never enters argv, persistent/host files, logs or JSON output; private task-scoped encrypted sidecar delivery is permitted |
| C06 | Seed restore uses empty staging and guarded cutover; pre-cutover failure preserves the original seed; ambiguous cutover states fail closed |
| C07 | On Unix, process ownership includes descendants; output is bounded, one deadline covers the operation, and cancellation cleans up the owned group. Other targets currently clean up only the direct child; whether descendant cleanup is required there remains a platform policy decision. |
| C08 | Terraform operations isolate mutable working state and reject foreign/malformed bootstrap ownership |
| C09 | Local dump publication is private, complete, no-clobber and cancellation-safe |
| C10 | Release archive validation checks structure, bounds, native execution and exact CLI protocol without claiming provenance/sandboxing |
| C11 | Runtime check exercises the built app and canonical database scripts; success means every requested check actually passed |
| C12 | JSON/error contracts remain stable and evidence explicitly distinguishes offline observations from live acceptance |
| C13 | Terraform output uses maintained HCL syntax trees; blueprint expressions/scripts retain their meaning, variables remain literal data, and Terraform still validates provider schemas |

## Reconciliation gate

Production types now carry the accepted states: semantic `CheckRequest`/`ApplicationCheck` replace boolean mode combinations, and persisted operation lifecycle uses validated identities with CAS transitions. Proof seeds are hints, not proof obligations, artifacts or passed checks. No data-model decision is open: approved reruns resume Active; only Completed can be replaced; terminal task failure requires a later explicit retry. C07's non-Unix descendant-cleanup requirement remains a platform policy decision; current code cleans only the direct child.

See domain-model.md, type-contracts.md, workflow-model.md, error-taxonomy.md, boundary-map.md, hazard-analysis.md and the two JSONL indexes.
