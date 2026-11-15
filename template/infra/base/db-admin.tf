locals {
  workload_permissions_boundary_arn = "arn:${local.partition}:iam::${local.account_id}:policy/${var.name}-workload-boundary"
}

resource "aws_iam_role" "application_task" {
  name                 = "${var.name}-portal-task"
  assume_role_policy   = local.task_trust_policy
  permissions_boundary = local.workload_permissions_boundary_arn
}

resource "aws_iam_role" "application_execution" {
  name                 = "${var.name}-portal-execution"
  assume_role_policy   = local.task_trust_policy
  permissions_boundary = local.workload_permissions_boundary_arn
}

resource "aws_iam_role" "db_admin_task" {
  name                 = "${var.name}-db-admin-task"
  assume_role_policy   = local.task_trust_policy
  permissions_boundary = local.workload_permissions_boundary_arn
}

resource "aws_iam_role" "db_admin_execution" {
  name                 = "${var.name}-db-admin-execution"
  assume_role_policy   = local.task_trust_policy
  permissions_boundary = local.workload_permissions_boundary_arn
}



resource "aws_iam_role_policy" "application_execution_base" {
  role = aws_iam_role.application_execution.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["ecr:GetAuthorizationToken"]
      Resource = "*"
      }, {
      Effect = "Allow"
      Action = [
        "ecr:BatchCheckLayerAvailability",
        "ecr:GetDownloadUrlForLayer",
        "ecr:BatchGetImage"
      ]
      Resource = aws_ecr_repository.portal.arn
      }, {
      Effect   = "Allow"
      Action   = ["logs:CreateLogStream", "logs:PutLogEvents"]
      Resource = "arn:aws:logs:${var.aws_region}:${local.account_id}:log-group:/backstage/${var.name}/*:*"
    }]
  })
}

locals {
  application_execution_secrets_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = ["secretsmanager:GetSecretValue"]
      Resource = [
        aws_secretsmanager_secret.app_db.arn,
        aws_secretsmanager_secret.session.arn,
        "arn:${local.partition}:secretsmanager:${var.aws_region}:${local.account_id}:secret:${var.name}/*"
      ]
      }, {
      Effect   = "Allow"
      Action   = ["kms:Decrypt"]
      Resource = aws_kms_key.app.arn
      Condition = {
        StringEquals = {
          "kms:ViaService" = "secretsmanager.${var.aws_region}.amazonaws.com"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "application_secrets" {
  role   = aws_iam_role.application_execution.id
  policy = local.application_execution_secrets_policy
}


resource "aws_iam_role_policy" "db_admin_execution_access" {
  role = aws_iam_role.db_admin_execution.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = [aws_db_instance.database.master_user_secret[0].secret_arn, aws_secretsmanager_secret.app_db.arn]
      }, {
      Effect   = "Allow"
      Action   = ["kms:Decrypt"]
      Resource = aws_kms_key.app.arn
      Condition = {
        StringEquals = {
          "kms:ViaService" = "secretsmanager.${var.aws_region}.amazonaws.com"
        }
      }
      }, {
      Effect   = "Allow"
      Action   = ["logs:CreateLogStream", "logs:PutLogEvents"]
      Resource = "${aws_cloudwatch_log_group.db_admin.arn}:*"
    }]
  })
}

resource "aws_iam_role_policy" "db_admin_secret_access" {
  role = aws_iam_role.db_admin_task.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = [aws_db_instance.database.master_user_secret[0].secret_arn, aws_secretsmanager_secret.app_db.arn]
      }, {
      Effect   = "Allow"
      Action   = ["kms:Decrypt"]
      Resource = aws_kms_key.app.arn
      Condition = {
        StringEquals = {
          "kms:ViaService" = "secretsmanager.${var.aws_region}.amazonaws.com"
        }
      }
      }, {
      Effect   = "Allow"
      Action   = ["s3:GetObject", "s3:PutObject"]
      Resource = "${aws_s3_bucket.seed.arn}/*"
      }, {
      Effect   = "Allow"
      Action   = ["kms:Decrypt", "kms:GenerateDataKey"]
      Resource = aws_kms_key.app.arn
      Condition = {
        StringEquals = {
          "kms:ViaService" = "s3.${var.aws_region}.amazonaws.com"
        }
        StringLike = {
          "kms:EncryptionContext:aws:s3:arn" = "${aws_s3_bucket.seed.arn}/*"
        }
      }
    }]
  })
}

