resource "aws_iam_policy" "load_balancing" {
  name = "${var.name}-provisioner-load-balancing"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "DescribeRegionalLoadBalancing"
        "Effect" = "Allow"
        "Action" = [
          "elasticloadbalancing:DescribeLoadBalancers",
          "elasticloadbalancing:DescribeLoadBalancerAttributes",
          "elasticloadbalancing:DescribeListeners",
          "elasticloadbalancing:DescribeListenerAttributes",
          "elasticloadbalancing:DescribeRules",
          "elasticloadbalancing:DescribeTargetGroups",
          "elasticloadbalancing:DescribeTargetGroupAttributes",
          "elasticloadbalancing:DescribeTargetHealth",
          "elasticloadbalancing:DescribeTags",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"      = "CreateProjectLoadBalancer"
        "Effect"   = "Allow"
        "Action"   = "elasticloadbalancing:CreateLoadBalancer"
        "Resource" = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:loadbalancer/app/${var.name}-alb/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"      = "CreateProjectListener"
        "Effect"   = "Allow"
        "Action"   = "elasticloadbalancing:CreateListener"
        "Resource" = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:loadbalancer/app/${var.name}-alb/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Sid"      = "CreateProjectRule"
        "Effect"   = "Allow"
        "Action"   = "elasticloadbalancing:CreateRule"
        "Resource" = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener/app/${var.name}-alb/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Sid"      = "CreateProjectTargets"
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
        "Sid"    = "TagNewLoadBalancingResources"
        "Effect" = "Allow"
        "Action" = "elasticloadbalancing:AddTags"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:loadbalancer/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener-rule/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/bs-*/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "elasticloadbalancing:CreateAction" = [
              "CreateLoadBalancer",
              "CreateListener",
              "CreateRule",
              "CreateTargetGroup",
            ]
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "MaintainProjectLoadBalancing"
        "Effect" = "Allow"
        "Action" = [
          "elasticloadbalancing:DeleteLoadBalancer",
          "elasticloadbalancing:ModifyLoadBalancerAttributes",
          "elasticloadbalancing:SetSecurityGroups",
          "elasticloadbalancing:SetSubnets",
          "elasticloadbalancing:DeleteListener",
          "elasticloadbalancing:ModifyListener",
          "elasticloadbalancing:ModifyListenerAttributes",
          "elasticloadbalancing:ModifyRule",
          "elasticloadbalancing:DeleteRule",
          "elasticloadbalancing:ModifyTargetGroup",
          "elasticloadbalancing:ModifyTargetGroupAttributes",
          "elasticloadbalancing:DeleteTargetGroup",
          "elasticloadbalancing:RegisterTargets",
          "elasticloadbalancing:DeregisterTargets",
          "elasticloadbalancing:AddTags",
          "elasticloadbalancing:RemoveTags",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:loadbalancer/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener-rule/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/bs-*/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
    ]
  })
}
