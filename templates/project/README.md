# Backstage on AWS

Backstage application plus Terraform for AWS Fargate and Aurora PostgreSQL. Local development works without an AWS account. AWS resources have not been deployed or acceptance-tested.

## Run locally

Use Node 24.14.0, pinned in `mise.toml`. Yarn 4.13.0 is included in the repository.

```sh
mise exec node@24.14.0 -- node .yarn/releases/yarn-4.13.0.cjs install --immutable
mise exec node@24.14.0 -- node .yarn/releases/yarn-4.13.0.cjs start
```

Open http://localhost:3000 and enter as Guest. Development uses an in-memory SQLite database and the generated sample catalog; restarting discards local database changes. Backend listens on port 7007. For loopback-only development, set `APP_CONFIG_backend_listen_host=127.0.0.1`.

## AWS architecture

Route 53 and ACM provide an HTTPS hostname on an Application Load Balancer. The ALB reaches two Fargate tasks in private subnets across two availability zones. Aurora PostgreSQL has two encrypted Serverless v2 instances in isolated database subnets, verified TLS connections, seven-day backups, and deletion protection. Two NAT gateways provide private-task egress. Access to the ALB requires an explicit CIDR allowlist. No Kubernetes cluster is needed.

ECR stores immutable application images. Secrets Manager injects OAuth credentials and a dedicated PostgreSQL login; secret values never enter Terraform. A separate, one-time ECS task creates that login using the managed Aurora administrator secret. Portal tasks cannot read the administrator secret. TechDocs reads generated documentation from a private S3 bucket; documentation generation happens outside the portal. CloudWatch captures container logs. GitHub Actions builds and deploys the application using OIDC and the AWS-maintained ECS actions.

Production uses the standalone `app-config.production.yaml`; the Docker command does not merge development configuration. Production has GitHub sign-in only. `usernameMatchingUserEntityName` requires a matching User in `catalog/organization.yaml`; the initial user is `__BACKSTAGE_AWS_USER__`, configured by the generator. Change this catalog to admit the intended users. All admitted users currently share the standard allow-all permission policy; role-based authorization is not implemented.

## First deployment

Prerequisites: AWS CLI v2, Terraform 1.17.2, Docker with daemon access, an authenticated deployment profile, a Route 53 public hosted zone, a GitHub OAuth application, and an existing GitHub Actions OIDC provider in the target AWS account with audience `sts.amazonaws.com`. Terraform creates a new VPC using `10.42.0.0/16`; check address conflicts before connecting it to other networks. GovCloud and China partitions are not supported by the image URI validation.

This stack creates billable Fargate, ALB, NAT, Aurora, logging and storage resources. Select the account, region, hostname and access networks before planning. Confirm Aurora PostgreSQL 17.7 and Serverless v2 are available in that region using `aws rds describe-db-engine-versions` and `aws rds describe-orderable-db-instance-options`.

1. Copy `infra/bootstrap/terraform.tfvars.example` to `infra/bootstrap/terraform.tfvars` and enter the region and a globally unique state-bucket name. Both roots must use the same project `name` so their ECR repository names match. Run:

   ```sh
   terraform -chdir=infra/bootstrap init -input=false
   terraform -chdir=infra/bootstrap plan -input=false -out=bootstrap.tfplan
   terraform -chdir=infra/bootstrap apply -input=false bootstrap.tfplan
   ```

   This root creates the versioned, encrypted state bucket and immutable ECR repository. Its own state remains local; retain `infra/bootstrap/terraform.tfstate` securely. Do not delete it. Application infrastructure state uses S3 native locking.

2. Build and push the initial application image using the `repository_url` output. Use a unique tag and capture its repository digest:

   ```sh
   corepack yarn install --immutable
   corepack yarn tsc
   corepack yarn build:backend
   aws ecr get-login-password --region "$AWS_REGION" | docker login --username AWS --password-stdin "$ECR_REGISTRY"
   docker build -f packages/backend/Dockerfile -t "$ECR_REPOSITORY_URL:initial" .
   docker push "$ECR_REPOSITORY_URL:initial"
   docker inspect --format='{{index .RepoDigests 0}}' "$ECR_REPOSITORY_URL:initial"
   ```

   Set the referenced environment variables to the selected region, registry host and full repository URL. Use the same Node version for host and image builds. The image runs as `node` and downloads the official RDS certificate bundle during its build.

3. Create the GitHub OAuth app with homepage `https://YOUR_HOSTNAME` and callback `https://YOUR_HOSTNAME/api/auth/github/handler/frame`. Create a Secrets Manager secret using the AWS console with JSON keys `github_client_id`, `github_client_secret`, and `github_token`. The integration token should have only the repository permissions needed by the selected catalog and templates. Use the default Secrets Manager encryption key; custom KMS keys require additional decrypt grants. Keep all values outside Git and the terminal transcript.

4. Copy `infra/portal/terraform.tfvars.example` and `backend.tfbackend.example` to their counterparts without `.example`. Fill the account-specific values, image digest and allowed CIDRs. Leave `service_count = 0` for the initial deployment:

   ```sh
   terraform -chdir=infra/portal init -input=false -backend-config=backend.tfbackend
   terraform -chdir=infra/portal plan -input=false -out=portal.tfplan
   terraform -chdir=infra/portal apply -input=false portal.tfplan
   ```

