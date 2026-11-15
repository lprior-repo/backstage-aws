resource "aws_iam_policy" "workload_boundary" {
  name        = "${var.name}-workload-boundary"
  description = "Owner-controlled workload boundary for ${var.name}; never editable by the provisioner."
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "NamedWorkloadCapabilities"
        "Effect" = "Allow"
        "Action" = [
          "ecr:BatchCheckLayerAvailability",
          "ecr:BatchGetImage",
          "ecr:CompleteLayerUpload",
          "ecr:DescribeImages",
          "ecr:DescribeImageScanFindings",
          "ecr:DescribeRepositories",
          "ecr:GetDownloadUrlForLayer",
          "ecr:InitiateLayerUpload",
          "ecr:PutImage",
          "ecr:UploadLayerPart",
          "ecr:StartImageScan",
          "logs:CreateLogGroup",
          "logs:CreateLogStream",
          "logs:PutLogEvents",
          "logs:DescribeLogStreams",
          "logs:DeleteLogGroup",
          "logs:ListTagsForResource",
          "logs:PutRetentionPolicy",
          "logs:DeleteRetentionPolicy",
          "logs:AssociateKmsKey",
          "logs:DisassociateKmsKey",
          "logs:TagResource",
          "logs:UntagResource",
          "ecs:CreateService",
          "ecs:DeleteService",
          "ecs:UpdateService",
          "ecs:RegisterTaskDefinition",
          "ecs:TagResource",
          "ecs:DescribeTasks",
          "ecs:DescribeServices",
          "ecs:DescribeClusters",
          "s3:GetObject",
          "s3:PutObject",
          "s3:DeleteObject",
          "secretsmanager:GetSecretValue",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal",
          "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*",
          "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:service/${var.name}-cluster/${var.name}-*",
          "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task-definition/${var.name}-*:*",
          "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task/${var.name}-cluster/*",
          "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster",
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-seed/*",
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate/*",
          "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:${var.name}/*",
        ]
      },
      {
        "Sid"    = "RegionalApiExceptions"
        "Effect" = "Allow"
        "Action" = [
          "ecr:GetAuthorizationToken",
          "ecs:DescribeTaskDefinition",
          "ecs:DeregisterTaskDefinition",
          "elasticloadbalancing:Describe*",
          "logs:DescribeLogGroups",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "ClusterTaskListing"
        "Effect" = "Allow"
        "Action" = [
          "ecs:ListTasks",
          "ecs:ListServices",
        ]
        "Resource" = "*"
        "Condition" = {
          "ArnEquals" = {
            "ecs:cluster" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster"
          }
        }
      },
      {
        "Sid"      = "CloneOnlyInProjectCluster"
        "Effect"   = "Allow"
        "Action"   = "ecs:RunTask"
        "Resource" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task-definition/${var.name}-db-admin-clone:*"
        "Condition" = {
          "ArnEquals" = {
            "ecs:cluster" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster"
          }
        }
      },
      {
        "Sid"    = "PassTaskRoles"
        "Effect" = "Allow"
        "Action" = "iam:PassRole"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-execution",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-execution",
        ]
        "Condition" = {
          "StringEquals" = {
            "iam:PassedToService" = "ecs-tasks.amazonaws.com"
          }
        }
      },
      {
        "Sid"      = "ReadProjectRdsMaster"
        "Effect"   = "Allow"
        "Action"   = "secretsmanager:GetSecretValue"
        "Resource" = "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:rds!db-*"
        "Condition" = {
          "StringEquals" = {
            "secretsmanager:ResourceTag/aws:secretsmanager:owningService" = "rds"
            "secretsmanager:ResourceTag/aws:rds:primaryDBInstanceArn"     = "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres"
          }
        }
      },
      {
        "Sid"    = "ProjectBucketMetadata"
        "Effect" = "Allow"
        "Action" = [
          "s3:ListBucket",
          "s3:GetBucketLocation",
          "s3:GetBucketVersioning",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate",
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-seed",
        ]
      },
      {
        "Sid"      = "DecryptProjectSecretKey"
        "Effect"   = "Allow"
        "Action"   = "kms:Decrypt"
        "Resource" = "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*"
        "Condition" = {
          "StringEquals" = {
            "kms:ViaService"          = "secretsmanager.${var.aws_region}.${data.aws_partition.current.dns_suffix}"
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "ProjectS3Encryption"
        "Effect" = "Allow"
        "Action" = [
          "kms:Decrypt",
          "kms:Encrypt",
          "kms:GenerateDataKey",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*"
        "Condition" = {
          "StringEquals" = {
            "kms:ViaService"          = "s3.${var.aws_region}.${data.aws_partition.current.dns_suffix}"
            "aws:ResourceTag/Project" = var.name
          }
          "ArnLike" = {
            "kms:EncryptionContext:aws:s3:arn" = [
              "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate",
              "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-seed/*",
            ]
          }
        }
      },
      {
        "Sid"    = "ProjectListenerRules"
        "Effect" = "Allow"
        "Action" = [
          "elasticloadbalancing:CreateRule",
          "elasticloadbalancing:ModifyRule",
          "elasticloadbalancing:DeleteRule",
          "elasticloadbalancing:AddTags",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener-rule/app/${var.name}-alb/*",
        ]
      },
      {
        "Sid"      = "CreateTaggedTargets"
        "Effect"   = "Allow"
        "Action"   = "elasticloadbalancing:CreateTargetGroup"
        "Resource" = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/bs-*/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"      = "TagNewTargets"
        "Effect"   = "Allow"
        "Action"   = "elasticloadbalancing:AddTags"
        "Resource" = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/bs-*/*"
        "Condition" = {
          "StringEquals" = {
            "elasticloadbalancing:CreateAction" = "CreateTargetGroup"
            "aws:RequestTag/Project"            = var.name
          }
        }
      },
      {
        "Sid"    = "MaintainOwnedTargets"
        "Effect" = "Allow"
        "Action" = [
          "elasticloadbalancing:AddTags",
          "elasticloadbalancing:RemoveTags",
          "elasticloadbalancing:ModifyTargetGroup",
          "elasticloadbalancing:ModifyTargetGroupAttributes",
          "elasticloadbalancing:DeleteTargetGroup",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/bs-*/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
    ]
  })
}
