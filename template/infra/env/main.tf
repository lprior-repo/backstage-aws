locals {
  base = data.terraform_remote_state.base.outputs
  # Rule priorities must be unique on the listener. Pull request previews take
  # theirs from the pull request number; any other namespace hashes into a
  # separate range.
  listener_priority = (
    can(regex("^pr-[0-9]{1,5}$", var.namespace))
    ? 1 + tonumber(substr(var.namespace, 3, -1)) % 40000
    : 40001 + parseint(substr(sha256(var.namespace), 0, 8), 16) % 9000
  )
  portal_secrets = concat(
    [
      { name = "POSTGRES_USER", valueFrom = "${local.base.database_application_secret_arn}:username::" },
      { name = "POSTGRES_PASSWORD", valueFrom = "${local.base.database_application_secret_arn}:password::" },
      { name = "AUTH_SESSION_SECRET", valueFrom = local.base.session_secret_arn }
    ],
    var.auth_mode == "github" ? [
      { name = "AUTH_GITHUB_CLIENT_ID", valueFrom = "${local.base.identity_secret_arn}:clientId::" },
      { name = "AUTH_GITHUB_CLIENT_SECRET", valueFrom = "${local.base.identity_secret_arn}:clientSecret::" }
    ] : [],
    [for key, secret in var.application_secrets : {
      name      = secret.environment_variable
      valueFrom = secret.json_key == null ? local.base.application_secret_arns[key] : "${local.base.application_secret_arns[key]}:${secret.json_key}::"
    }]
  )
}

resource "aws_lb_target_group" "env" {
  name_prefix          = "bs-"
  tags                 = { Project = var.name }
  vpc_id               = local.base.vpc_id
  port                 = 7007
  protocol             = "HTTP"
  target_type          = "ip"
  deregistration_delay = 5
  health_check {
    enabled             = true
    path                = "/.backstage/health/v1/readiness"
    matcher             = "200"
    interval            = 5
    timeout             = 2
    healthy_threshold   = 2
    unhealthy_threshold = 3
  }
}

resource "aws_lb_listener_rule" "env" {
  listener_arn = local.base.listener_arn
  priority     = local.listener_priority
  condition {
    host_header { values = [var.hostname] }
  }
  action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.env.arn
  }
}

resource "aws_cloudwatch_log_group" "env" {
  name              = "/backstage/${var.name}/${var.namespace}"
  retention_in_days = 30
  kms_key_id        = local.base.kms_key_arn
}

resource "aws_ecs_task_definition" "env" {
  family                   = "${var.name}-${var.namespace}"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "1024"
  memory                   = "2048"
  execution_role_arn       = local.base.application_execution_role_arn
  task_role_arn            = local.base.application_task_role_arn
  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }
  container_definitions = jsonencode([{
    name         = "backstage"
    image        = "${local.base.ecr_repository_url}@${var.image_digest}"
    essential    = true
    user         = "1000:1000"
    portMappings = [{ containerPort = 7007, protocol = "tcp", name = "http" }]
    command      = ["node", "packages/backend", "--config", "app-config.yaml", "--config", "app-config.production.yaml", "--config", "app-config.auth-${var.auth_mode}.yaml"]
    environment = [
      { name = "NODE_ENV", value = "production" },
      { name = "APP_BASE_URL", value = "https://${var.hostname}" },
      { name = "BACKEND_BASE_URL", value = "https://${var.hostname}" },
      { name = "AWS_REGION", value = var.aws_region },
      { name = "POSTGRES_HOST", value = local.base.database_endpoint },
      { name = "POSTGRES_PORT", value = "5432" },
      { name = "POSTGRES_DB", value = var.database_name },
      { name = "PGSSLROOTCERT", value = "/app/certs/aws-rds-global-bundle.pem" }
    ]
    secrets = local.portal_secrets
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.env.name
        awslogs-region        = var.aws_region
        awslogs-stream-prefix = "portal"
      }
    }
  }])
}

resource "aws_ecs_service" "env" {
  name            = "${var.name}-${var.namespace}"
  cluster         = local.base.ecs_cluster_arn
  task_definition = aws_ecs_task_definition.env.arn
  desired_count   = var.desired_count
  capacity_provider_strategy {
    capacity_provider = var.spot ? "FARGATE_SPOT" : "FARGATE"
    weight            = 1
  }
  platform_version                   = "LATEST"
  enable_execute_command             = false
  health_check_grace_period_seconds  = 60
  deployment_minimum_healthy_percent = 100
  deployment_maximum_percent         = 200
  wait_for_steady_state              = false
  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }
  network_configuration {
    subnets          = local.base.application_subnet_ids
    security_groups  = [local.base.portal_security_group_id]
    assign_public_ip = local.base.assign_public_ip
  }
  load_balancer {
    target_group_arn = aws_lb_target_group.env.arn
    container_name   = "backstage"
    container_port   = 7007
  }
}
