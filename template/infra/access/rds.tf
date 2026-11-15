resource "aws_iam_policy" "rds" {
  name        = "${var.name}-provisioner-rds"
  description = "Provision the project RDS database, subnet group, and parameter group"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "CreateProjectDatabase"
        "Effect" = "Allow"
        "Action" = [
          "rds:CreateDBInstance",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres",
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:subgrp:${var.name}-database",
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:pg:${var.name}-postgres16-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "MutateOwnedProjectDatabase"
        "Effect" = "Allow"
        "Action" = [
          "rds:ModifyDBInstance",
          "rds:DeleteDBInstance",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"     = var.aws_region
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "TagOwnedProjectDatabase"
        "Effect" = "Allow"
        "Action" = [
          "rds:AddTagsToResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres",
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
        "Sid"    = "UntagOwnedProjectDatabaseWithoutChangingOwner"
        "Effect" = "Allow"
        "Action" = [
          "rds:RemoveTagsFromResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres",
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
        "Sid"    = "ReadProjectDatabase"
        "Effect" = "Allow"
        "Action" = [
          "rds:DescribeDBInstances",
          "rds:ListTagsForResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "CreateProjectRdsGroups"
        "Effect" = "Allow"
        "Action" = [
          "rds:CreateDBSubnetGroup",
          "rds:CreateDBParameterGroup",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:subgrp:${var.name}-database",
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:pg:${var.name}-postgres16-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "MutateOwnedRdsGroups"
        "Effect" = "Allow"
        "Action" = [
          "rds:ModifyDBSubnetGroup",
          "rds:DeleteDBSubnetGroup",
          "rds:ModifyDBParameterGroup",
          "rds:DeleteDBParameterGroup",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:subgrp:${var.name}-database",
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:pg:${var.name}-postgres16-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"     = var.aws_region
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "TagOwnedRdsGroups"
        "Effect" = "Allow"
        "Action" = [
          "rds:AddTagsToResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:subgrp:${var.name}-database",
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:pg:${var.name}-postgres16-*",
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
        "Sid"    = "UntagOwnedRdsGroupsWithoutChangingOwner"
        "Effect" = "Allow"
        "Action" = [
          "rds:RemoveTagsFromResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:subgrp:${var.name}-database",
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:pg:${var.name}-postgres16-*",
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
        "Sid"    = "ReadProjectRdsGroups"
        "Effect" = "Allow"
        "Action" = [
          "rds:DescribeDBSubnetGroups",
          "rds:DescribeDBParameterGroups",
          "rds:DescribeDBParameters",
          "rds:ListTagsForResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:subgrp:${var.name}-database",
          "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:pg:${var.name}-postgres16-*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
    ]
  })
}