resource "aws_cloudwatch_log_group" "db_admin" {
  name              = "/backstage/${var.name}/db-admin"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.app.arn
}

resource "aws_cloudwatch_log_group" "portal" {
  name              = "/backstage/${var.name}/portal"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.app.arn
}

resource "aws_cloudwatch_log_group" "flow" {
  name              = "/backstage/${var.name}/vpc-flow"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.app.arn
}

resource "aws_iam_role" "flow_logs" {
  name                 = "${var.name}-flow-logs"
  permissions_boundary = local.workload_permissions_boundary_arn
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "vpc-flow-logs.amazonaws.com" }
      Action    = "sts:AssumeRole"
      Condition = { StringEquals = { "aws:SourceAccount" = local.account_id } }
    }]
  })
}

resource "aws_iam_role_policy" "flow_logs" {
  role = aws_iam_role.flow_logs.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["logs:CreateLogStream", "logs:PutLogEvents", "logs:DescribeLogStreams"]
      Resource = "${aws_cloudwatch_log_group.flow.arn}:*"
    }]
  })
}

resource "aws_flow_log" "main" {
  vpc_id               = aws_vpc.main.id
  traffic_type         = "ALL"
  iam_role_arn         = aws_iam_role.flow_logs.arn
  log_destination      = aws_cloudwatch_log_group.flow.arn
  log_destination_type = "cloud-watch-logs"
  depends_on           = [aws_iam_role_policy.flow_logs]
}

locals {
  task_trust_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ecs-tasks.amazonaws.com" }
      Action    = "sts:AssumeRole"
      Condition = {
        StringEquals = { "aws:SourceAccount" = local.account_id }
        ArnLike      = { "aws:SourceArn" = "arn:${local.partition}:ecs:${var.aws_region}:${local.account_id}:*" }
      }
    }]
  })
}

# `init` sets the role to the current application password. `rotate` sets it
# to the pending one, before the CLI makes that version current.
resource "aws_ecs_task_definition" "db_admin_init" {
  for_each                 = { init = "AWSCURRENT", rotate = "AWSPENDING" }
  family                   = "${var.name}-db-admin-${each.key}"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"
  execution_role_arn       = aws_iam_role.db_admin_execution.arn
  task_role_arn            = aws_iam_role.db_admin_task.arn
  container_definitions = jsonencode([{
    name       = "db-admin"
    image      = "public.ecr.aws/docker/library/postgres:16"
    essential  = true
    entryPoint = ["/bin/sh", "-ec"]
    command = [<<-EOT
      psql --set=ON_ERROR_STOP=1 --set=app_role="$APP_ROLE" --set=app_password="$APP_PASSWORD" <<'SQL'
      SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'app_role', :'app_password') WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = :'app_role') \gexec
      SELECT format('ALTER ROLE %I WITH LOGIN PASSWORD %L', :'app_role', :'app_password') \gexec
      SELECT format('GRANT %I TO CURRENT_USER', :'app_role') \gexec
      SELECT format('CREATE DATABASE %I OWNER %I', 'seed', :'app_role') WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'seed') \gexec
      SQL
    EOT
    ]
    environment = [
      { name = "APP_ROLE", value = var.database_application_username },
      { name = "PGHOST", value = aws_db_instance.database.address },
      { name = "PGPORT", value = tostring(aws_db_instance.database.port) },
      { name = "PGDATABASE", value = "postgres" },
      { name = "PGUSER", value = "backstage_admin" },
      { name = "PGSSLMODE", value = "require" }
    ]
    secrets = [
      { name = "PGPASSWORD", valueFrom = "${aws_db_instance.database.master_user_secret[0].secret_arn}:password::" },
      { name = "APP_PASSWORD", valueFrom = "${aws_secretsmanager_secret.app_db.arn}:password:${each.value}:" }
    ]
    logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = each.key } }
  }])
}

