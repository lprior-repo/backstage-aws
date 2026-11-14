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

resource "aws_iam_role" "portal_execution" {
  name               = "${var.name}-portal-execution"
  assume_role_policy = local.task_trust_policy
}

resource "aws_iam_role" "bootstrap_execution" {
  name               = "${var.name}-bootstrap-execution"
  assume_role_policy = local.task_trust_policy
}

resource "aws_iam_role" "portal_task" {
  name               = "${var.name}-portal-task"
  assume_role_policy = local.task_trust_policy
}

resource "aws_iam_role" "bootstrap_task" {
  name               = "${var.name}-bootstrap-task"
  assume_role_policy = local.task_trust_policy
}

resource "aws_iam_role_policy" "portal_execution" {
  role = aws_iam_role.portal_execution.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ECRLogin"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid      = "PullThisPortalImage"
        Effect   = "Allow"
        Action   = ["ecr:BatchCheckLayerAvailability", "ecr:GetDownloadUrlForLayer", "ecr:BatchGetImage"]
        Resource = aws_ecr_repository.portal.arn
      },
      {
        Sid      = "WritePortalLogs"
        Effect   = "Allow"
        Action   = ["logs:CreateLogStream", "logs:PutLogEvents"]
        Resource = "${aws_cloudwatch_log_group.portal.arn}:*"
      },
      {
        Sid      = "InjectApplicationSecrets"
        Effect   = "Allow"
        Action   = "secretsmanager:GetSecretValue"
        Resource = concat([aws_secretsmanager_secret.database_application.arn, aws_secretsmanager_secret.identity.arn], [for secret in values(aws_secretsmanager_secret.application) : secret.arn])
      },
      {
        Sid      = "DecryptOnlyApplicationSecrets"
        Effect   = "Allow"
        Action   = "kms:Decrypt"
        Resource = aws_kms_key.application.arn
        Condition = {
          StringEquals = {
            "kms:ViaService"                  = "secretsmanager.${var.aws_region}.amazonaws.com"
            "kms:EncryptionContext:SecretARN" = concat([aws_secretsmanager_secret.database_application.arn, aws_secretsmanager_secret.identity.arn], [for secret in values(aws_secretsmanager_secret.application) : secret.arn])
          }
        }
      }
    ]
  })
}

resource "aws_iam_role_policy" "bootstrap_execution" {
  role = aws_iam_role.bootstrap_execution.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ECRLogin"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid      = "PullThisPortalImage"
        Effect   = "Allow"
        Action   = ["ecr:BatchCheckLayerAvailability", "ecr:GetDownloadUrlForLayer", "ecr:BatchGetImage"]
        Resource = aws_ecr_repository.portal.arn
      },
      {
        Sid      = "WriteBootstrapLogs"
        Effect   = "Allow"
        Action   = ["logs:CreateLogStream", "logs:PutLogEvents"]
        Resource = "${aws_cloudwatch_log_group.bootstrap.arn}:*"
      }
    ]
  })
}

resource "aws_iam_role_policy" "portal_task" {
  role = aws_iam_role.portal_task.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "StartOnlyApprovedPortalRollout"
        Effect   = "Allow"
        Action   = "states:StartExecution"
        Resource = local.rollout_arn
      },
      {
        Sid      = "ReadOnlyApprovedRolloutExecutions"
        Effect   = "Allow"
        Action   = "states:DescribeExecution"
        Resource = "arn:${local.partition}:states:${var.aws_region}:${local.account_id}:execution:${local.rollout_name}:*"
      }
    ]
  })
}

resource "aws_iam_role_policy" "bootstrap_task" {
  role = aws_iam_role.bootstrap_task.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ReadOnlyDatabaseSecrets"
        Effect   = "Allow"
        Action   = "secretsmanager:GetSecretValue"
        Resource = [aws_db_instance.database.master_user_secret[0].secret_arn, aws_secretsmanager_secret.database_application.arn]
      },
      {
        Sid      = "WriteOnlyApplicationDatabaseSecret"
        Effect   = "Allow"
        Action   = "secretsmanager:PutSecretValue"
        Resource = aws_secretsmanager_secret.database_application.arn
      },
      {
        Sid      = "DecryptDatabaseSecrets"
        Effect   = "Allow"
        Action   = "kms:Decrypt"
        Resource = aws_kms_key.application.arn
        Condition = {
          StringEquals = {
            "kms:ViaService"                  = "secretsmanager.${var.aws_region}.amazonaws.com"
            "kms:EncryptionContext:SecretARN" = [aws_db_instance.database.master_user_secret[0].secret_arn, aws_secretsmanager_secret.database_application.arn]
          }
        }
      },
      {
        Sid      = "EncryptApplicationDatabaseSecret"
        Effect   = "Allow"
        Action   = "kms:GenerateDataKey"
        Resource = aws_kms_key.application.arn
        Condition = {
          StringEquals = {
            "kms:ViaService"                  = "secretsmanager.${var.aws_region}.amazonaws.com"
            "kms:EncryptionContext:SecretARN" = aws_secretsmanager_secret.database_application.arn
          }
        }
      }
    ]
  })
}

resource "aws_iam_role" "flow_logs" {
  name = "${var.name}-flow-logs"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "vpc-flow-logs.amazonaws.com" }
      Action    = "sts:AssumeRole"
      Condition = {
        StringEquals = { "aws:SourceAccount" = local.account_id }
        ArnLike      = { "aws:SourceArn" = "arn:${local.partition}:ec2:${var.aws_region}:${local.account_id}:vpc-flow-log/*" }
      }
    }]
  })
}

resource "aws_iam_role_policy" "flow_logs" {
  role = aws_iam_role.flow_logs.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents", "logs:DescribeLogStreams"]
        Resource = "${aws_cloudwatch_log_group.flow.arn}:*"
      },
      {
        Sid      = "DescribeLogGroupsRequiresWildcard"
        Effect   = "Allow"
        Action   = "logs:DescribeLogGroups"
        Resource = "*"
      }
    ]
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
