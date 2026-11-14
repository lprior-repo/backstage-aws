# Keep the implementation small

| Decision | Implemented choice | Alternative and tradeoff |
| --- | --- | --- |
| ECS compute | Fargate, private tasks | ECS on EC2 can improve sustained utilization economics but adds host AMIs, patching, capacity and drain operations; it is not implemented here |
| Deployment orchestration | Standard Step Functions, JSONata | No Lambda adapter is needed for the included native AWS API/ECS integrations |
| Image builds | Docker on GitHub-hosted runner or operator host | Not CodeBuild; this does not assume privileged Docker-in-Docker runs on Fargate |
| IaC | Native AWS Terraform resources | Few dependencies and explicit policy/resource boundaries; no CloudFormation |
| App runtime | Backstage's Node app + PostgreSQL | Optional inherited Rust operator helper; no Python application/runtime component |
| Accounts | Dedicated application account | Use existing Organizations processes if required; no Control Tower or landing zone |
| Egress | NAT, two gateways by default | Single NAT is an explicit availability/cost compromise; endpoint-only networks require validating all Backstage integration dependencies |
| Availability | Two tasks and Multi-AZ RDS | Evaluation-only settings can reduce task count/NAT/RDS redundancy |
| Secrets | AWS Secrets Manager + ECS injection | Running containers need replacement after a value changes |
| Public edge | HTTPS ALB restricted to explicit CIDRs + app auth | VPN-only/internal ALB/WAF are separate requirements; no claim that this is a public multi-tenant product |

serverless.tf is useful when a Lambda/S3/EventBridge function pattern is actually needed. This implementation uses native Step Functions ECS/AWS SDK integrations, so inserting a Lambda and serverless.tf module would add a component without a job. Reuse terraform-aws-modules/serverless.tf patterns for a future bounded custom adapter; do not add an unused adapter just to make it part of the stack.

The code deliberately does not implement automatic AWS account creation, domain purchase/delegation, OAuth registration, organization policies, a fleet-wide landing zone, arbitrary app/plugin migration recovery, or a full EC2 variant. Those require separate account/identity/ownership or application contracts. All live checks are narrow evidence, not a production/security certification.

References:
- https://serverless.tf/
- https://github.com/terraform-aws-modules/terraform-aws-ecs
- https://backstage.io/docs/deployment/docker/
- https://docs.aws.amazon.com/step-functions/latest/dg/transforming-data.html
- https://docs.aws.amazon.com/step-functions/latest/dg/connect-ecs.html
- https://developer.hashicorp.com/terraform/language/backend/s3
