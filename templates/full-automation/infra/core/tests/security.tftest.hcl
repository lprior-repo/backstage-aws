# Mocked provider: these tests never create AWS resources and require no AWS credentials.
mock_provider "aws" {
  # ACM validation records determine Route53 for_each keys during planning.
  # Plan-time computed fixtures also let every assertion run without applying
  # mock state that would later need to destroy the protected application key.
  override_during = plan

  mock_data "aws_caller_identity" {
    defaults = { account_id = "123456789012" }
  }
  mock_data "aws_partition" {
    defaults = { partition = "aws" }
  }
  mock_data "aws_availability_zones" {
    defaults = { names = ["us-east-1a", "us-east-1b"] }
  }
  mock_resource "aws_db_instance" {
    defaults = {
      address               = "backstage-postgres.example.us-east-1.rds.amazonaws.com"
      port                  = 5432
      engine_version_actual = "16.10"
      master_user_secret = [{
        secret_arn    = "arn:aws:secretsmanager:us-east-1:123456789012:secret:rds!cluster-admin-abcdef"
        kms_key_id    = "arn:aws:kms:us-east-1:123456789012:key/11111111-1111-1111-1111-111111111111"
        secret_status = "active"
      }]
    }
  }
  mock_resource "aws_ecr_repository" {
    defaults = {
      arn            = "arn:aws:ecr:us-east-1:123456789012:repository/backstage/portal"
      repository_url = "123456789012.dkr.ecr.us-east-1.amazonaws.com/backstage/portal"
    }
  }
  mock_resource "aws_acm_certificate" {
    defaults = {
      arn = "arn:aws:acm:us-east-1:123456789012:certificate/11111111-1111-1111-1111-111111111111"
      domain_validation_options = [{
        domain_name           = "backstage.example.com"
        resource_record_name  = "_validation.backstage.example.com"
        resource_record_type  = "CNAME"
        resource_record_value = "_validation.acm-validations.aws."
      }]
    }
  }
  mock_resource "aws_acm_certificate_validation" {
    defaults = {
      certificate_arn = "arn:aws:acm:us-east-1:123456789012:certificate/11111111-1111-1111-1111-111111111111"
    }
  }
  mock_resource "aws_kms_key" {
    defaults = { arn = "arn:aws:kms:us-east-1:123456789012:key/11111111-1111-1111-1111-111111111111" }
  }
  mock_resource "aws_cloudwatch_log_group" {
    defaults = { arn = "arn:aws:logs:us-east-1:123456789012:log-group:/backstage/test" }
  }
  mock_resource "aws_iam_role" {
    defaults = { arn = "arn:aws:iam::123456789012:role/test-role" }
  }
  mock_resource "aws_ecs_cluster" {
    defaults = { arn = "arn:aws:ecs:us-east-1:123456789012:cluster/backstage-cluster" }
  }
  mock_resource "aws_ecs_service" {
    defaults = { id = "arn:aws:ecs:us-east-1:123456789012:service/backstage-cluster/backstage-portal" }
  }
  mock_resource "aws_security_group" {
    defaults = { id = "sg-11111111" }
  }
  mock_resource "aws_dynamodb_table" {
    defaults = { arn = "arn:aws:dynamodb:us-east-1:123456789012:table/backstage-release-lock" }
  }
  mock_resource "aws_secretsmanager_secret" {
    defaults = { arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:test-plugin-abcdef" }
  }
}

override_resource {
  target          = aws_ecs_task_definition.portal
  override_during = plan
  values          = { arn = "arn:aws:ecs:us-east-1:123456789012:task-definition/backstage-portal:7" }
}
override_resource {
  target          = aws_ecs_task_definition.bootstrap
  override_during = plan
  values          = { arn = "arn:aws:ecs:us-east-1:123456789012:task-definition/backstage-db-bootstrap:3" }
}
override_resource {
  target          = aws_ecs_task_definition.rotate
  override_during = plan
  values          = { arn = "arn:aws:ecs:us-east-1:123456789012:task-definition/backstage-db-rotate:2" }
}
override_resource {
  target          = aws_subnet.application[0]
  override_during = plan
  values          = { id = "subnet-11111111" }
}
override_resource {
  target          = aws_subnet.application[1]
  override_during = plan
  values          = { id = "subnet-22222222" }
}
override_resource {
  target          = aws_secretsmanager_secret.database_application
  override_during = plan
  values          = { arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:test-database-abcdef" }
}
override_resource {
  target          = aws_secretsmanager_secret.identity
  override_during = plan
  values          = { arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:test-identity-abcdef" }
}

variables {
  aws_account_id        = "123456789012"
  aws_region            = "us-east-1"
  domain_name           = "backstage.example.com"
  route53_zone_id       = "Z0123456789EXAMPLE"
  allowed_ingress_cidrs = ["203.0.113.10/32"]
}

run "infrastructure_starts_closed" {
  command = plan

  assert {
    condition     = aws_ecs_service.portal.desired_count == 0 && !aws_ecs_service.portal.network_configuration[0].assign_public_ip
    error_message = "The initial service must have zero tasks and must not assign public IPs."
  }
  assert {
    condition     = aws_ecs_task_definition.portal.skip_destroy
    error_message = "Portal revisions must remain available for intentional rollback until operators retire them."
  }
  assert {
    condition     = !aws_db_instance.database.publicly_accessible && aws_db_instance.database.manage_master_user_password && aws_db_instance.database.storage_encrypted
    error_message = "RDS must remain private, encrypted, and use its managed master secret."
  }
  assert {
    condition     = aws_db_instance.database.db_name == var.database_name && aws_db_instance.database.multi_az && aws_db_instance.database.backup_retention_period >= 7
    error_message = "Bootstrap needs the named DB, with HA and backups enabled by default."
  }
  assert {
    condition     = length(aws_subnet.application) == 2 && length(aws_subnet.database) == 2 && length(aws_nat_gateway.main) == 2
    error_message = "The default topology must isolate application/database subnets across two AZs."
  }
  assert {
    condition     = jsondecode(aws_sfn_state_machine.rollout.definition).States.ValidateRequest.Choices[0].Next == "ImageNotPublished"
    error_message = "The placeholder image must not permit a rollout."
  }
  assert {
    condition     = jsondecode(aws_iam_role_policy.portal_task.policy).Statement[0].Action == "states:StartExecution" && jsondecode(aws_iam_role_policy.portal_task.policy).Statement[0].Resource == local.rollout_arn
    error_message = "The portal may start only its fixed rollout workflow."
  }
  assert {
    condition     = !strcontains(aws_iam_role_policy.portal_task.policy, "secretsmanager:") && !strcontains(aws_iam_role_policy.portal_task.policy, "ecs:")
    error_message = "The portal task must not gain database-secret or ECS mutation permissions."
  }
  assert {
    condition     = !strcontains(aws_iam_role_policy.portal_execution.policy, aws_db_instance.database.master_user_secret[0].secret_arn)
    error_message = "The portal execution role must never read the RDS administrator secret."
  }
  assert {
    condition     = !strcontains(aws_iam_role_policy.bootstrap_execution.policy, "secretsmanager:")
    error_message = "The bootstrap execution role must stay limited to ECR and logging."
  }
  assert {
    condition     = jsondecode(aws_iam_role_policy.rollout.policy).Statement[2].Condition.ArnEquals["ecs:task-definition"] == aws_ecs_task_definition.portal.arn
    error_message = "Workflow IAM must independently restrict UpdateService to the approved task revision."
  }
  assert {
    condition     = aws_lb_target_group.portal.health_check[0].path == "/.backstage/health/v1/readiness"
    error_message = "The ALB must use Backstage readiness, rather than a UI or authenticated route."
  }
}

run "published_image_uses_fixed_approved_resources" {
  command = plan
  variables {
    image_digest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  }
  assert {
    condition     = jsondecode(aws_sfn_state_machine.rollout.definition).States.ValidateRequest.Choices[0].Next == "RememberRequest"
    error_message = "Applying an immutable digest should enable the rollout."
  }
  assert {
    condition     = jsondecode(aws_sfn_state_machine.rollout.definition).States.BootstrapDatabase.Arguments.TaskDefinition == aws_ecs_task_definition.bootstrap.arn && jsondecode(aws_sfn_state_machine.rollout.definition).States.DeployPortal.Arguments.TaskDefinition == aws_ecs_task_definition.portal.arn
    error_message = "Both tasks must be fixed by infrastructure, never supplied in request input."
  }
  assert {
    condition     = !can(jsondecode(aws_sfn_state_machine.rollout.definition).States.BootstrapDatabase.Arguments.Overrides)
    error_message = "The approved bootstrap must not permit command, role, or environment overrides."
  }
  assert {
    condition     = jsondecode(aws_sfn_state_machine.rollout.definition).TimeoutSeconds == 1200 && jsondecode(aws_sfn_state_machine.rollout.definition).States.BootstrapDatabase.TimeoutSeconds == 300
    error_message = "Workflow and bootstrap duration must remain bounded."
  }
  assert {
    condition     = strcontains(jsondecode(aws_sfn_state_machine.rollout.definition).States.CheckDeployment.Choices[1].Condition, "$deploymentId") && strcontains(jsondecode(aws_sfn_state_machine.rollout.definition).States.CheckDeployment.Choices[1].Condition, "COMPLETED")
    error_message = "Success must identify this deployment and require ECS completion, so rollback is not mistaken for success."
  }
  assert {
    condition     = endswith(jsondecode(aws_ecs_task_definition.portal.container_definitions)[0].image, "@${var.image_digest}")
    error_message = "The task must use an immutable repository image digest."
  }
}

run "reject_mutable_or_invalid_image" {
  command = plan
  variables {
    image_digest = "latest"
  }
  expect_failures = [var.image_digest]
}

run "reject_empty_ingress_allowlist" {
  command = plan
  variables {
    allowed_ingress_cidrs = []
  }
  expect_failures = [var.allowed_ingress_cidrs]
}

run "reject_invalid_account_id" {
  command = plan
  variables {
    aws_account_id = "wrong-account"
  }
  expect_failures = [var.aws_account_id]
}

run "password_maintenance_is_not_a_portal_capability" {
  command = plan
  variables {
    image_digest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  }
  assert {
    condition     = jsondecode(aws_sfn_state_machine.database_rotation.definition).QueryLanguage == "JSONata" && jsondecode(aws_sfn_state_machine.rollout.definition).QueryLanguage == "JSONata"
    error_message = "Both workflows must be JSONata-only."
  }
  assert {
    condition     = !strcontains(aws_iam_role_policy.portal_task.policy, local.rotation_arn)
    error_message = "The portal must not start privileged database-password maintenance."
  }
  assert {
    condition     = aws_dynamodb_table.release_lock.deletion_protection_enabled && aws_dynamodb_table.release_lock.point_in_time_recovery[0].enabled
    error_message = "The release mutex needs protected persistent state."
  }
  assert {
    condition     = jsondecode(aws_sfn_state_machine.rollout.definition).States.RememberRequest.Next == "AcquireReleaseLock"
    error_message = "A release must acquire its mutex before any external task or service mutation."
  }
}
run "extra_plugin_secret_uses_only_its_scoped_injection" {
  command = plan
  variables {
    application_secrets = {
      github = { environment_variable = "GITHUB_TOKEN", json_key = "token" }
    }
  }
  assert {
    condition     = length(jsondecode(aws_ecs_task_definition.portal.container_definitions)[0].secrets) == 6
    error_message = "One configured plugin secret must create exactly one additional injection."
  }
  assert {
    condition     = jsondecode(aws_ecs_task_definition.portal.container_definitions)[0].secrets[5].valueFrom == "${aws_secretsmanager_secret.application["github"].arn}:token::"
    error_message = "The plugin must receive only its selected secret JSON key."
  }
  assert {
    condition     = !strcontains(aws_iam_role_policy.portal_task.policy, "secretsmanager:")
    error_message = "Injected plugin secrets must not give the running portal direct Secrets Manager access."
  }
}
run "reject_world_open_ingress" {
  command = plan
  variables { allowed_ingress_cidrs = ["0.0.0.0/0"] }
  expect_failures = [var.allowed_ingress_cidrs]
}
run "reject_reserved_secret_environment" {
  command = plan
  variables { application_secrets = { wrong = { environment_variable = "POSTGRES_PASSWORD" } } }
  expect_failures = [var.application_secrets]
}
