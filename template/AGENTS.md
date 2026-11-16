# Agent guide

- Use `backstage-aws`; [operator docs](https://github.com/lprior-repo/backstage-aws/tree/main/docs).
- Load trusted `environments/*.env` in shell/CI. CLI reads flags/env; no prompts.
- Normal commands write one JSON document to stdout; `--help` and `--version` print plain text. Progress goes to stderr; stop on nonzero exit or `ok:false`.
- Exits: 0 success, 2 invalid, 3 blocked or approval required, 4 tool/API failure, 5 remote work continues after timeout.
- Remote mutations require `--approve`; preserve state/KMS keys and active locks.
- Short-lived AWS credentials; GitHub OIDC. No GitHub deployment Environments.
- Separate preview/production accounts; namespaces share the database role. Sanitize seed data.
- Terraform 1.17.2; Docker Buildx or Podman (`--container-engine`) for images; matching attested CLI release in CI.
- `setup` shows the next prerequisite; `up --approve` runs the first install. Use `check --app` for app checks; `env up` deploys and `env down` removes a namespace.
- `db dump` exports a namespace; `seed` restores only the template. Password rotation restarts every environment's service.
- Live AWS, recovery, sign-in and five-minute startup remain unverified.

