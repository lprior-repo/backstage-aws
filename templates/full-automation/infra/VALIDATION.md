> Historical baseline evidence from the original source archive. It does not establish validation of the changed automation bundle. See docs/VALIDATION.md at the repository root for current results.

# Infrastructure validation

## Version and provenance

The Terraform CLI is pinned to **1.17.2** in `.terraform-version` and both root modules. The AWS provider is pinned to **6.73.0**. The two `.terraform.lock.hcl` files contain the provider checksums and must be committed with the source.

Primary release records:

- https://github.com/hashicorp/terraform/releases/tag/v1.17.2
- https://releases.hashicorp.com/terraform/1.17.2/
- https://github.com/hashicorp/terraform-provider-aws/releases/tag/v6.73.0

The Linux AMD64 Terraform ZIP used during authoring matched the published SHA256SUMS entry:

```text
e6f16b2f0b137ee5729caa8c4e79a3bae4ee261acd198a1fcfa8584a6296605b  terraform_1.17.2_linux_amd64.zip
```

This verifies the download against HashiCorp's HTTPS-hosted checksum manifest; it is not an independent GPG signature verification of the CLI ZIP. `terraform init` separately reported the AWS provider package as **signed by HashiCorp**. Bootstrap reused that same downloaded provider with the copied checksum lockfile.

## Completed checks

- Terraform's formatter parsed all HCL source, examples, and test configuration; formatting was applied.
- `terraform init -backend=false -input=false` completed for the core module, including signed provider installation.
- Bootstrap initialization completed using the same provider and its existing lockfile.
- Source review checked the resource dependency graph, role separation, exact workflow target ARNs, private networking, externally populated secrets, explicit database name, and ownership of service activation/revisions.
- A provider-free renderer evaluates the actual `workflow.tf` expression with fixed fixture identifiers. The resulting ASL fixtures support offline state-graph and JSONata tests without AWS access.
- Four offline structure tests passed: valid transition destinations, the unpublished-image gate, fixed AWS request arguments, and bootstrap-before-deployment ordering.
- Ten independently authored JSONata test groups passed with JSONata **2.0.6**, the version documented by AWS, and Node **24.22.1**. These cover malformed inputs, container identity/exit status, primary-deployment selection, missing service/failure responses, deployment identity and desired/running counts, rollback/replacement rejection, and timeout on the sixtieth observation. They evaluate the actual rendered expressions, not an AWS service emulator.

## Environment limitation

`terraform validate` could not load the AWS provider schema in the authoring executor. Diagnostic output identifies the restriction precisely:

```text
plugin init error: listen unix /tmp/plugin...: socket: operation not permitted
```

The executable architecture, file mode, and provider startup were checked. The failure is the provider's local RPC socket, not an AWS API failure. No sandbox bypass or AWS deployment was attempted. Mock-provider tests also need that real provider schema, so they must run in a normal CI runner or workstation that supports local sockets. Their presence is not a claim that they passed in this executor.

The following remain unverified until those checks and an AWS deployment run: provider schema validation, real AWS IAM evaluation, ASL validation by Step Functions, image startup on Fargate, database TLS connectivity, DNS/certificate provisioning, bootstrap permissions, and deployment completion/rollback behavior.

## Commands to rerun

From `infra/` with Terraform 1.17.2:

```sh
terraform fmt -check -recursive
terraform -chdir=bootstrap init -backend=false -input=false
terraform -chdir=bootstrap validate
terraform -chdir=bootstrap test
terraform -chdir=core init -backend=false -input=false
terraform -chdir=core validate
terraform -chdir=core test
npm ci --prefix tools --ignore-scripts
node tools/render-workflow-fixture.mjs
npm test --prefix tools
```

The Terraform tests use `mock_provider "aws"` and never deploy AWS resources. They check the initial zero-task state, private tasks and database, managed administrator credentials, default availability/backups, readiness path, restricted runtime IAM, fixed approved task revisions, image publication gate, bounded workflow duration, and invalid account/image/ingress inputs.

The core mock generates computed fixtures during planning (`override_during = plan`) so ACM validation records can supply known Route53 `for_each` keys and policy/ASL assertions have deterministic values. All runs are plan-only. The application KMS key's `prevent_destroy` lifecycle setting is checked by source review; it is not an exposed resource attribute asserted by these mocks. That key must be retained as long as any encrypted RDS snapshot or backup is needed.

The portal task definition sets `skip_destroy = true`, retaining old approved revisions for intentional rollback. Support and semantics were checked in the [pinned AWS provider 6.73.0 documentation](https://github.com/hashicorp/terraform-provider-aws/blob/v6.73.0/website/docs/r/ecs_task_definition.html.markdown). Retiring unused old portal revisions is an explicit operator task. Bootstrap revisions keep the provider's default lifecycle.

After obtaining real Terraform outputs, validate the fully rendered ASL with the AWS validation API before starting an execution:

```sh
terraform -chdir=core output -raw rollout_definition > rollout.asl.json
aws stepfunctions validate-state-machine-definition \
  --type STANDARD --definition file://rollout.asl.json
```

This final command validates a definition; it does not execute it. A successful API validation is still not proof of IAM permissions or runtime behavior. Perform the first rollout in a dedicated account/environment and verify both successful deployment and an intentionally failing image rollback before production use.
