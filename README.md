# backstage-aws

Backstage on AWS Fargate. The Rust CLI provisions shared AWS resources; GitHub Actions deploys an isolated namespace for each same-repository pull request.

**[Quickstart](docs/quickstart.md)** · [Architecture](docs/concepts.md) · [Documentation](docs/README.md)

| Included | Default |
|---|---|
| PR previews | `pr-N` hostname, cloned seed database, readiness check and URL comment; fork PRs do not deploy |
| AWS authentication | GitHub Actions OIDC; no GitHub deployment Environments |
| Compute | One 1-vCPU/2-GB Fargate task per namespace; Spot by default except production |
| Shared infrastructure | ALB (HTTPS with a domain, HTTP-only without), ECR, private PostgreSQL 16, Secrets Manager and VPC |
| Cost controls | No NAT gateways, `db.t4g.micro`, 20-GB gp3; the $100 budget threshold needs a configured email and is an alert, not a cap |
| Recovery paths | Seven-day RDS automated backups/PITR; versioned, KMS-encrypted Terraform state and seed/dump objects |
| Configuration | Commit non-secret settings in `environments/*.env`; each command writes one JSON document (except plain-text `--help` and `--version`) |

## Install

```sh
git clone https://github.com/lprior-repo/backstage-aws.git
cd backstage-aws
cargo install --locked --path crates/backstage_aws
```

Follow the [quickstart](docs/quickstart.md) in a dedicated preview account. After creating a project and loading its settings, the read-only `backstage-aws setup --profile PROFILE` guides account prerequisites one step at a time. It does not create infrastructure.

The first `up --approve` builds and pushes the initial image locally, so Docker Engine with Buildx, or Podman (`--container-engine podman`), is required. Later pull-request workflows build images in GitHub Actions. Those workflows require the matching published CLI release and verify its attestation.

## Local app work

See the [local development guide](docs/guides/local-development.md), [plugin guide](docs/guides/plugins.md), and [CLI reference](docs/reference/cli.md).

## Project status

Local evidence dated 2026-11-15: 187 Rust tests, strict production Clippy, generated Terraform validation and 18 Terraform tests, app build/typecheck/tests, and a PostgreSQL 16/Backstage runtime rehearsal. See [evidence and boundaries](docs/reviews/rust-hardening-2026-11-15.md).

**Not live-verified:** AWS/OIDC deployment, generated GitHub workflows, RDS operations, DNS/TLS, GitHub sign-in, disaster recovery, or the under-five-minute preview target. Local PostgreSQL recovery checks are not RDS recovery evidence.

Use separate preview and production accounts. Preview namespaces share one database role and are not tenant isolation. Shared resources remain billable with no previews. [Costs and defaults](docs/prerequisites.md#costs) · [Backup and restore](docs/guides/backup-and-restore.md).
