resource "aws_iam_policy" "storage" {
  name        = "${var.name}-provisioner-storage"
  description = "Project Terraform state, seed upload, and audit bucket access"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Effect" = "Allow"
        "Action" = [
          "s3:CreateBucket",
          "s3:GetBucketLocation",
          "s3:GetBucketTagging",
          "s3:PutBucketTagging",
          "s3:GetBucketVersioning",
          "s3:PutBucketVersioning",
          "s3:GetBucketPublicAccessBlock",
          "s3:PutBucketPublicAccessBlock",
          "s3:GetBucketOwnershipControls",
          "s3:PutBucketOwnershipControls",
          "s3:GetEncryptionConfiguration",
          "s3:PutEncryptionConfiguration",
          "s3:GetBucketPolicy",
          "s3:PutBucketPolicy",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "s3:GetObject",
          "s3:PutObject",
          "s3:DeleteObject",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate/base/terraform.tfstate",
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate/base/terraform.tfstate.tflock",
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate/bootstrap/terraform.tfstate",
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate/bootstrap/terraform.tfstate.tflock",
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate/env/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "s3:ListBucket",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "s3:CreateBucket",
          "s3:GetBucketLocation",
          "s3:GetBucketTagging",
          "s3:PutBucketTagging",
          "s3:GetBucketPublicAccessBlock",
          "s3:PutBucketPublicAccessBlock",
          "s3:GetEncryptionConfiguration",
          "s3:PutEncryptionConfiguration",
          "s3:GetBucketPolicy",
          "s3:PutBucketPolicy",
          "s3:GetBucketVersioning",
          "s3:PutBucketVersioning",
          "s3:GetLifecycleConfiguration",
          "s3:PutLifecycleConfiguration",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-seed",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "s3:GetObject",
          "s3:PutObject",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-seed/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "s3:GetBucketLocation",
          "s3:GetBucketAcl",
          "s3:GetBucketTagging",
          "s3:GetBucketPublicAccessBlock",
          "s3:GetBucketVersioning",
          "s3:GetEncryptionConfiguration",
          "s3:GetBucketPolicy",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-audit",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "logs:CreateLogGroup",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "logs:DeleteLogGroup",
          "logs:PutRetentionPolicy",
          "logs:DeleteRetentionPolicy",
          "logs:AssociateKmsKey",
          "logs:DisassociateKmsKey",
          "logs:ListTagsForResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "logs:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*",
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
              "Purpose",
            ]
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "logs:UntagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "ManagedBy",
              "Environment",
              "Purpose",
            ]
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "logs:FilterLogEvents",
          "logs:DescribeLogStreams",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "logs:GetLogEvents",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*:log-stream:*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
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
        "Effect" = "Allow"
        "Action" = [
          "cloudwatch:PutMetricAlarm",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudwatch:${var.aws_region}:${var.aws_account_id}:alarm:${var.name}-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "cloudwatch:DescribeAlarms",
          "cloudwatch:DeleteAlarms",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudwatch:${var.aws_region}:${var.aws_account_id}:alarm:${var.name}-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "cloudwatch:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudwatch:${var.aws_region}:${var.aws_account_id}:alarm:${var.name}-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "cloudwatch:UntagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudwatch:${var.aws_region}:${var.aws_account_id}:alarm:${var.name}-*",
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
