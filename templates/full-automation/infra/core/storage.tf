resource "aws_kms_key" "application" {
  description             = "${var.name}: secrets, database, ECR, and operational logs"
  enable_key_rotation     = true
  deletion_window_in_days = 30
  lifecycle {
    # Final snapshots and retained automated backups still need this key after
    # the DB is removed. Teardown must deliberately retain the key and its state.
    prevent_destroy = true
  }
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid       = "EnableAccountIAMPolicies"
        Effect    = "Allow"
        Principal = { AWS = "arn:${local.partition}:iam::${local.account_id}:root" }
        Action    = "kms:*"
        Resource  = "*"
      },
      {
        Sid       = "CloudWatchEncryptionForThisProject"
        Effect    = "Allow"
        Principal = { Service = "logs.${var.aws_region}.amazonaws.com" }
        Action    = ["kms:Encrypt", "kms:Decrypt", "kms:ReEncrypt*", "kms:GenerateDataKey*", "kms:DescribeKey"]
        Resource  = "*"
        Condition = { ArnLike = { "kms:EncryptionContext:aws:logs:arn" = [
          "arn:${local.partition}:logs:${var.aws_region}:${local.account_id}:log-group:/backstage/${var.name}/*",
          "arn:${local.partition}:logs:${var.aws_region}:${local.account_id}:log-group:/aws/vendedlogs/states/${var.name}-rollout",
          "arn:${local.partition}:logs:${var.aws_region}:${local.account_id}:log-group:/aws/rds/instance/${var.name}-postgres/*"
        ] } }
      }
    ]
  })
}

resource "aws_kms_alias" "application" {
  name          = "alias/${var.name}-application"
  target_key_id = aws_kms_key.application.key_id
}

resource "aws_ecr_repository" "portal" {
  name                 = "${var.name}/portal"
  image_tag_mutability = "IMMUTABLE"
  force_delete         = false
  encryption_configuration {
    encryption_type = "KMS"
    kms_key         = aws_kms_key.application.arn
  }
  image_scanning_configuration {
    scan_on_push = true
  }
}

resource "aws_ecr_lifecycle_policy" "portal" {
  repository = aws_ecr_repository.portal.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Expire untagged build layers after 14 days; retain every tagged release for explicit rollback"
      selection = {
        tagStatus   = "untagged"
        countType   = "sinceImagePushed"
        countUnit   = "days"
        countNumber = 14
      }
      action = { type = "expire" }
    }]
  })
}

# No aws_secretsmanager_secret_version resources: plaintext never enters state.
resource "aws_secretsmanager_secret" "database_application" {
  name                    = "${var.name}/${var.environment}/database-application"
  description             = "Created by the DB bootstrap task: JSON username/password for the application role"
  kms_key_id              = aws_kms_key.application.arn
  recovery_window_in_days = 30
}

resource "aws_secretsmanager_secret" "identity" {
  name                    = "${var.name}/${var.environment}/identity"
  description             = "Supply GitHub OAuth clientId/clientSecret and a strong sessionSecret outside Terraform"
  kms_key_id              = aws_kms_key.application.arn
  recovery_window_in_days = 30
}

resource "aws_cloudwatch_log_group" "portal" {
  name              = "/backstage/${var.name}/portal"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.application.arn
}

resource "aws_cloudwatch_log_group" "bootstrap" {
  name              = "/backstage/${var.name}/db-bootstrap"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.application.arn
}

resource "aws_cloudwatch_log_group" "workflow" {
  name              = "/aws/vendedlogs/states/${var.name}-rollout"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.application.arn
}

resource "aws_cloudwatch_log_group" "flow" {
  name              = "/backstage/${var.name}/vpc-flow"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.application.arn
}

resource "aws_cloudwatch_log_group" "database" {
  for_each          = toset(["postgresql", "upgrade"])
  name              = "/aws/rds/instance/${var.name}-postgres/${each.value}"
  retention_in_days = var.log_retention_days
  kms_key_id        = aws_kms_key.application.arn
}

resource "aws_db_subnet_group" "database" {
  name       = "${var.name}-database"
  subnet_ids = aws_subnet.database[*].id
}

resource "aws_db_parameter_group" "database" {
  name_prefix = "${var.name}-postgres16-"
  family      = "postgres16"
  parameter {
    name         = "rds.force_ssl"
    value        = "1"
    apply_method = "pending-reboot"
  }
  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_db_instance" "database" {
  identifier                      = "${var.name}-postgres"
  engine                          = "postgres"
  engine_version                  = var.postgres_engine_version
  db_name                         = var.database_name
  instance_class                  = var.database_instance_class
  allocated_storage               = 20
  max_allocated_storage           = 100
  storage_type                    = "gp3"
  storage_encrypted               = true
  kms_key_id                      = aws_kms_key.application.arn
  username                        = "backstage_admin"
  manage_master_user_password     = true
  master_user_secret_kms_key_id   = aws_kms_key.application.arn
  db_subnet_group_name            = aws_db_subnet_group.database.name
  parameter_group_name            = aws_db_parameter_group.database.name
  vpc_security_group_ids          = [aws_security_group.database.id]
  publicly_accessible             = false
  multi_az                        = var.database_multi_az
  backup_retention_period         = 14
  backup_window                   = "03:00-04:00"
  maintenance_window              = "sun:05:00-sun:06:00"
  copy_tags_to_snapshot           = true
  deletion_protection             = var.deletion_protection
  skip_final_snapshot             = false
  final_snapshot_identifier       = "${var.name}-postgres-final"
  delete_automated_backups        = false
  auto_minor_version_upgrade      = true
  allow_major_version_upgrade     = false
  apply_immediately               = false
  engine_lifecycle_support        = "open-source-rds-extended-support-disabled"
  enabled_cloudwatch_logs_exports = ["postgresql", "upgrade"]
  performance_insights_enabled    = false
  depends_on                      = [aws_cloudwatch_log_group.database]
}
