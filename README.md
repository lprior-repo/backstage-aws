# backstage-aws

Backstage on AWS Fargate. One Rust CLI; one running environment per pull request.

**[Quickstart](docs/quickstart.md)** · [Architecture](docs/concepts.md) · [Documentation](docs/README.md)

| Included | Default |
|---|---|
| PR previews | `pr-N` hostname, seeded database, readiness check, URL comment, close-to-delete |
| AWS authentication | GitHub OIDC; no GitHub deployment Environments |
| Compute | 1-vCPU/2-GB Fargate; Spot previews, on-demand production |
| Shared infrastructure | ALB/HTTPS, ECR, private PostgreSQL 16, Secrets Manager |
| Cost controls | No NAT, `db.t4g.micro`, 20-GB gp3, $100 budget alert |
| Recovery | Seven-day RDS PITR, versioned encrypted state/dumps, `db dump` |
| Portability | Committed non-secret settings; one JSON result per command |

## Install

```sh
git clone https://github.com/lprior-repo/backstage-aws.git
cd backstage-aws
cargo install --locked --path crates/backstage_aws
```

Discover prerequisites: `backstage-aws setup`. Read-only; reports the next action and resume command.

Then [prepare a sandbox account and deploy your first PR](docs/quickstart.md). Images build in GitHub Actions; local Docker is optional. Generated workflows require the matching published, attested CLI release.

## Project status

Passed: Rust checks/tests, offline Terraform tests, workflow lint, generated-settings round-trip, repository build CI.

**Not live-verified:** AWS/OIDC, generated deployment workflows, database operations, DNS/TLS, sign-in, recovery, or the under-five-minute preview target.

Use separate preview/production accounts. Preview namespaces share a database role—not tenant isolation. [Costs](docs/prerequisites.md#costs) · [Backup and restore](docs/guides/backup-and-restore.md).

