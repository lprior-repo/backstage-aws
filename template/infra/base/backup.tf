# The seed bucket also holds on-demand database dumps, under `backups/`.
# Versioning keeps a replaced or deleted object recoverable for 30 days.
resource "aws_s3_bucket_versioning" "seed" {
  bucket = aws_s3_bucket.seed.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_lifecycle_configuration" "seed" {
  bucket     = aws_s3_bucket.seed.id
  depends_on = [aws_s3_bucket_versioning.seed]
  rule {
    id     = "expire-replaced-objects"
    status = "Enabled"
    filter {}
    noncurrent_version_expiration {
      noncurrent_days = 30
    }
    abort_incomplete_multipart_upload {
      days_after_initiation = 7
    }
  }
}

# `backstage-aws db dump` runs this task: `pg_dump` writes one database to a
# scratch volume and the second container uploads the file. The database is
# reachable only from inside the VPC, so this is also how a production copy
# leaves the account to become another account's seed.
resource "aws_ecs_task_definition" "db_admin_dump" {
  family                   = "${var.name}-db-admin-dump"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "512"
  memory                   = "1024"
  execution_role_arn       = aws_iam_role.db_admin_execution.arn
  task_role_arn            = aws_iam_role.db_admin_task.arn
  volume { name = "dump" }
  container_definitions = jsonencode([
    {
      name        = "dump"
      image       = "public.ecr.aws/docker/library/postgres:16"
      essential   = false
      entryPoint  = ["/bin/sh", "-ec"]
      command     = ["pg_dump --format=custom --no-owner --no-acl --role=\"$APP_ROLE\" --dbname=\"$DB_NAME\" --file=/dump/database.dump"]
      mountPoints = [{ sourceVolume = "dump", containerPath = "/dump", readOnly = false }]
      environment = [
        { name = "DB_NAME", value = "seed" },
        { name = "APP_ROLE", value = var.database_application_username },
        { name = "PGHOST", value = aws_db_instance.database.address },
        { name = "PGPORT", value = tostring(aws_db_instance.database.port) },
        { name = "PGUSER", value = "backstage_admin" },
        { name = "PGSSLMODE", value = "require" }
      ]
      secrets = [
        { name = "PGPASSWORD", valueFrom = "${aws_db_instance.database.master_user_secret[0].secret_arn}:password::" }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "dump" } }
    },
    {
      name        = "upload"
      image       = "public.ecr.aws/aws-cli/aws-cli:2"
      essential   = true
      dependsOn   = [{ containerName = "dump", condition = "SUCCESS" }]
      entryPoint  = ["/bin/sh"]
      command     = ["-c", "aws s3 cp /dump/database.dump \"s3://${aws_s3_bucket.seed.id}/$BACKUP_KEY\""]
      mountPoints = [{ sourceVolume = "dump", containerPath = "/dump", readOnly = true }]
      environment = [
        { name = "AWS_REGION", value = var.aws_region },
        { name = "BACKUP_KEY", value = "backups/manual.dump" }
      ]
      logConfiguration = { logDriver = "awslogs", options = { awslogs-group = aws_cloudwatch_log_group.db_admin.name, awslogs-region = var.aws_region, awslogs-stream-prefix = "upload" } }
    }
  ])
}
