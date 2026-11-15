resource "aws_iam_policy" "network" {
  name = "${var.name}-provisioner-network"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:DescribeAccountAttributes",
          "ec2:DescribeAvailabilityZones",
          "ec2:DescribeFlowLogs",
          "ec2:DescribeImages",
          "ec2:DescribeInstanceStatus",
          "ec2:DescribeInstanceCreditSpecifications",
          "ec2:DescribeInstanceTypes",
          "ec2:DescribeInstances",
          "ec2:DescribeInternetGateways",
          "ec2:DescribeNatGateways",
          "ec2:DescribeNetworkAcls",
          "ec2:DescribeNetworkInterfaceAttribute",
          "ec2:DescribeNetworkInterfaces",
          "ec2:DescribeRouteTables",
          "ec2:DescribeSecurityGroupRules",
          "ec2:DescribeSecurityGroups",
          "ec2:DescribeSubnets",
          "ec2:DescribeTags",
          "ec2:DescribeVolumes",
          "ec2:DescribeVpcAttribute",
          "ec2:DescribeVpcs",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "ec2:Region" = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateVpc"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:DeleteVpc",
          "ec2:ModifyVpcAttribute",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateInternetGateway"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:internet-gateway/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:DeleteInternetGateway",
          "ec2:AttachInternetGateway",
          "ec2:DetachInternetGateway",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:internet-gateway/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateSubnet"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:subnet/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateSubnet"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:DeleteSubnet",
          "ec2:ModifySubnetAttribute",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:subnet/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateRouteTable"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:route-table/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateRouteTable"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:DeleteRouteTable",
          "ec2:CreateRoute",
          "ec2:ReplaceRoute",
          "ec2:DeleteRoute",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:route-table/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:AssociateRouteTable",
          "ec2:DisassociateRouteTable",
          "ec2:ReplaceRouteTableAssociation",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:route-table/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:subnet/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:internet-gateway/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateSecurityGroup"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:security-group/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateSecurityGroup"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:DeleteSecurityGroup",
          "ec2:AuthorizeSecurityGroupIngress",
          "ec2:AuthorizeSecurityGroupEgress",
          "ec2:RevokeSecurityGroupIngress",
          "ec2:RevokeSecurityGroupEgress",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:security-group/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = [
          "ec2:AuthorizeSecurityGroupIngress",
          "ec2:AuthorizeSecurityGroupEgress",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:security-group-rule/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = "ec2:CreateTags"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:subnet/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:internet-gateway/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:route-table/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:security-group/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:security-group-rule/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:instance/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:volume/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:network-interface/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:elastic-ip/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc-flow-log/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:natgateway/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "ec2:CreateAction" = [
              "AllocateAddress",
              "AuthorizeSecurityGroupEgress",
              "AuthorizeSecurityGroupIngress",
              "CreateFlowLogs",
              "CreateInternetGateway",
              "CreateNetworkInterface",
              "CreateRouteTable",
              "CreateSecurityGroup",
              "CreateSubnet",
              "CreateVpc",
              "RunInstances",
              "CreateNatGateway",
            ]
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
    ]
  })
}
