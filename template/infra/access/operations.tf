resource "aws_iam_policy" "operations" {
  name        = "${var.name}-provisioner-operations"
  description = "Provision project notifications, trail, analyzer, and budget"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "CreateProjectOperationsTopic"
        "Effect" = "Allow"
        "Action" = [
          "sns:CreateTopic",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:sns:${var.aws_region}:${var.aws_account_id}:${var.name}-operations",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "ManageProjectOperationsTopic"
        "Effect" = "Allow"
        "Action" = [
          "sns:GetTopicAttributes",
          "sns:SetTopicAttributes",
          "sns:DeleteTopic",
          "sns:Subscribe",
          "sns:ListSubscriptionsByTopic",
          "sns:ListTagsForResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:sns:${var.aws_region}:${var.aws_account_id}:${var.name}-operations",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "TagProjectOperationsTopic"
        "Effect" = "Allow"
        "Action" = [
          "sns:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:sns:${var.aws_region}:${var.aws_account_id}:${var.name}-operations",
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
        "Sid"    = "UntagProjectOperationsTopicWithoutChangingOwner"
        "Effect" = "Allow"
        "Action" = [
          "sns:UntagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:sns:${var.aws_region}:${var.aws_account_id}:${var.name}-operations",
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
        "Sid"    = "ManageProjectTrail"
        "Effect" = "Allow"
        "Action" = [
          "cloudtrail:GetTrail",
          "cloudtrail:GetTrailStatus",
          "cloudtrail:GetEventSelectors",
          "cloudtrail:ListTags",
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
        "Sid"    = "TrailDiscovery"
        "Effect" = "Allow"
        "Action" = [
          "cloudtrail:DescribeTrails",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "ManageProjectAnalyzer"
        "Effect" = "Allow"
        "Action" = [
          "access-analyzer:GetAnalyzer",
          "access-analyzer:ListTagsForResource",
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
        "Sid"    = "AnalyzerDiscovery"
        "Effect" = "Allow"
        "Action" = [
          "access-analyzer:ListAnalyzers",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "ManageProjectBudget"
        "Effect" = "Allow"
        "Action" = [
          "budgets:ModifyBudget",
          "budgets:ViewBudget",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:budgets::${var.aws_account_id}:budget/${var.name}-monthly",
        ]
      },
    ]
  })
}
