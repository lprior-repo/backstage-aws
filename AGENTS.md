# Agent guide

- Operate through `backstage-aws`; [commands/settings](docs/README.md). Flags/env only; no prompts.
- Shell/CI loads trusted `environments/*.env`; secrets never belong there.
- One JSON stdout; progress/child logs stderr. Stop on nonzero exit or `ok:false`.
- Exits: 0 success, 2 invalid, 3 blocked, 4 external failure, 5 remote timeout.
- Remote mutations require `--approve`. Preserve state/KMS keys; never clear active locks.
- Short-lived AWS credentials; GitHub OIDC in CI. No GitHub deployment Environments.
- Separate preview/production accounts; namespaces share a database role. Sanitize seed data.
- Terraform 1.17.2; Docker Buildx or Podman (`--container-engine`) for image builds. CI requires the matching attested CLI release.
- `check --app` validates Terraform and app; `env up/down` handles deployment lifecycle.
- `db dump` exports a namespace; `seed` restores only the template. Password rotation affects all services.
- Offline checks are not live AWS, recovery, sign-in, or five-minute startup evidence.

