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

resource "aws_iam_role" "db_lifecycle_task" {
  name                 = "${var.name}-db-lifecycle-task"
  assume_role_policy   = local.task_trust_policy
  permissions_boundary = local.workload_permissions_boundary_arn
}

resource "aws_iam_role" "db_lifecycle_execution" {
  name                 = "${var.name}-db-lifecycle-execution"
  assume_role_policy   = local.task_trust_policy
  permissions_boundary = local.workload_permissions_boundary_arn
}



resource "aws_iam_role_policy" "application_execution_base" {
  role = aws_iam_role.application_execution.id
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/application-execution-base.json.tftpl", {
    portal_repository_arn = aws_ecr_repository.portal.arn
    log_group_arn         = "arn:aws:logs:${var.aws_region}:${local.account_id}:log-group:/backstage/${var.name}/*:*"
  })))
}

locals {
  application_execution_secrets_policy = jsonencode(jsondecode(templatefile("${path.module}/policies/application-execution-secrets.json.tftpl", {
    secret_arns = [
      aws_secretsmanager_secret.app_db.arn,
      aws_secretsmanager_secret.session.arn,
      aws_secretsmanager_secret.identity.arn,
      # Plugin secrets are shared across environments, so the portal must read
      # all plugin secrets declared in the account. The lifecycle secret is
      # excluded so an attacker-controlled portal revision cannot inject it.
      "arn:${local.partition}:secretsmanager:${var.aws_region}:${local.account_id}:secret:${var.name}/plugin/*"
    ]
    kms_key_arn     = aws_kms_key.app.arn
    kms_via_service = "secretsmanager.${var.aws_region}.amazonaws.com"
  })))
}

resource "aws_iam_role_policy" "application_secrets" {
  role   = aws_iam_role.application_execution.id
  policy = local.application_execution_secrets_policy
}


resource "aws_iam_role_policy" "db_admin_execution_access" {
  role = aws_iam_role.db_admin_execution.id
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/db-admin-execution-access.json.tftpl", {
    secret_arns = [
      aws_db_instance.database.master_user_secret[0].secret_arn,
      aws_secretsmanager_secret.app_db.arn,
      aws_secretsmanager_secret.db_lifecycle.arn
    ]
    kms_key_arn     = aws_kms_key.app.arn
    kms_via_service = "secretsmanager.${var.aws_region}.amazonaws.com"
    log_group_arn   = "${aws_cloudwatch_log_group.db_admin.arn}:*"
  })))
}

resource "aws_iam_role_policy" "db_lifecycle_execution_access" {
  role = aws_iam_role.db_lifecycle_execution.id
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/db-admin-execution-access.json.tftpl", {
    secret_arns = [
      aws_secretsmanager_secret.db_lifecycle.arn
    ]
    kms_key_arn     = aws_kms_key.app.arn
    kms_via_service = "secretsmanager.${var.aws_region}.amazonaws.com"
    log_group_arn   = "${aws_cloudwatch_log_group.db_admin.arn}:*"
  })))
}

resource "aws_iam_role_policy" "db_admin_secret_access" {
  role = aws_iam_role.db_admin_task.id
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/db-admin-secret-access.json.tftpl", {
    secret_arns = [
      aws_db_instance.database.master_user_secret[0].secret_arn,
      aws_secretsmanager_secret.app_db.arn,
      aws_secretsmanager_secret.db_lifecycle.arn
    ]
    kms_key_arn     = aws_kms_key.app.arn
    kms_via_service = "secretsmanager.${var.aws_region}.amazonaws.com"
    seed_object_arn = "${aws_s3_bucket.seed.arn}/*"
    s3_via_service  = "s3.${var.aws_region}.amazonaws.com"
  })))
}

