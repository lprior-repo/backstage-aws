# Preview environments

| Shared per preview account | Per PR |
| --- | --- |
| VPC, ALB, RDS, ECR, seed DB | Service, database, Terraform stack |

The namespace is `pr-<number>` unless `BACKSTAGE_AWS_NAMESPACE` overrides it. A new namespace clones `seed` once; redeploys preserve its database.

## GitHub Actions lifecycle

`preview.yml` deploys opened, updated and reopened same-repository PRs. It checks out the PR **merge revision**, loads `environments/preview.env`, assumes the deploy role with GitHub OIDC and runs `env up --approve`. The CLI builds/pushes the image, clones the DB if absent, applies Terraform, waits for ECS/ALB health and updates the PR comment. Forks are excluded.

On close, a trusted `pull_request_target` job checks out the base branch and runs `env down`; it never executes PR code. Up/down share a per-PR concurrency group and do not cancel active applies.

## Inspect or operate

```sh
set -a; . environments/preview.env; set +a
backstage-aws env list
backstage-aws env status --pull-request 42
GITHUB_TOKEN="$(gh auth token)" backstage-aws env up --pull-request 42 --approve
```

`--pull-request 42` selects `pr-42`; with `GITHUB_TOKEN`, the CLI creates/updates its GitHub comment. Without a token, deploy still returns the URL in JSON. `status` reports URL, image and readiness—not sign-in or application behavior. Settings are committed non-secret configuration; credentials are separate.


## Defaults

| Setting | Default | Trade-off |
| --- | --- | --- |
| NAT gateways | 0 | Tasks use public subnet addresses; security groups admit inbound app traffic only from ALB. DB has no internet route. |
| RDS | `db.t4g.micro`, 20 GiB gp3 | Shared capacity and limited connections |
| ECS task | 1 vCPU, 2 GiB | One task per namespace |
| Fargate Spot | On except environment `production` | Reclaimable, no capacity guarantee |
| Monthly budget alert | $100 | Alert, not a cap |

Private task egress: `BACKSTAGE_AWS_FCK_NAT=true` for one shared instance, or `BACKSTAGE_AWS_NAT_GATEWAYS=1/2` for managed NAT; rerun `base`. Do not combine them. fck-nat is single-instance/single-AZ, not HA; operators own replacement and AMI patch updates. Shared idle resources incur charges; no cost or timing is measured.

Each namespace has its own database, but namespaces share RDS and the application role; they are not security boundaries. `seed --file` affects future namespaces only. Secrets come from Secrets Manager; ingress CIDRs control ALB access. Fork PRs receive no deploy role.

See [Seed database](seed-database.md), [Secrets](secrets.md), [Back up and restore](backup-and-restore.md), [Tear down](teardown.md), and [Other CI](other-ci.md).
