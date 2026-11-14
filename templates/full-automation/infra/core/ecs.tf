resource "aws_ecs_cluster" "portal" {
  name = "${var.name}-cluster"
  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}

resource "aws_ecs_task_definition" "portal" {
  family                   = "${var.name}-portal"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = tostring(var.portal_cpu)
  memory                   = "4096"
  execution_role_arn       = aws_iam_role.portal_execution.arn
  task_role_arn            = aws_iam_role.portal_task.arn
  # Retain prior approved portal revisions for rollback. Operators retire old
  # revisions explicitly after they are no longer needed by any service.
  skip_destroy = true
  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }
  container_definitions = jsonencode([{
    name      = "backstage"
    image     = local.portal_image
    essential = true
    user      = "1000:1000"
    # Backstage can write frontend configuration and temporary scaffolder data.
    # Persistent state is external; verify a writable-/tmp layout before enabling read-only root.
    readonlyRootFilesystem = false
    stopTimeout            = 120
    linuxParameters        = { initProcessEnabled = true }
    portMappings           = [{ containerPort = 7007, protocol = "tcp", name = "http" }]
    environment = [
      { name = "NODE_ENV", value = "production" },
      { name = "APP_BASE_URL", value = local.portal_base_url },
      { name = "BACKEND_BASE_URL", value = local.portal_base_url },
      { name = "AWS_REGION", value = var.aws_region },
      { name = "AWS_ROLLOUT_STATE_MACHINE_ARN", value = local.rollout_arn },
      { name = "POSTGRES_HOST", value = aws_db_instance.database.address },
      { name = "POSTGRES_PORT", value = tostring(aws_db_instance.database.port) },
      { name = "POSTGRES_DB", value = var.database_name },
      { name = "PGSSLROOTCERT", value = "/app/certs/aws-rds-global-bundle.pem" }
    ]
    secrets = concat([
      { name = "POSTGRES_USER", valueFrom = "${aws_secretsmanager_secret.database_application.arn}:username::" },
      { name = "POSTGRES_PASSWORD", valueFrom = "${aws_secretsmanager_secret.database_application.arn}:password::" },
      { name = "AUTH_GITHUB_CLIENT_ID", valueFrom = "${aws_secretsmanager_secret.identity.arn}:clientId::" },
      { name = "AUTH_GITHUB_CLIENT_SECRET", valueFrom = "${aws_secretsmanager_secret.identity.arn}:clientSecret::" },
      { name = "AUTH_SESSION_SECRET", valueFrom = "${aws_secretsmanager_secret.identity.arn}:sessionSecret::" }
      ], [for key, secret in var.application_secrets : {
        name      = secret.environment_variable
        valueFrom = secret.json_key == null ? aws_secretsmanager_secret.application[key].arn : "${aws_secretsmanager_secret.application[key].arn}:${secret.json_key}::"
    }])
    healthCheck = {
      command     = ["CMD", "node", "-e", "fetch('http://127.0.0.1:7007/.backstage/health/v1/readiness',{signal:AbortSignal.timeout(2000)}).then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"]
      interval    = 30
      timeout     = 5
      retries     = 3
      startPeriod = 60
    }
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.portal.name
        awslogs-region        = var.aws_region
        awslogs-stream-prefix = "backstage"
        mode                  = "non-blocking"
        max-buffer-size       = "10m"
      }
    }
  }])
}

resource "aws_ecs_task_definition" "bootstrap" {
  family                   = "${var.name}-db-bootstrap"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"
  execution_role_arn       = aws_iam_role.bootstrap_execution.arn
  task_role_arn            = aws_iam_role.bootstrap_task.arn
  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }
  container_definitions = jsonencode([{
    name                   = "db-bootstrap"
    image                  = local.portal_image
    essential              = true
    user                   = "1000:1000"
    readonlyRootFilesystem = true
    command                = ["node", "scripts/bootstrap-db.mjs"]
    stopTimeout            = 30
    linuxParameters        = { initProcessEnabled = true }
    environment = [
      { name = "NODE_ENV", value = "production" },
      { name = "AWS_REGION", value = var.aws_region },
      { name = "DB_ADMIN_SECRET_ARN", value = aws_db_instance.database.master_user_secret[0].secret_arn },
      { name = "DB_APP_SECRET_ARN", value = aws_secretsmanager_secret.database_application.arn },
      { name = "DB_APP_USER", value = var.database_application_username },
      { name = "POSTGRES_HOST", value = aws_db_instance.database.address },
      { name = "POSTGRES_PORT", value = tostring(aws_db_instance.database.port) },
      { name = "POSTGRES_DB", value = var.database_name },
      { name = "PGSSLROOTCERT", value = "/app/certs/aws-rds-global-bundle.pem" }
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.bootstrap.name
        awslogs-region        = var.aws_region
        awslogs-stream-prefix = "db-bootstrap"
        mode                  = "non-blocking"
        max-buffer-size       = "4m"
      }
    }
  }])
}

resource "aws_ecs_service" "portal" {
  name                               = "${var.name}-portal"
  cluster                            = aws_ecs_cluster.portal.id
  task_definition                    = aws_ecs_task_definition.portal.arn
  desired_count                      = 0
  launch_type                        = "FARGATE"
  platform_version                   = "LATEST"
  enable_execute_command             = false
  health_check_grace_period_seconds  = 180
  deployment_minimum_healthy_percent = 100
  deployment_maximum_percent         = 200
  wait_for_steady_state              = true
  propagate_tags                     = "SERVICE"
  enable_ecs_managed_tags            = true
  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }
  network_configuration {
    subnets          = aws_subnet.application[*].id
    security_groups  = [aws_security_group.portal.id]
    assign_public_ip = false
  }
  load_balancer {
    target_group_arn = aws_lb_target_group.portal.arn
    container_name   = "backstage"
    container_port   = 7007
  }
  lifecycle {
    # Terraform registers approved revisions. The rollout workflow activates
    # them only after the database bootstrap succeeds, including future upgrades.
    ignore_changes = [desired_count, task_definition]
  }
  depends_on = [
    aws_lb_listener.https,
    aws_iam_role_policy.portal_execution,
    aws_iam_role_policy.portal_task,
    aws_route.application_egress
  ]
}

# A separate fixed task revision makes password rotation an explicit maintenance
# operation. Step Functions never accepts a command or secret value in its input.
resource "aws_ecs_task_definition" "rotate" {
  family                   = "${var.name}-db-rotate"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"
  execution_role_arn       = aws_iam_role.bootstrap_execution.arn
  task_role_arn            = aws_iam_role.bootstrap_task.arn
  skip_destroy             = true
  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }
  container_definitions = jsonencode([merge(jsondecode(aws_ecs_task_definition.bootstrap.container_definitions)[0], {
    environment = concat(jsondecode(aws_ecs_task_definition.bootstrap.container_definitions)[0].environment, [{ name = "DB_ROTATE_PASSWORD", value = "true" }])
  })])
}
