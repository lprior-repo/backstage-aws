resource "aws_iam_policy" "data_services" {
  name        = "${var.name}-provisioner-data-services"
  description = "Provision project application secrets and ECR repository"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "CreateProjectSecrets"
        "Effect" = "Allow"
        "Action" = [
          "secretsmanager:CreateSecret",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "ManageOwnedProjectSecrets"
        "Effect" = "Allow"
        "Action" = [
          "secretsmanager:DescribeSecret",
          "secretsmanager:ListSecretVersionIds",
          "secretsmanager:PutSecretValue",
          "secretsmanager:GetSecretValue",
          "secretsmanager:UpdateSecret",
          "secretsmanager:DeleteSecret",
          "secretsmanager:UpdateSecretVersionStage",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"     = var.aws_region
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "TagOwnedProjectSecrets"
        "Effect" = "Allow"
        "Action" = [
          "secretsmanager:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"     = var.aws_region
            "aws:ResourceTag/Project" = var.name
            "aws:RequestTag/Project"  = var.name
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "Project",
              "ManagedBy",
              "Environment",
            ]
          }
        }
      },
      {
        "Sid"    = "UntagOwnedProjectSecretsWithoutChangingOwner"
        "Effect" = "Allow"
        "Action" = [
          "secretsmanager:UntagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"     = var.aws_region
            "aws:ResourceTag/Project" = var.name
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "ManagedBy",
              "Environment",
            ]
          }
        }
      },
      {
        "Sid"    = "CreateAndTagRdsManagedSecret"
        "Effect" = "Allow"
        "Action" = [
          "secretsmanager:CreateSecret",
          "secretsmanager:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:rds!db-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "ManagedRdsPrimarySecretReadOnly"
        "Effect" = "Allow"
        "Action" = [
          "secretsmanager:DescribeSecret",
          "secretsmanager:GetSecretValue",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:rds!db-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"                                         = var.aws_region
            "secretsmanager:ResourceTag/aws:secretsmanager:owningService" = "rds"
            "secretsmanager:ResourceTag/aws:rds:primaryDBInstanceArn"     = "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres"
          }
        }
      },
      {
        "Sid"    = "EcrAuthorizationTokenGlobal"
        "Effect" = "Allow"
        "Action" = [
          "ecr:GetAuthorizationToken",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "CreateProjectRepository"
        "Effect" = "Allow"
        "Action" = [
          "ecr:CreateRepository",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "ManageOwnedProjectRepository"
        "Effect" = "Allow"
        "Action" = [
          "ecr:DescribeRepositories",
          "ecr:ListTagsForResource",
          "ecr:DeleteRepository",
          "ecr:PutImageTagMutability",
          "ecr:PutImageScanningConfiguration",
          "ecr:PutLifecyclePolicy",
          "ecr:GetLifecyclePolicy",
          "ecr:DeleteLifecyclePolicy",
          "ecr:BatchCheckLayerAvailability",
          "ecr:InitiateLayerUpload",
          "ecr:UploadLayerPart",
          "ecr:CompleteLayerUpload",
          "ecr:PutImage",
          "ecr:BatchGetImage",
          "ecr:GetDownloadUrlForLayer",
          "ecr:DescribeImages",
          "ecr:StartImageScan",
          "ecr:DescribeImageScanFindings",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"     = var.aws_region
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "TagProjectRepository"
        "Effect" = "Allow"
        "Action" = [
          "ecr:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "Project",
              "ManagedBy",
              "Environment",
            ]
          }
        }
      },
      {
        "Sid"    = "UntagProjectRepositoryWithoutChangingOwner"
        "Effect" = "Allow"
        "Action" = [
          "ecr:UntagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "ManagedBy",
              "Environment",
            ]
          }
        }
      },
    ]
  })
}