resource "aws_ecs_task_definition" "db_admin_clone" {
  family                   = "${var.name}-db-admin-clone"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"
  execution_role_arn       = aws_iam_role.db_admin_execution.arn
  task_role_arn            = aws_iam_role.db_admin_task.arn
  container_definitions = jsonencode([{
    name       = "db-admin"
    image      = "public.ecr.aws/docker/library/postgres:16"
    essential  = true
    entryPoint = ["/bin/sh", "-ec"]
    command = [<<-EOT
      if [ "$DB_ACTION" = drop ]; then
        psql --set=ON_ERROR_STOP=1 --set=db_name="$DB_NAME" <<'SQL'
      SELECT format('DROP DATABASE IF EXISTS %I WITH (FORCE)', :'db_name') \gexec
      SQL
      else
        psql --set=ON_ERROR_STOP=1 --set=db_name="$DB_NAME" --set=app_role="$APP_ROLE" <<'SQL'
      SELECT format('CREATE DATABASE %I TEMPLATE %I OWNER %I', :'db_name', 'seed', :'app_role') WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = :'db_name') \gexec
      SQL
      fi
    EOT
    ]
    environment = [
      { name = "DB_ACTION", value = "clone" },
      { name = "PGHOST", value = aws_db_instance.database.address },
      { name = "PGPORT", value = tostring(aws_db_instance.database.port) },
      { name = "PGDATABASE", value = "postgres" },
      { name = "PGUSER", value = "backstage_admin" },
      { name = "APP_ROLE", value = var.database_application_username },
      { name = "PGSSLMODE", value = "require" }
    ]
    secrets = [
      { name = "PGPASSWORD", valueFrom = "${aws_db_instance.database.master_user_secret[0].secret_arn}:password::" }
    ]
    logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "clone" } }
  }])
}

resource "aws_ecs_task_definition" "db_admin_seed_restore" {
  family                   = "${var.name}-db-admin-seed-restore"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "512"
  memory                   = "1024"
  execution_role_arn       = aws_iam_role.db_admin_execution.arn
  task_role_arn            = aws_iam_role.db_admin_task.arn
  volume { name = "dump" }
  container_definitions = jsonencode([
    {
      name        = "download"
      image       = "public.ecr.aws/aws-cli/aws-cli:2"
      essential   = false
      entryPoint  = ["/bin/sh"]
      command     = ["-c", "aws s3 cp \"s3://${aws_s3_bucket.seed.id}/$SEED_OBJECT_KEY\" /dump/seed.dump"]
      mountPoints = [{ sourceVolume = "dump", containerPath = "/dump", readOnly = false }]
      environment = [
        { name = "AWS_REGION", value = var.aws_region },
        { name = "SEED_OBJECT_KEY", value = "seed.dump" }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "download" } }
    },
    {
      name       = "restore"
      image      = "public.ecr.aws/docker/library/postgres:16"
      essential  = true
      dependsOn  = [{ containerName = "download", condition = "SUCCESS" }]
      entryPoint = ["/bin/sh", "-ec"]
      # One transaction: a failed restore leaves the previous seed intact.
      command     = ["pg_restore --single-transaction --clean --if-exists --no-owner --role=\"$APP_ROLE\" --host=\"$PGHOST\" --username=backstage_admin --dbname=seed /dump/seed.dump"]
      mountPoints = [{ sourceVolume = "dump", containerPath = "/dump", readOnly = true }]
      environment = [
        { name = "APP_ROLE", value = var.database_application_username },
        { name = "PGHOST", value = aws_db_instance.database.address },
        { name = "PGSSLMODE", value = "require" }
      ]
      secrets = [
        { name = "PGPASSWORD", valueFrom = "${aws_db_instance.database.master_user_secret[0].secret_arn}:password::" }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "restore" } }
    }
  ])
}
