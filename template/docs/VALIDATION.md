# Validation and evidence limits

`backstage-aws check` is offline and checks formatting, initialization, validation, and tests for the three Terraform stacks. The application workflow installs dependencies immutably, typechecks, runs tests, and builds the backend. These checks do not establish that AWS resources can be created or reached.

The preview target is a reachable URL within five minutes of a pull-request push with a warm image cache. No timing has been measured; the budget is unverified. Each deployment reports per-step elapsed milliseconds and adds a timeline to `GITHUB_STEP_SUMMARY` when available.

No workflow, command, Terraform apply, or application deployment in this design has been run against a real AWS account. IAM policy evaluation, GitHub OIDC trust, database tasks and TLS, ECS startup/readiness, DNS/certificates, actual guest and OAuth sign-in, password rotation, and teardown remain unverified against live services. Do not describe them as tested.