5. Initialize the database login using the built-in one-time task. Run under the same AWS profile and region as Terraform:

   ```sh
   mkdir -p .cache
   terraform -chdir=infra/portal output -json bootstrap_network > .cache/bootstrap-network.json
   CLUSTER=$(terraform -chdir=infra/portal output -raw ecs_cluster)
   DEFINITION=$(terraform -chdir=infra/portal output -raw bootstrap_task_definition)
   TASK=$(aws ecs run-task --cluster "$CLUSTER" --task-definition "$DEFINITION" --launch-type FARGATE --platform-version 1.4.0 --network-configuration file://.cache/bootstrap-network.json --query 'tasks[0].taskArn' --output text)
   aws ecs wait tasks-stopped --cluster "$CLUSTER" --tasks "$TASK"
   aws ecs describe-tasks --cluster "$CLUSTER" --tasks "$TASK" --query 'tasks[0].containers[0].[exitCode,reason]' --output json
   ```

   Require exit code `0` and the `Database application role and secret created` log message. The bootstrap creates `backstage_app` with database/schema creation privileges for Backstage migrations, without superuser, role-creation or database-creation privileges. It publishes a generated password directly into Secrets Manager. A repeated bootstrap refuses to rotate an existing login. Keep the application stopped if any bootstrap step fails; inspect the `database-bootstrap` stream in `/ecs/PROJECT_NAME`. A secret publication and database commit cannot be atomic across services; after interrupted bootstrap, verify both before enabling the portal.

6. Set `service_count = 2`, plan again and apply the reviewed plan. Require healthy ALB targets and readiness HTTP 200, test real GitHub sign-in, and verify the catalog and database survive replacing a task. Terraform validation and mocked plans do not establish this runtime acceptance.

7. Configure a GitHub `production` environment restricted to `main`, with required reviewers as desired. Set secret `AWS_ROLE_ARN` from `deploy_role_arn`. Set environment variables `AWS_REGION`, `ECR_REPOSITORY` (repository name), `ECS_CLUSTER`, `ECS_SERVICE` and `ECS_TASK_FAMILY`. Run **Deploy portal** from `main`. The workflow builds, tests, pushes a unique image and deploys its immutable digest, waiting for stability. ECS rolls back unhealthy deployments. Terraform ignores service task-definition revisions because CI owns application releases; task-role/network/database changes still go through Terraform. Promote infrastructure changes to the active task definition through the deployment workflow.

## Verify

```sh
corepack yarn install --immutable
corepack yarn tsc
corepack yarn test --watch=false --runInBand
corepack yarn test:bootstrap
corepack yarn lint:all
corepack yarn build:backend
terraform fmt -check -recursive infra
terraform -chdir=infra/bootstrap init -backend=false -input=false
terraform -chdir=infra/bootstrap validate
terraform -chdir=infra/portal init -backend=false -input=false
terraform -chdir=infra/portal validate
terraform -chdir=infra/portal test
```

Terraform tests mock AWS and exercise private/redundant deployment, TLS, database protection, first-deploy shutdown, immutable image validation and explicit ingress. Bootstrap unit tests cover successful credential publication, duplicate-login refusal, secret-write failure and database-grant failure. Neither suite talks to AWS or a real PostgreSQL server. CI also builds the Docker image. Live OAuth, IAM, ECS scheduling, Aurora connectivity, secret rotation and recovery remain AWS acceptance checks. Application password rotation must coordinate a database password change, secret update and new ECS tasks; no automated rotation workflow is included. Database and ALB deletion protection and bucket safeguards require a deliberate infrastructure change before teardown.

## Provisioning workflows

This repository hosts Backstage. Account leasing, tenant isolation, EventBridge submission, Step Functions execution, Rust/Terraform workers and result/status integration from the supplied design are not implemented. Their contracts and approved operations must be defined before exposing infrastructure-changing templates. The generated example scaffolder template is development-only; production loads only the organization catalog.

The intended extension is Backstage template → AWS EventBridge action → scoped event bus/rule → Step Functions → dedicated Fargate worker. Keep portal delivery in the external deployment workflow. Do not grant portal tasks infrastructure deployment privileges.

## References

- [Official Backstage application setup](https://backstage.io/docs/getting-started/)
- [Official Backstage Docker deployment](https://backstage.io/docs/deployment/docker/)
- [Official Backstage authentication](https://backstage.io/docs/auth/)
- [AWS ECS Blueprints Backstage/Fargate/Aurora example](https://github.com/aws-ia/ecs-blueprints/tree/main/terraform/fargate-examples/backstage), inspected at `ee02ffc075c0c2aacad604bc72e8685579ac14a5`; architecture reference, not vendored code
- [AWS Backstage scaffolder actions](https://github.com/awslabs/backstage-plugins-for-aws/tree/main/plugins/core/scaffolder-actions)
- [AWS ECS deployment action](https://github.com/aws-actions/amazon-ecs-deploy-task-definition)
- [Aurora PostgreSQL release notes](https://docs.aws.amazon.com/AmazonRDS/latest/AuroraPostgreSQLReleaseNotes/AuroraPostgreSQL.Updates.html)

Provider versions, the VPC module, Yarn and GitHub Actions revisions are pinned. Commit both Terraform dependency lock files with the application lock file. Generated application baseline: Backstage 1.56.0 from `@backstage/create-app` 0.9.3.