# Lifecycle task role: can read the lifecycle secret (to set its password on
# init), write to the app_db secret (for rotation), but NOT read seed S3.
# This is intentionally different from the admin task role.
resource "aws_iam_role_policy" "db_lifecycle_task_access" {
  role = aws_iam_role.db_lifecycle_task.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = ["secretsmanager:GetSecretValue"]
        Resource = [aws_secretsmanager_secret.db_lifecycle.arn]
      },
      {
        Effect = "Allow"
        Action = ["secretsmanager:PutSecretValue"]
        Resource = [aws_secretsmanager_secret.db_lifecycle.arn]
      },
      {
        Effect = "Allow"
        Action = ["secretsmanager:GetSecretValue"]
        Resource = [aws_secretsmanager_secret.app_db.arn]
      },
      {
        Effect = "Allow"
        Action = ["kms:Decrypt"]
        Resource = aws_kms_key.app.arn
        Condition = {
          StringEquals = { "kms:ViaService" = "secretsmanager.${var.aws_region}.amazonaws.com" }
        }
      }
    ]
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
  assume_role_policy = jsonencode(jsondecode(templatefile("${path.module}/policies/flow-logs-trust.json.tftpl", {
    account_id = local.account_id
  })))
}

resource "aws_iam_role_policy" "flow_logs" {
  role = aws_iam_role.flow_logs.id
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/flow-logs-role.json.tftpl", {
    log_group_arn = "${aws_cloudwatch_log_group.flow.arn}:*"
    kms_key_arn   = aws_kms_key.app.arn
  })))
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
  task_trust_policy = jsonencode(jsondecode(templatefile("${path.module}/policies/task-trust.json.tftpl", {
    account_id     = local.account_id
    ecs_source_arn = "arn:${local.partition}:ecs:${var.aws_region}:${local.account_id}:*"
  })))
}

# `init` applies AWSCURRENT. Rotation fetches an explicit secret version by
# the persistent operation ID, never by the mutable AWSPENDING stage.
resource "aws_ecs_task_definition" "db_admin_init" {
  family                   = "${var.name}-db-admin-init"
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
    command    = [file("${path.module}/scripts/initialize-db.sh")]
    environment = [
      { name = "APP_ROLE", value = var.database_application_username },
      { name = "LIFECYCLE_ROLE", value = "backstage_lifecycle" },
      { name = "PGHOST", value = aws_db_instance.database.address },
      { name = "PGPORT", value = tostring(aws_db_instance.database.port) },
      { name = "PGDATABASE", value = "postgres" },
      { name = "PGUSER", value = "backstage_admin" },
      { name = "PGSSLMODE", value = "require" }
    ]
    secrets = [
      { name = "PGPASSWORD", valueFrom = "${aws_db_instance.database.master_user_secret[0].secret_arn}:password::" },
      { name = "APP_PASSWORD", valueFrom = "${aws_secretsmanager_secret.app_db.arn}:password:AWSCURRENT:" },
      { name = "LIFECYCLE_PASSWORD", valueFrom = "${aws_secretsmanager_secret.db_lifecycle.arn}:password:AWSCURRENT:" }
    ]
    logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "init" } }
  }])
}

