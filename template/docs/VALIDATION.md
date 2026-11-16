# Validation and evidence limits

`backstage-aws check` needs no AWS credentials or account access; it checks formatting, initialization, validation, and tests for all three Terraform stacks. `Validate source` (`.github/workflows/validate.yml`) runs `backstage-aws check --runtime --postgres-bin /usr/lib/postgresql/16/bin`: it also installs app dependencies immutably, typechecks, tests, builds the backend, and rehearses PostgreSQL 16 backup/restore plus the app locally. These checks do not establish that AWS resources can be created or reached.

The preview target is a reachable URL within five minutes of a pull-request push with a warm image cache. No timing has been measured; the budget is unverified. Each deployment reports per-step elapsed milliseconds and adds a timeline to `GITHUB_STEP_SUMMARY` when available.

No end-to-end deployment to AWS has been performed or verified. IAM policy evaluation, GitHub OIDC trust, database tasks and TLS, ECS startup/readiness, DNS/certificates, actual guest and OAuth sign-in, password rotation, and teardown remain unverified against live services. Do not describe them as tested.
