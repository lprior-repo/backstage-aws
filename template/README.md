# Backstage on AWS

This project contains the Backstage app, Terraform, and thin GitHub Actions workflows. Operate it with `backstage-aws`; GitHub OIDC supplies short-lived AWS credentials. There are **no GitHub deployment Environments**.

Full walkthrough and operator guides: [backstage-aws documentation](https://github.com/lprior-repo/backstage-aws/tree/main/docs).

## First three steps

1. **Configure a new shell.** Use short-lived credentials for the preview account, then load the committed, non-secret settings:

   ```sh
   export AWS_PROFILE=backstage-preview  # replace with your short-lived preview profile name
   set -a
   . environments/preview.env
   set +a
   ```

   Set `GITHUB_TOKEN` or `GH_TOKEN` for GitHub API access, and configure Git's credential helper to push to the repository. The token must be able to administer the configured repository or create it. Use `production.env` only with credentials for the separate production account. The CLI reads flags and environment variables, not these files directly; source only trusted files.

2. **Check prerequisites before provisioning.** Install Terraform 1.17.2, Docker with Buildx or Podman, Git, and AWS CLI v2, then run:

   ```sh
   backstage-aws setup
   backstage-aws check
   backstage-aws doctor
   ```

   `setup` is read-only: follow its `next` action and rerun it until prerequisites are ready. `check` validates Terraform without AWS credentials; `doctor` checks local tools, AWS identity, and GitHub access. Do not continue to `up` until all three succeed.

3. **Run the first install:**

   ```sh
   backstage-aws up --approve
   ```

   To use a custom PostgreSQL dump or production GitHub OAuth credentials, provide `--seed-file /secure/path/seed.dump` and/or `--identity-file /secure/path/oauth.json` as needed. Without `--seed-file`, new environments clone the empty seed. `up` provisions shared resources, deploys the initial environment, and publishes the project; it builds the first image locally and commits non-ignored project files to `main`. Keep dumps and credentials outside this project.

The CI workflows download and verify the matching attested CLI release; that release must exist before workflows can run. Open a same-repository PR to build/deploy it. The CLI creates `pr-N`, waits for ECS rollout and a healthy target, and comments the URL. Closing the PR runs cleanup. A cold image build may exceed the unmeasured five-minute target.

## Operate

```sh
backstage-aws env list
backstage-aws env status --pull-request 38
backstage-aws db dump --pull-request 38 --file /secure/path/pr-38.dump --approve
backstage-aws env down --pull-request 38 --approve
```

`env down` destroys the namespace database. Shared infrastructure remains billable. [Recovery](docs/RECOVERY.md) · [Security](docs/SECURITY.md) · [Automation](docs/AUTOMATION.md) · [Secrets](docs/SECRETS.md).

Defaults favor cost: no NAT, `db.t4g.micro`/20-GB gp3, 1-vCPU/2-GB tasks, Spot except production, no Container Insights. Namespaces share a database role: they are not tenant security boundaries.

## Automation contract

Normal commands write one JSON document to stdout; `--help` and `--version` print plain text. NDJSON progress and child logs go to stderr. Remote mutations need `--approve`. Exit codes: 0 success, 2 invalid input, 3 blocked or approval required, 4 tool/API failure, 5 remote work continues after timeout. `--help` lists all commands and settings.

Only AWS-independent checks have been verified. AWS apply, IAM/OIDC, DNS/TLS, database operations, sign-in, recovery, generated deployment workflows, and startup timing require a live sandbox acceptance run.
