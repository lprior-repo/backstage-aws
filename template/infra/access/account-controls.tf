resource "aws_iam_policy" "account_controls" {
  count       = var.enable_account_controls ? 1 : 0
  name        = "${var.name}-provisioner-account-controls"
  description = "Temporary account baseline permissions; remove by setting enable_account_controls false after bootstrap."
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Effect" = "Allow"
        "Action" = [
          "iam:UpdateAccountPasswordPolicy",
          "s3:PutAccountPublicAccessBlock",
        ]
        "Resource" = "*"
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "s3:CreateBucket",
          "s3:PutBucketTagging",
          "s3:PutBucketPublicAccessBlock",
          "s3:PutBucketVersioning",
          "s3:PutEncryptionConfiguration",
          "s3:PutBucketPolicy",
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
        "Sid"    = "CreateProjectTrail"
        "Effect" = "Allow"
        "Action" = [
          "cloudtrail:CreateTrail",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudtrail:${var.aws_region}:${var.aws_account_id}:trail/${var.name}-account",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "ManageProjectTrail"
        "Effect" = "Allow"
        "Action" = [
          "cloudtrail:UpdateTrail",
          "cloudtrail:PutEventSelectors",
          "cloudtrail:StartLogging",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudtrail:${var.aws_region}:${var.aws_account_id}:trail/${var.name}-account",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "TagProjectTrail"
        "Effect" = "Allow"
        "Action" = [
          "cloudtrail:AddTags",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudtrail:${var.aws_region}:${var.aws_account_id}:trail/${var.name}-account",
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
        "Sid"    = "UntagProjectTrailWithoutChangingOwner"
        "Effect" = "Allow"
        "Action" = [
          "cloudtrail:RemoveTags",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:cloudtrail:${var.aws_region}:${var.aws_account_id}:trail/${var.name}-account",
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
      {
        "Sid"    = "CreateProjectAnalyzer"
        "Effect" = "Allow"
        "Action" = [
          "access-analyzer:CreateAnalyzer",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:access-analyzer:${var.aws_region}:${var.aws_account_id}:analyzer/${var.name}-account",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "ManageProjectAnalyzer"
        "Effect" = "Allow"
        "Action" = [
          "access-analyzer:DeleteAnalyzer",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:access-analyzer:${var.aws_region}:${var.aws_account_id}:analyzer/${var.name}-account",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "TagProjectAnalyzer"
        "Effect" = "Allow"
        "Action" = [
          "access-analyzer:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:access-analyzer:${var.aws_region}:${var.aws_account_id}:analyzer/${var.name}-account",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "UntagProjectAnalyzer"
        "Effect" = "Allow"
        "Action" = [
          "access-analyzer:UntagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:access-analyzer:${var.aws_region}:${var.aws_account_id}:analyzer/${var.name}-account",
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
