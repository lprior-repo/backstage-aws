# Preview environments

Each namespace has its own service, database, and Terraform stack. Namespaces share the preview account's VPC, ALB, RDS instance, ECR repository, seed database, and application database role; namespaces are not security boundaries. The default namespace for PR 42 is `pr-42`, unless `BACKSTAGE_AWS_NAMESPACE` overrides it.

Every successful `backstage-aws` command exits 0 and prints one JSON document. Exit 2 means invalid input, 3 blocked/precondition, 4 external or state failure, and 5 timeout; remote work may continue after exit 5.

## Set up and publish

1. For a fresh generated project, set the required `BACKSTAGE_AWS_*` project/account/DNS/repository settings and AWS credentials for the preview account. Review the generated files before running:

   ```sh
   GITHUB_TOKEN="$(gh auth token)" backstage-aws up --approve
   ```

   Expected: exit 0 and JSON `ok:true`, `command:"up"`. `result` contains `doctor`, `bootstrap`, `base`, `seed`, `environment`, and `publish`; bootstrap reports `state_migrated`, `settings_file`, and `deploy_role_arn`; `environment` reports `namespace`, `url`, `image_digest`, `timings`, and `comment_url`; `publish` reports `repository`, `created`, and `commit`. `up` applies the initial environment and then publishes the current checkout to `main`. The final publish requires a GitHub token with repository-admin access and may fail after AWS resources already exist.
2. If the project is already provisioned but has not been published, inspect the checkout, load its reviewed settings, and publish:

   ```sh
   git status --short
   set -a; . environments/preview.env; set +a
   GITHUB_TOKEN="$(gh auth token)" backstage-aws publish --approve
   ```

   Expected: `git status` exits 0; `publish` exits 0 and returns JSON `result.repository`, `result.created`, and `result.commit`. `publish` stages all checkout changes, commits if dirty, and pushes the checked-out `HEAD` to `main`; review all changes first. Do not run it after `up` has already published.

## Deploy and inspect a PR

3. The generated `preview.yml` deploys same-repository PRs on opened, synchronize, and reopened events. It checks out the PR merge revision, loads `environments/preview.env`, assumes `<name>-github-deploy` through GitHub OIDC, and runs `env up --approve`. On close, a `pull_request_target` job checks out only the base branch and runs `env down`; it does not execute PR code. Both jobs serialize by PR without cancelling active operations. Fork PRs are excluded; no GitHub Deployment Environment is used.
4. To list active namespaces and inspect PR 42:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws env list
   backstage-aws env status --pull-request 42
   ```

   Expected: `env list` exits 0 with JSON `result.namespaces`; `env status` exits 0 with `result.namespace`, `result.url`, `result.service_status`, `result.running_count`, `result.image_digest`, and `result.ready`. Readiness does not verify sign-in, DNS, or application behavior.
5. To deploy manually, use the same settings and PR selector:

   ```sh
   set -a; . environments/preview.env; set +a
   GITHUB_TOKEN="$(gh auth token)" backstage-aws env up --pull-request 42 --approve
   ```

   Expected: exit 0 and JSON `result.namespace`, `result.url`, `result.image_digest`, `result.timings`, and `result.comment_url`. With a usable GitHub token, the CLI creates or updates the PR comment; without one, deployment still returns its URL and `comment_url` is null.

## Back up and remove

6. Before teardown, export any data to retain to a new local file path and optionally restore it as the seed for future namespaces:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws db dump --pull-request 42 --file ./pr-42.dump --approve
   backstage-aws seed --file ./pr-42.dump --approve
   ```

   Expected: `db dump` exits 0 with JSON `result.bucket`, `result.key`, `result.database`, and `result.file`; `seed` exits 0 with `result.bucket`, `result.key`, `result.operation_id`, and `result.resumed`. The custom-format dump contains the selected namespace without owner or ACL entries. `seed` replaces the shared seed only; existing namespaces are unchanged.
7. To remove PR 42 manually, load preview settings and run:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws env down --pull-request 42 --approve
   ```

   Expected: exit 0 and JSON `result.namespace`, `result.destroyed`, and `result.comment_url`. This deletes the namespace database. It does not remove shared VPC/RDS/ECR resources; S3 state versions and shared backups are not a namespace restore mechanism. See [Tear down](teardown.md) and [Back up and restore](backup-and-restore.md).

## Cost and network defaults

8. The generated base uses zero NAT gateways by default; Fargate tasks receive public subnet addresses, but inbound application traffic is restricted to the ALB and the database has no internet route. Preview uses one vCPU/two GiB task per namespace, `db.t4g.micro` with 20 GiB gp3 shared RDS, and Fargate Spot by default except for `production`. A `$100` budget is an alert, not a spend cap. No cost or startup time is measured.
