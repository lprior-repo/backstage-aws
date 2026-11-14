# Backstage AWS generator

The Rust binary now uses `backstage-aws-full-automation.zip` as its project baseline. It embeds the application, operator automation, Terraform, GitHub workflows, tests, documentation, licenses and provenance. Generation works offline from a copied executable; it does not run Terraform, install dependencies, call AWS or publish anything.

## Generate

```sh
cargo build --locked --release
./target/release/backstage-aws generator.example.toml ../my-portal
```

Edit the example TOML with your real non-secret identifiers before deployment. The output directory must not exist; its parent must exist. Existing directories, files and symlinks are refused. A write failure can leave a newly created partial directory for inspection. Shell entry points receive executable permissions on Unix.

The output contains 144 files, including a ready-filled `deployment.json`. Its schema matches the archive's operator automation. `owner_github_id` is the immutable numeric GitHub ID, not a login. The example account, ID, domain, email addresses and networks are fixtures. Unknown fields and credential values are rejected; configuration errors do not echo input contents.

The config schema changed with this baseline: `owner_github_id`, `budget_email` and `alert_email` replace the earlier login-based setup. Image digests, identity-secret ARNs and Terraform state are provisioned and managed by the supplied automation; the earlier `image_digest`, `app_secret_arn`, `state_bucket` and `github_user` generator fields are no longer inputs. Use the updated example. Optional plugin-secret references can be configured in generated `deployment.json` using the archive's documented schema.

After generation, use Node 24.22.1 and Terraform 1.17.2:

```sh
cd ../my-portal
./platform configure
./scripts/check-automation.sh
```

`configure` writes local Terraform inputs and updates the catalog owner; it makes no AWS calls. Follow the generated README for actual deployment. Commands such as `up`, `release`, `rollback`, secret rotation and GitHub setup have real external effects and are not run by the generator or local test suite.

## Baseline and adaptations

The imported archive is preserved under `templates/full-automation/`. Its SHA-256 is `9c1a4e68da9190dc2fd2573245c230f1ce53f673244f7745598c6bd3e32dacdc`; all three Downloads copies were identical and the original source checksums passed before adaptation. Earlier repository work is preserved, including a source snapshot under `.evidence/`; the generator's active manifest now selects this archive-based template.

This baseline contains Backstage 1.56.5, PostgreSQL 16 on RDS, Fargate, JSONata rollout/maintenance state machines, default-deny permissions, image gates, secret/bootstrap recovery and operator automation. This changes the earlier Aurora hosting scaffold. The generator remains Rust; Backstage and the supplied operator automation run on Node. The optional supplied `portalctl` Rust helper keeps its separate Rust 1.100.0 toolchain.

Local testing exposed incomplete Terraform mock fixtures. The adapted security test supplies valid certificate ARNs and plan-time computed identifiers, with distinct task revisions, subnets and database/identity secrets. Its assertions and production policies/state-machine definitions remain intact. See `templates/full-automation/GENERATOR-PROVENANCE.md` for the exact adaptation scope.

The original `SOURCE-SHA256SUMS` is emitted as `provenance/archive-SOURCE-SHA256SUMS`, explicitly historical rather than a manifest for rendered files. The archive's preparation logs are also historical; they do not replace fresh test evidence.

## Verify

```sh
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --lib --bins --examples --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --release
```

Generator tests cover the complete output, deployment JSON, immutable owner binding, deterministic generation, preserved binary assets/expressions, executable scripts, copied-binary offline use, invalid inputs and existing-path preservation. CI also configures the generated project and runs its supplied automation checks and application build/tests.

Fresh local evidence is recorded in `.evidence/automation-adoption.md`. Original preparation limits remain limits until exercised: fake AWS tests and Terraform mocks do not establish actual AWS IAM, Step Functions execution, TLS/DNS, ECR/ECS operation or real GitHub authentication. Docker image execution and AWS deployment remain separate acceptance checks. No AWS resources or GitHub settings were changed during this adoption.
