mock_provider "aws" {}
mock_provider "random" {}

run "base_security_and_outputs" {
  command = plan
  override_data {
    target = data.aws_availability_zones.available
    values = {
      names = ["us-east-1a", "us-east-1b"]
    }
  }
  override_data {
    target = data.aws_caller_identity.current
    values = {
      account_id = "123456789012"
      arn        = "arn:aws:iam::123456789012:user/test"
      user_id    = "AIDATEST"
    }
  }
  override_data {
    target = data.aws_partition.current
    values = {
      partition = "aws"
    }
  }
  override_resource {
    target          = aws_db_instance.database
    override_during = plan
    values = {
      master_user_secret = [{ secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:master" }]
    }
  }
  override_resource {
    target          = aws_secretsmanager_secret.app_db
    override_during = plan
    values = {
      arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:application"
    }
  }
  override_resource {
    target          = aws_secretsmanager_secret.session
    override_during = plan
    values = {
      arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:session"
    }
  }
  override_resource {
    target          = aws_kms_key.app
    override_during = plan
    values = {
      arn = "arn:aws:kms:us-east-1:123456789012:key/00000000-0000-0000-0000-000000000000"
    }
  }
  variables {
    aws_account_id        = "123456789012"
    aws_region            = "us-east-1"
    name                  = "backstage"
    environment           = "preview"
    domain_name           = "preview.example.com"
    route53_zone_id       = "Z123456789ABCDE"
    allowed_ingress_cidrs = ["192.0.2.0/24"]
  }

  assert {
    condition     = toset([for rule in aws_vpc_security_group_ingress_rule.https : rule.cidr_ipv4]) == toset(["192.0.2.0/24"])
    error_message = "HTTPS ingress must be exactly the allowlist."
  }
  assert {
    condition     = toset([for rule in aws_vpc_security_group_ingress_rule.http : rule.cidr_ipv4]) == toset(["192.0.2.0/24"])
    error_message = "HTTP ingress must be exactly the allowlist."
  }
  assert {
    condition     = toset(keys(aws_vpc_security_group_egress_rule.https)) == toset(["app", "admin"]) && toset(keys(aws_vpc_security_group_egress_rule.to_database)) == toset(["app", "admin"]) && aws_vpc_security_group_egress_rule.alb_to_app.from_port == 7007
    error_message = "Tasks need HTTPS and database egress, and the load balancer needs egress to the application port; Terraform removes the default egress rule."
  }
  assert {
    condition     = aws_lb_listener.https[0].default_action[0].type == "fixed-response" && aws_lb_listener.https[0].default_action[0].fixed_response[0].status_code == "404"
    error_message = "HTTPS listener must default to a fixed 404."
  }
  assert {
    condition     = aws_db_instance.database.publicly_accessible == false && aws_db_instance.database.storage_encrypted && aws_db_instance.database.manage_master_user_password && aws_db_instance.database.password == null
    error_message = "Database must be private, encrypted and use an RDS-managed master password without a password argument."
  }
  assert {
    condition     = contains([for parameter in aws_db_parameter_group.database.parameter : "${parameter.name}=${parameter.value}"], "rds.force_ssl=1")
    error_message = "PostgreSQL must require SSL."
  }
  assert {
    condition     = aws_s3_bucket_public_access_block.seed.block_public_acls && aws_s3_bucket_public_access_block.seed.block_public_policy && aws_s3_bucket_public_access_block.seed.ignore_public_acls && aws_s3_bucket_public_access_block.seed.restrict_public_buckets
    error_message = "Seed bucket must block public access."
  }
  assert {
    condition     = length(aws_nat_gateway.main) == 0 && output.assign_public_ip && output.db_admin_network_configuration.awsvpcConfiguration.assignPublicIp == "ENABLED" && alltrue([for route in aws_route.application : route.nat_gateway_id == null])
    error_message = "Without NAT gateways, tasks must get public addresses and route through the internet gateway."
  }
  assert {
    condition     = aws_s3_bucket_versioning.seed.versioning_configuration[0].status == "Enabled"
    error_message = "Seed and dump objects must stay recoverable after being replaced."
  }
  assert {
    condition     = !strcontains(local.application_execution_secrets_policy, aws_db_instance.database.master_user_secret[0].secret_arn)
    error_message = "Application execution role must not read the master secret."
  }
  # The portal must not be able to read the lifecycle credential, so an
  # attacker-controlled portal revision cannot clone or drop databases.
  assert {
    condition     = !strcontains(local.application_execution_secrets_policy, aws_secretsmanager_secret.db_lifecycle.arn)
    error_message = "Application execution role must not read the lifecycle credential."
  }
  # The portal secret policy must not use a broad wildcard that would include
  # the lifecycle or master secrets.
  assert {
    condition     = !strcontains(local.application_execution_secrets_policy, "secret/${var.name}/database-application") && !strcontains(local.application_execution_secrets_policy, "secret:${var.name}/*")
    error_message = "Portal secret policy must use exact ARNs, not a project-wide wildcard."
  }
  assert {
    condition = alltrue([for role in [
      aws_iam_role.application_task,
      aws_iam_role.application_execution,
      aws_iam_role.db_admin_task,
      aws_iam_role.db_admin_execution
      ] :
      jsondecode(role.assume_role_policy).Statement[0].Condition.StringEquals["aws:SourceAccount"] == "123456789012" &&
      jsondecode(role.assume_role_policy).Statement[0].Condition.ArnLike["aws:SourceArn"] == "arn:aws:ecs:us-east-1:123456789012:*"
    ])
    error_message = "Every ECS role trust must scope the source account and ECS ARN."
  }
  assert {
    condition = alltrue([for role in [
      aws_iam_role.application_task,
      aws_iam_role.application_execution,
      aws_iam_role.db_admin_task,
      aws_iam_role.db_admin_execution,
      aws_iam_role.flow_logs
      ] : role.permissions_boundary == "arn:aws:iam::123456789012:policy/backstage-workload-boundary"
    ])
    error_message = "Every ECS and VPC flow log role must attach the exact project workload permissions boundary."
  }
}
