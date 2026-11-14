# Agent guide

- Use `backstage-aws`; [operator docs](https://github.com/lprior-repo/backstage-aws/tree/main/docs).
- Load trusted `environments/*.env` in shell/CI. CLI reads flags/env; no prompts.
- One JSON stdout; progress stderr. Stop on nonzero exit or `ok:false`.
- Exits: 0 success, 2 invalid, 3 blocked, 4 external failure, 5 remote timeout.
- Remote mutations require `--approve`; preserve state/KMS keys and active locks.
- Short-lived AWS credentials; GitHub OIDC. No GitHub deployment Environments.
- Separate preview/production accounts; namespaces share the database role. Sanitize seed data.
- Terraform 1.17.2; Docker Buildx for images; matching attested CLI release in CI.
- `check --app` validates; `env up/down` deploys/removes. Removal drops the namespace database.
- `db dump` exports a namespace; `seed` restores only the template. Rotation affects every service.
- Live AWS, recovery, sign-in and five-minute startup remain unverified.

