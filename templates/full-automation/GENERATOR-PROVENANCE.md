# Template provenance

Starting archive: `backstage-aws-full-automation.zip`, SHA-256 `9c1a4e68da9190dc2fd2573245c230f1ce53f673244f7745598c6bd3e32dacdc`. The numbered Downloads copies were byte-identical. All original archive entries passed their supplied source checksums before adaptation.

The Rust generator embeds the full automation project and emits deployment.json from validated TOML inputs. It fills the portal title and immutable numeric GitHub owner annotation, and marks shell entry points executable. Generation makes no network or cloud calls.

The supplied security test's ACM mocks now use valid synthetic certificate ARNs. Computed resource identifiers are supplied during planning so that workflow and IAM assertions can evaluate. Portal, bootstrap and rotation task revisions, application subnets and identity/database secrets have distinct fixtures. No IAM policy or state-machine definition is overridden; security assertions remain unchanged. The original preparation environment had not run these mock plans successfully.

`provenance/archive-SOURCE-SHA256SUMS` records the original archive's files before these adaptations. It is historical provenance, not a checksum manifest for generated or modified files. Original preparation logs and validation documents are also historical claims; consult the current local test report for fresh results.

This baseline uses PostgreSQL 16 on RDS, JSONata rollout and maintenance state machines, and the supplied Node operator automation. The Rust executable generates this project; Backstage and the operator automation retain their supplied Node runtime. AWS deployment and security acceptance require live evidence.
