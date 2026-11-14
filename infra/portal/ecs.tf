locals {
  ecs_trust = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Principal = { Service = "ecs-tasks.amazonaws.com" }, Action = "sts:AssumeRole" }]
  })
}
resource "aws_iam_role" "execution" {
  name_prefix        = "${var.name}-exec-"
  assume_role_policy = local.ecs_trust
}
resource "aws_iam_role_policy_attachment" "execution" {
  role       = aws_iam_role.execution.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}
resource "aws_iam_role_policy" "secrets" {
  role = aws_iam_role.execution.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow", Action = ["secretsmanager:GetSecretValue"]
      Resource = [var.app_secret_arn, aws_secretsmanager_secret.database.arn]
    }]
  })
}
resource "aws_iam_role" "app" {
  name_prefix        = "${var.name}-app-"
  assume_role_policy = local.ecs_trust
}
resource "aws_iam_role_policy" "docs" {
  role = aws_iam_role.app.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      { Effect = "Allow", Action = ["s3:GetObject"], Resource = "${aws_s3_bucket.techdocs.arn}/*" },
      { Effect = "Allow", Action = ["s3:ListBucket"], Resource = aws_s3_bucket.techdocs.arn }
    ]
  })
}
resource "aws_ecs_cluster" "portal" {
  name = var.name
  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}
resource "aws_ecs_task_definition" "portal" {
  family                   = var.name
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "1024"
  memory                   = "2048"
  execution_role_arn       = aws_iam_role.execution.arn
  task_role_arn            = aws_iam_role.app.arn
  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }
  container_definitions = jsonencode([{
    name            = "backstage", image = var.image, essential = true
    user            = "node"
    portMappings    = [{ containerPort = 7007, protocol = "tcp" }]
    linuxParameters = { initProcessEnabled = true }
    environment = [
      { name = "BASE_URL", value = "https://${var.hostname}" },
      { name = "AWS_REGION", value = var.region },
      { name = "POSTGRES_HOST", value = aws_rds_cluster.db.endpoint },
      { name = "TECHDOCS_BUCKET", value = aws_s3_bucket.techdocs.id }
    ]
    secrets = [
      { name = "POSTGRES_USER", valueFrom = "${aws_secretsmanager_secret.database.arn}:username::" },
      { name = "POSTGRES_PASSWORD", valueFrom = "${aws_secretsmanager_secret.database.arn}:password::" },
      { name = "AUTH_GITHUB_CLIENT_ID", valueFrom = "${var.app_secret_arn}:github_client_id::" },
      { name = "AUTH_GITHUB_CLIENT_SECRET", valueFrom = "${var.app_secret_arn}:github_client_secret::" },
      { name = "GITHUB_TOKEN", valueFrom = "${var.app_secret_arn}:github_token::" }
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.portal.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "backstage"
      }
    }
    healthCheck = {
      command  = ["CMD-SHELL", "curl -fsS http://localhost:7007/.backstage/health/v1/readiness || exit 1"]
      interval = 30, timeout = 5, retries = 3, startPeriod = 120
    }
  }])
}
resource "aws_ecs_service" "portal" {
  name                              = var.name
  cluster                           = aws_ecs_cluster.portal.id
  task_definition                   = aws_ecs_task_definition.portal.arn
  desired_count                     = var.service_count
  launch_type                       = "FARGATE"
  platform_version                  = "1.4.0"
  health_check_grace_period_seconds = 180
  wait_for_steady_state             = true
  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }
  network_configuration {
    subnets          = module.vpc.private_subnets
    security_groups  = [aws_security_group.app.id]
    assign_public_ip = false
  }
  load_balancer {
    target_group_arn = aws_lb_target_group.portal.arn
    container_name   = "backstage"
    container_port   = 7007
  }
  # CI owns application revisions after the initial Terraform deployment.
  lifecycle { ignore_changes = [task_definition] }
  depends_on = [aws_lb_listener.https, aws_rds_cluster_instance.db, aws_iam_role_policy.secrets, aws_iam_role_policy_attachment.execution]
}
