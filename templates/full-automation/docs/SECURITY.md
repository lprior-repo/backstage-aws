# Security design and operational limits

This document describes implemented controls. It is not a security certification
or evidence of a completed live AWS deployment. Read `VALIDATION.md` for the actual
checks and remaining verification work.

## Network and transport

- ALB ingress is restricted to a required list of explicit IPv4 CIDRs. The
  example is deliberately unusable until replaced. Port 80 redirects to HTTPS;
  application traffic uses an ACM certificate and a TLS 1.2/1.3 policy.
- Fargate tasks run in private application subnets with `assign_public_ip=false`.
  The portal security group permits port 7007 only from the ALB security group.
- RDS is not publicly accessible. Its isolated subnet route tables have no
  internet route; database ingress is limited to the portal/bootstrap groups.
- RDS enforces TLS. Both application and bootstrap validate the certificate with
  the bundled AWS RDS CA bundle; `rejectUnauthorized` remains enabled.
- ALB-to-container traffic is HTTP within the VPC. The portal filesystem is
  writable because Backstage may write runtime frontend configuration and task
  data; the database job uses a read-only root filesystem. Neither task is root
  or privileged, and ECS Exec is disabled.
- Application/bootstrap outbound access permits HTTPS needed for GitHub and AWS
  APIs plus database traffic. NAT and security groups do not implement a hostname
  allowlist. Use an egress proxy/firewall if your organization requires one.
- The initial endpoint is a public ALB with network restrictions and application
  authentication. It is not an internal ALB, WAF deployment, VPN, or Zero Trust
  access gateway. Do not claim that all frontend assets are protected by OAuth.

## Identity and authorization

GitHub's immutable numeric user ID must match the `github.com/user-id` annotation
of a trusted local catalog User. There is no guest provider, username-only
fallback, email-local-part fallback, or automatic admission of unknown identities.

The source-controlled `platform-operators` group is the authority for rollout
operations. Authenticated users can read catalog/template metadata. Operators can
create/read scaffolder tasks and execute only `aws:backstage:rollout`. Catalog
mutations, arbitrary actions, inline dry runs, task cancellation, and unrecognized
permissions deny by default. Adding plugins may require deliberate policy updates;
silently allowing all permissions would defeat this boundary.

Users cannot choose a state-machine ARN, task definition, IAM role, Docker image,
container command, network, or Terraform source in the included template. Its
optional reason is bounded metadata. The backend derives request identity from
the scaffolder task and reconciles duplicate execution submissions against the
original exact payload.

The role can still initiate the approved workflow if the portal is compromised.
That is an intentional capability, not an assertion of isolation from every AWS
effect. An attacker with permission to replace the app image, modify Terraform,
or change the state machine crosses a stronger administrative boundary. Protect
the source repository, release approvals, deployment identity, and ECR write access.

## IAM capabilities

| Principal | Intended capability |
| --- | --- |
| Portal execution role | Pull this ECR image, write its log stream, inject only app-DB and identity secrets |
| Portal task role | Start the one rollout state machine; describe executions of that state machine |
| Bootstrap execution role | Pull this ECR image and write bootstrap logs; no secret injection |
| Bootstrap task role | Read the RDS-managed admin and app-DB secrets, write only the app-DB secret, use the relevant KMS key via Secrets Manager |
| Step Functions execution role | Run the exact approved bootstrap task revision in this cluster; pass only fixed ECS roles; update only this portal service with the approved task definition; describe/stop tasks in this cluster; support the optimized integration's event/log delivery |
| Terraform operator | Provision and modify the declared AWS resources and read/write the encrypted remote state; separate from runtime identities |

The ECR authorization-token API and some CloudWatch log-delivery APIs require
wildcard resource scopes. These exceptions are explicit in source; they do not
grant wildcard ECR repository actions, `iam:PassRole`, ECS RunTask, or Step
Functions StartExecution. Task/service role trust is restricted by source account
and supported source-ARN conditions. KMS secret access is constrained by service
and secret encryption context.

