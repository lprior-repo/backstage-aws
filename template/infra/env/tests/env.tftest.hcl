mock_provider "aws" {
  mock_resource "aws_lb_target_group" {
    defaults = { arn = "arn:aws:elasticloadbalancing:us-east-1:123456789012:targetgroup/bs-test/1234567890123456" }
  }
}

override_data {
  target = data.terraform_remote_state.base
  values = {
    outputs = {
      vpc_id                          = "vpc-12345678"
      listener_arn                    = "arn:aws:elasticloadbalancing:us-east-1:123456789012:listener/app/test/1234567890123456/1234567890123456"
      kms_key_arn                     = "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555"
      ecr_repository_url              = "123456789012.dkr.ecr.us-east-1.amazonaws.com/backstage/portal"
      database_application_secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:backstage/database-app"
      database_endpoint               = "db.example.us-east-1.rds.amazonaws.com"
      ecs_cluster_name                = "backstage"
      ecs_cluster_arn                 = "arn:aws:ecs:us-east-1:123456789012:cluster/backstage"
      session_secret_arn              = "arn:aws:secretsmanager:us-east-1:123456789012:secret:backstage/session"
      application_subnet_ids          = ["subnet-12345678"]
      application_execution_role_arn  = "arn:aws:iam::123456789012:role/backstage-portal-execution"
      application_task_role_arn       = "arn:aws:iam::123456789012:role/backstage-portal-task"
      portal_security_group_id        = "sg-12345678"
      identity_secret_arn             = "arn:aws:secretsmanager:us-east-1:123456789012:secret:backstage/identity"
      application_secret_arns         = { github-token = "arn:aws:secretsmanager:us-east-1:123456789012:secret:backstage/plugin/github-token" }
    }
  }
}

variables {
  aws_account_id    = "123456789012"
  aws_region        = "us-east-1"
  name              = "backstage"
  namespace         = "pr-38"
  hostname          = "pr-38.example.com"
  domain_name       = "example.com"
  image_digest      = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  database_name     = "pr_38"
  state_bucket_name = "state-bucket"
}

run "guest_mode_is_fast_and_digest_pinned" {
  command = plan
  assert {
    condition     = flatten([for condition in aws_lb_listener_rule.env.condition : [for host in condition.host_header : tolist(host.values)]]) == [var.hostname]
    error_message = "Listener host must exactly match hostname."
  }
  assert {
    condition     = endswith(jsondecode(aws_ecs_task_definition.env.container_definitions)[0].image, "@${var.image_digest}")
    error_message = "Task image must be pinned to the requested digest."
  }
  assert {
    condition     = jsondecode(aws_ecs_task_definition.env.container_definitions)[0].command == ["node", "packages/backend", "--config", "app-config.yaml", "--config", "app-config.production.yaml", "--config", "app-config.auth-guest.yaml"]
    error_message = "Guest task must select its auth configuration."
  }
  assert {
    condition = {
      for item in jsondecode(aws_ecs_task_definition.env.container_definitions)[0].environment : item.name => item.value
      if contains(["POSTGRES_DB", "APP_BASE_URL", "BACKEND_BASE_URL"], item.name)
    } == { POSTGRES_DB = "pr_38", APP_BASE_URL = "https://pr-38.example.com", BACKEND_BASE_URL = "https://pr-38.example.com" }
    error_message = "Database name and base URLs follow the namespace."
  }
  assert {
    condition     = contains([for secret in jsondecode(aws_ecs_task_definition.env.container_definitions)[0].secrets : secret.name], "AUTH_SESSION_SECRET")
    error_message = "The session secret must be injected in every auth mode."
  }
  assert {
    condition     = !contains([for item in jsondecode(aws_ecs_task_definition.env.container_definitions)[0].environment : item.name], "POSTGRES_PASSWORD")
    error_message = "Database password must not be exposed as a plain environment variable."
  }
  assert {
    condition     = aws_lb_target_group.env.health_check[0].interval == 5 && aws_lb_target_group.env.health_check[0].healthy_threshold == 2
    error_message = "Target group health check interval and healthy threshold must match the fast readiness contract."
  }
  assert {
    condition     = aws_lb_target_group.env.tags["Project"] == var.name
    error_message = "Target groups must carry the Project tag used by deploy-role authorization."
  }
  assert {
    condition     = contains([for secret in jsondecode(aws_ecs_task_definition.env.container_definitions)[0].secrets : secret.name], "POSTGRES_PASSWORD")
    error_message = "Database password is delivered through ECS secrets."
  }
  assert {
    condition     = aws_lb_listener_rule.env.priority == 39
    error_message = "A pull request preview takes its listener priority from the pull request number."
  }
}

run "github_mode_reads_the_shared_identity_and_plugin_secrets" {
  command = apply
  variables {
    auth_mode           = "github"
    application_secrets = { github-token = { environment_variable = "GITHUB_TOKEN", json_key = "token" } }
  }
  assert {
    condition = {
      for secret in jsondecode(aws_ecs_task_definition.env.container_definitions)[0].secrets : secret.name => secret.valueFrom
      if contains(["AUTH_GITHUB_CLIENT_ID", "AUTH_GITHUB_CLIENT_SECRET", "GITHUB_TOKEN"], secret.name)
      } == {
      AUTH_GITHUB_CLIENT_ID     = "arn:aws:secretsmanager:us-east-1:123456789012:secret:backstage/identity:clientId::"
      AUTH_GITHUB_CLIENT_SECRET = "arn:aws:secretsmanager:us-east-1:123456789012:secret:backstage/identity:clientSecret::"
      GITHUB_TOKEN              = "arn:aws:secretsmanager:us-east-1:123456789012:secret:backstage/plugin/github-token:token::"
    }
    error_message = "GitHub mode and plugins read the secrets the base stack owns."
  }
  assert {
    condition     = contains(jsondecode(aws_ecs_task_definition.env.container_definitions)[0].command, "app-config.auth-github.yaml")
    error_message = "GitHub task must select its auth configuration."
  }
}

run "other_namespaces_never_take_a_pull_request_priority" {
  command = plan
  variables {
    namespace     = "production"
    hostname      = "example.com"
    database_name = "production"
  }
  assert {
    condition     = aws_lb_listener_rule.env.priority > 40000 && aws_lb_listener_rule.env.priority <= 49000
    error_message = "Named namespaces hash into the range above every pull request priority."
  }
}
