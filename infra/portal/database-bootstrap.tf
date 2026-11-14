resource "aws_secretsmanager_secret" "database" {
  name_prefix             = "${var.name}-database-"
  recovery_window_in_days = 30
  lifecycle { prevent_destroy = true }
}
resource "aws_iam_role" "bootstrap_execution" {
  name_prefix        = "${var.name}-dbexec-"
  assume_role_policy = local.ecs_trust
}
resource "aws_iam_role_policy_attachment" "bootstrap_execution" {
  role       = aws_iam_role.bootstrap_execution.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}
resource "aws_iam_role_policy" "bootstrap_read" {
  role = aws_iam_role.bootstrap_execution.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow", Action = ["secretsmanager:GetSecretValue"]
      Resource = aws_rds_cluster.db.master_user_secret[0].secret_arn
    }]
  })
}
resource "aws_iam_role" "bootstrap" {
  name_prefix        = "${var.name}-dbinit-"
  assume_role_policy = local.ecs_trust
}
resource "aws_iam_role_policy" "bootstrap_write" {
  role = aws_iam_role.bootstrap.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow", Action = ["secretsmanager:PutSecretValue"]
      Resource = aws_secretsmanager_secret.database.arn
    }]
  })
}
resource "aws_ecs_task_definition" "bootstrap" {
  family                   = "${var.name}-database-bootstrap"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"
  execution_role_arn       = aws_iam_role.bootstrap_execution.arn
  task_role_arn            = aws_iam_role.bootstrap.arn
  container_definitions = jsonencode([{
    name    = "bootstrap", image = var.image, essential = true, user = "node"
    command = ["node", "packages/backend/scripts/bootstrap-database.cjs"]
    environment = [
      { name = "POSTGRES_HOST", value = aws_rds_cluster.db.endpoint },
      { name = "POSTGRES_USER", value = aws_rds_cluster.db.master_username },
      { name = "AWS_REGION", value = var.region },
      { name = "APP_DATABASE_SECRET_ARN", value = aws_secretsmanager_secret.database.arn }
    ]
    secrets = [{ name = "POSTGRES_PASSWORD", valueFrom = "${aws_rds_cluster.db.master_user_secret[0].secret_arn}:password::" }]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.portal.name
        awslogs-region        = var.region
        awslogs-stream-prefix = "database-bootstrap"
      }
    }
  }])
}
output "bootstrap_task_definition" { value = aws_ecs_task_definition.bootstrap.arn }
output "bootstrap_network" {
  value = { awsvpcConfiguration = {
    subnets        = module.vpc.private_subnets
    securityGroups = [aws_security_group.app.id]
    assignPublicIp = "DISABLED"
  } }
}
