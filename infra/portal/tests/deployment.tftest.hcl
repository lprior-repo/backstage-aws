mock_provider "aws" {
  mock_data "aws_availability_zones" {
    defaults = { names = ["us-east-1a", "us-east-1b"] }
  }
  mock_data "aws_caller_identity" {
    defaults = { account_id = "123456789012" }
  }
  mock_data "aws_partition" {
    defaults = { partition = "aws" }
  }
  mock_resource "aws_rds_cluster" {
    defaults = {
      master_user_secret = [{
        secret_arn    = "arn:aws:secretsmanager:us-east-1:123456789012:secret:db-test"
        secret_status = "active"
        kms_key_id    = "test"
      }]
    }
  }
}
variables {
  region                   = "us-east-1"
  hostname                 = "backstage.example.com"
  route53_zone_id          = "Z123456789"
  image                    = "123456789012.dkr.ecr.us-east-1.amazonaws.com/backstage@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  app_secret_arn           = "arn:aws:secretsmanager:us-east-1:123456789012:secret:oauth-test"
  github_oidc_provider_arn = "arn:aws:iam::123456789012:oidc-provider/token.actions.githubusercontent.com"
  allowed_cidrs            = ["192.0.2.10/32"]
}
run "private_redundant_service" {
  command = plan
  variables { service_count = 2 }
  assert {
    condition     = aws_ecs_service.portal.desired_count == 2 && !aws_ecs_service.portal.network_configuration[0].assign_public_ip
    error_message = "Portal tasks must remain private and redundant."
  }
  assert {
    condition     = aws_ecs_service.portal.deployment_circuit_breaker[0].rollback
    error_message = "Unhealthy deployments must roll back."
  }
  assert {
    condition     = aws_rds_cluster.db.storage_encrypted && aws_rds_cluster.db.deletion_protection && aws_rds_cluster.db.backup_retention_period >= 7
    error_message = "Database encryption, deletion protection and backups are required."
  }
  assert {
    condition     = aws_lb_listener.https.protocol == "HTTPS" && aws_lb_listener.https.port == 443
    error_message = "Portal traffic must use HTTPS."
  }
}
run "reject_mutable_image" {
  command = plan
  variables { image = "123456789012.dkr.ecr.us-east-1.amazonaws.com/backstage:latest" }
  expect_failures = [var.image]
}
run "initial_service_stays_stopped" {
  command = plan
  assert {
    condition     = aws_ecs_service.portal.desired_count == 0
    error_message = "The service must stay stopped until its database login is initialized."
  }
}
run "reject_empty_ingress" {
  command = plan
  variables { allowed_cidrs = [] }
  expect_failures = [var.allowed_cidrs]
}