`operator_release_policy_json` is an example of narrow release capabilities
after provisioning. It does not grant all Terraform apply permissions or attach
itself to your identity. Inspect it against your organization's deployment model.

## Database credentials and bootstrap

Terraform creates secret metadata and asks RDS to manage its administrator
password. It does not create/read a `secret_string` value. Plaintext credentials
are therefore absent from this project's Terraform variables and state. Secret
ARNs, network metadata, and other infrastructure data still make state sensitive.

The fixed bootstrap task acquires a PostgreSQL advisory lock before reading or
creating the app secret. It generates a random high-entropy base64url password
only when no app-secret version exists. It persists the secret before reconciling
the role so a crash can be recovered with the same credential. A concurrent
bootstrap fails before secret mutation; rerun the workflow after the first run
finishes. It does not silently rotate passwords on each rollout.

The app account can connect to and create plugin schemas in the one application
database. It must not be a superuser, role creator, database creator, replication
user, or RLS bypass role. The bootstrap rejects unexpected privilege elevation
or role memberships rather than treating a preexisting elevated role as safe.
Backstage uses `pluginDivisionMode: schema` and small per-plugin pools.

The bootstrap computes a standard SCRAM-SHA-256 verifier with Node's crypto
primitives before issuing `ALTER ROLE`. This avoids putting the plaintext app
password in the SQL statement. Verifiers remain sensitive; restrict PostgreSQL
activity and log access. Application logs never intentionally print credentials
or SQL. The relevant server-side warning is documented in
[PostgreSQL ALTER ROLE](https://www.postgresql.org/docs/16/sql-alterrole.html).

The application secret is static until an operator coordinates a credential
change. RDS's separately managed administrator password can rotate independently.
Secrets injected into ECS tasks refresh only when tasks start. Do not enable
uncoordinated app-secret rotation without a matching DB update and rollout.

## Image and dependency trust

- Backstage package pins are derived from its published 1.56.5 release manifest;
  Yarn locks the transitive graph and checks package integrity.
- The generated package-age safety window is retained. The fresh Backstage patch
  is explicitly allowed through the generator's Backstage rule; other package
  updates remain subject to the age setting.
- The Node image is pinned by tag and manifest digest. Docker dependencies are
  installed in a separate build stage, with only the app runtime copied forward.
- ECR tags are immutable and deployment uses a `sha256` digest. Scan-on-push is
  enabled. A scan finding is not automatically blocked by this starter; define
  your severity/exception policy before wider rollout.
- The runtime image also contains the fixed database job. That code runs with
  elevated database capability only under its separate bootstrap task role.
- CI actions use full commit SHAs, read-only repository permission, and no stored
  AWS key. The included workflow validates source; it does not publish or deploy.

Build-time third-party Node tooling may require Python/C++; the Docker runtime
stage excludes those build tools. No claim of a completely Python-free build
dependency graph is made.

## State, backups, and logs

Terraform state has KMS encryption, S3 versioning, public-access blocking, TLS
enforcement, native lockfiles, and deletion protection. Keep bootstrap and core
state in separate keys. Protect the saved backend configuration and deployment
identity. Do not run two Terraform applies against the same state concurrently.

RDS has encrypted storage, 14-day automated retention, retained backups and a
final snapshot on deletion, plus deletion protection by default. Multi-AZ improves
availability; it does not replace restore testing. Snapshot recovery and a full
disaster-recovery drill have not been executed for this starter.
The application KMS key also has `prevent_destroy`: encrypted snapshots and
retained backups require that key to remain enabled. Do not schedule its deletion
while retained data still depends on it; manage retained keys independently if
decommissioning the compute/database stack.

CloudWatch app/bootstrap/workflow/flow groups are encrypted with explicit
retention. ALB access logs use S3-managed encryption, which matches ALB log-delivery
requirements, and a dedicated blocked-public bucket. Retained logs and their
access need organizational review; authenticated user identifiers and deployment
metadata can still be sensitive. Account-wide CloudTrail/Config/Security Hub,
WAF rules, alert destinations, backup restore drills, and incident response are
organization-level extensions, not silently provisioned here.