resource "aws_ecs_task_definition" "db_admin_rotate" {
  family                   = "${var.name}-db-admin-rotate"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "512"
  memory                   = "1024"
  execution_role_arn       = aws_iam_role.db_admin_execution.arn
  task_role_arn            = aws_iam_role.db_admin_task.arn
  volume { name = "rotation" }
  container_definitions = jsonencode([
    {
      name       = "rotation-secret"
      image      = "public.ecr.aws/aws-cli/aws-cli:2"
      essential  = false
      entryPoint = ["/bin/sh", "-ec"]
      command = [<<-EOT
        umask 077
        aws s3 cp "s3://$SEED_BUCKET/$ROTATION_LOCK_KEY" /rotation/operation.json >/dev/null
        version_id=$(sed -n 's/.*"id":"\([0-9a-f-]*\)".*/\1/p' /rotation/operation.json)
        [ "$version_id" = "$ROTATION_ID" ]
        password=$(aws secretsmanager get-secret-value --secret-id "$SECRET_ARN" --version-id "$version_id" --query SecretString --output text | sed -n 's/.*"password":"\([A-Za-z0-9]\{40\}\)".*/\1/p')
        [ "$${#password}" -eq 40 ]
        case "$password" in *[!A-Za-z0-9]*) exit 1 ;; esac
        printf '%s' "$password" > /rotation/password
        unset password
      EOT
      ]
      mountPoints = [{ sourceVolume = "rotation", containerPath = "/rotation", readOnly = false }]
      environment = [
        { name = "AWS_REGION", value = var.aws_region },
        { name = "SEED_BUCKET", value = aws_s3_bucket.seed.id },
        { name = "ROTATION_LOCK_KEY", value = "operations/rotation/active.json" },
        { name = "SECRET_ARN", value = aws_secretsmanager_secret.app_db.arn }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "rotation-secret" } }
    },
    {
      name        = "db-admin"
      image       = "public.ecr.aws/docker/library/postgres:16"
      essential   = true
      dependsOn   = [{ containerName = "rotation-secret", condition = "SUCCESS" }]
      entryPoint  = ["/bin/sh", "-ec"]
      command     = [file("${path.module}/scripts/rotate-db.sh")]
      mountPoints = [{ sourceVolume = "rotation", containerPath = "/rotation", readOnly = true }]
      environment = [
        { name = "APP_ROLE", value = var.database_application_username },
        { name = "APP_PASSWORD_FILE", value = "/rotation/password" },
        { name = "PGHOST", value = aws_db_instance.database.address },
        { name = "PGPORT", value = tostring(aws_db_instance.database.port) },
        { name = "PGDATABASE", value = "postgres" },
        { name = "PGUSER", value = "backstage_admin" },
        { name = "PGSSLMODE", value = "require" }
      ]
      secrets = [
        { name = "PGPASSWORD", valueFrom = "${aws_db_instance.database.master_user_secret[0].secret_arn}:password::" }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "rotate" } }
    }
  ])
}

resource "aws_ecs_task_definition" "db_admin_clone" {
  family                   = "${var.name}-db-admin-clone"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"
  # Clone uses the dedicated lifecycle login (CREATEDB) instead of the
  # RDS master, so CI can never pass master credentials to an environment.
  execution_role_arn       = aws_iam_role.db_lifecycle_execution.arn
  task_role_arn            = aws_iam_role.db_lifecycle_task.arn
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
      { name = "PGUSER", value = "backstage_lifecycle" },
      { name = "APP_ROLE", value = var.database_application_username },
      { name = "PGSSLMODE", value = "require" }
    ]
    secrets = [
      { name = "PGPASSWORD", valueFrom = "${aws_secretsmanager_secret.db_lifecycle.arn}:password:AWSCURRENT:" }
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
      entryPoint  = ["/bin/sh", "-ec"]
      command     = ["aws s3 cp \"s3://$SEED_BUCKET/$SEED_OBJECT_KEY\" /dump/seed.dump"]
      mountPoints = [{ sourceVolume = "dump", containerPath = "/dump", readOnly = false }]
      environment = [
        { name = "AWS_REGION", value = var.aws_region },
        { name = "SEED_BUCKET", value = aws_s3_bucket.seed.id },
        { name = "SEED_OBJECT_KEY", value = "operations/seed/unset.dump" }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "download" } }
    },
    {
      name        = "restore"
      image       = "public.ecr.aws/docker/library/postgres:16"
      essential   = true
      dependsOn   = [{ containerName = "download", condition = "SUCCESS" }]
      entryPoint  = ["/bin/sh", "-ec"]
      command     = [file("${path.module}/scripts/restore-seed.sh")]
      mountPoints = [{ sourceVolume = "dump", containerPath = "/dump", readOnly = true }]
      environment = [
        { name = "APP_ROLE", value = var.database_application_username },
        { name = "SEED_OPERATION_ID", value = "00000000-0000-0000-0000-000000000000" },
        { name = "DUMP_PATH", value = "/dump/seed.dump" },
        { name = "PGHOST", value = aws_db_instance.database.address },
        { name = "PGPORT", value = tostring(aws_db_instance.database.port) },
        { name = "PGDATABASE", value = "postgres" },
        { name = "PGUSER", value = "backstage_admin" },
        { name = "PGSSLMODE", value = "require" }
      ]
      secrets = [
        { name = "PGPASSWORD", valueFrom = "${aws_db_instance.database.master_user_secret[0].secret_arn}:password::" }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "restore" } }
    }
  ])
}
