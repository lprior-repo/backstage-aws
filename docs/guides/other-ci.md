# Other CI systems

Generated workflows support GitHub Actions. A different CI provider needs its own trusted-branch rules, short-lived AWS federation, and one-writer-per-namespace serialization. Do not run deploy jobs on untrusted fork code.

Every successful `backstage-aws` command exits 0 and prints one JSON document. Exit 2 means invalid input, 3 blocked/precondition, 4 external or state failure, and 5 timeout; remote work may continue after exit 5.

## Set up the runner

1. Create provider-specific OIDC/workload-identity trust and a narrowly scoped deploy role for that CI provider. The generated GitHub role is `<name>-github-deploy`, trusts `token.actions.githubusercontent.com`, allows any ref in the configured repository for preview, and restricts production to `refs/heads/main`. Do not reuse its GitHub trust for another provider. The deploy policy does not grant secret-value reads, and generated workflows do not use GitHub Deployment Environments.
2. Install the same CLI release version and Terraform 1.17.2 used by the project. Verify the CLI release archive with GitHub attestations before installing it; use the release version and repository embedded in the generated `setup-backstage-aws` action:

   ```sh
   CLI_VERSION='VERSION_FROM_SETUP_ACTION'
   CLI_REPOSITORY='OWNER/REPOSITORY_FROM_SETUP_ACTION'
   dir="$(mktemp -d)"
   archive="$dir/backstage-aws-x86_64-unknown-linux-gnu.tar.gz"
   gh release download "v${CLI_VERSION}" \
     --repo "$CLI_REPOSITORY" \
     --pattern backstage-aws-x86_64-unknown-linux-gnu.tar.gz \
     --dir "$dir"
   gh attestation verify "$archive" --repo "$CLI_REPOSITORY"
   tar -xzf "$archive" -C "$dir"
   chmod +x "$dir/backstage-aws"
   test "$("$dir/backstage-aws" --version)" = "backstage-aws $CLI_VERSION"
   "$dir/backstage-aws" check --release-archive "$archive"
   "$dir/backstage-aws" check --runtime
   ```

   Expected: all commands exit 0; attestation, CLI version, release archive, and runtime checks pass. Each `check` command emits the CLI's JSON success document. Do not substitute an unattested build.
3. Store non-secret deploy settings in the reviewed `environments/preview.env` or `environments/production.env`. They are shell code: load only trusted branch files. Keep credentials in the provider's short-lived identity mechanism, not settings files or job logs.

## Deploy from GitLab

4. Configure GitLab workload identity to assume the provider-specific AWS role. Use a protected runner and serialize jobs by merge-request IID:

   ```yaml
   deploy_preview:
     stage: deploy
     rules:
       - if: '$CI_PIPELINE_SOURCE == "merge_request_event" && $CI_MERGE_REQUEST_SOURCE_PROJECT_PATH == $CI_PROJECT_PATH'
     resource_group: "preview-$CI_MERGE_REQUEST_IID"
     script:
       - set -a
       - . environments/preview.env
       - set +a
       - BACKSTAGE_AWS_PULL_REQUEST="$CI_MERGE_REQUEST_IID" backstage-aws env up --approve
   ```

   Expected: the deploy command exits 0 and returns the standard `env up` JSON fields `namespace`, `url`, `image_digest`, `timings`, and `comment_url`. GitLab does not get a GitHub PR comment from the CLI; the comment URL is null unless a GitHub PR token and matching repository are configured. The sample excludes forks.
5. For trusted cleanup, load the same settings and run:

   ```sh
   set -a
   . environments/preview.env
   set +a
   BACKSTAGE_AWS_PULL_REQUEST="$CI_MERGE_REQUEST_IID" backstage-aws env down --approve
   ```

   Expected: exit 0 and JSON `result.namespace`, `result.destroyed`, and `result.comment_url`. Do not cancel an active Terraform apply.

## Deploy from Jenkins

6. Configure a protected production job, provider-specific short-lived AWS identity, and a lockable resource. This example assumes the Jenkins Lockable Resources plugin:

   ```groovy
   lock(resource: 'backstage-production') {
     sh '''
       set -a
       . environments/production.env
       set +a
       backstage-aws env up --scan --approve
     '''
   }
   ```

   Expected: exit 0 and `env up` JSON with `namespace`, `url`, `image_digest`, `timings`, and `comment_url` (null without a GitHub PR comment). To deploy an existing digest, use `backstage-aws env up --scan --digest sha256:<digest> --approve`; the CLI requires a current scan result. ECS/ALB readiness does not prove sign-in or application behavior.

## Back up and restore

7. To export a namespace, load its settings and choose a new local file path:

   ```sh
   set -a; . environments/production.env; set +a
   backstage-aws db dump --file ./namespace.dump --approve
   ```

   Expected: exit 0 and JSON `result.bucket`, `result.key`, `result.database`, and `result.file`. The custom-format dump contains one database without owner or ACL entries. To restore it as the seed for future namespaces, load the target account's settings and run:

   ```sh
   set -a; . environments/preview.env; set +a
   backstage-aws seed --file ./namespace.dump --approve
   ```

   Expected: exit 0 and JSON `result.bucket`, `result.key`, `result.operation_id`, and `result.resumed`. This does not restore an existing namespace, RDS snapshot, Terraform state, or secrets. See [Back up and restore](backup-and-restore.md).
