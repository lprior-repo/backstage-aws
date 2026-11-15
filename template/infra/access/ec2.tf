resource "aws_iam_policy" "ec2" {
  name = "${var.name}-provisioner-ec2"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:RunInstances"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:instance/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:InstanceType"       = "t4g.nano"
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:RunInstances"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:volume/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:RunInstances"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:network-interface/*"
        "Condition" = {
          "StringEquals" = {
            "ec2:Region" = var.aws_region
          }
          "Null" = {
            "ec2:NetworkInterfaceID" = "true"
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:RunInstances"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}::image/*"
        "Condition" = {
          "StringEquals" = {
            "ec2:Owner"  = "568608671756"
            "ec2:Region" = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:RunInstances"
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
        "Action"   = "ec2:RunInstances"
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
          "ec2:TerminateInstances",
          "ec2:ModifyInstanceAttribute",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:instance/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:AllocateAddress"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:elastic-ip/*"
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
          "ec2:ReleaseAddress",
          "ec2:AssociateAddress",
          "ec2:DisassociateAddress",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:elastic-ip/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:instance/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateFlowLogs"
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
        "Action"   = "ec2:CreateFlowLogs"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc-flow-log/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Effect"   = "Allow"
        "Action"   = "ec2:DeleteFlowLogs"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc-flow-log/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Sid"      = "CreateOwnedManagedNat"
        "Effect"   = "Allow"
        "Action"   = "ec2:CreateNatGateway"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:natgateway/*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestTag/Project" = var.name
            "ec2:Region"             = var.aws_region
          }
        }
      },
      {
        "Sid"    = "ManagedNatUsesOwnedNetwork"
        "Effect" = "Allow"
        "Action" = "ec2:CreateNatGateway"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:subnet/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:elastic-ip/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
      {
        "Sid"      = "DeleteOwnedManagedNat"
        "Effect"   = "Allow"
        "Action"   = "ec2:DeleteNatGateway"
        "Resource" = "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:natgateway/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
        }
      },
    ]
  })
}
