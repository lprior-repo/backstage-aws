resource "aws_iam_policy" "encryption" {
  name        = "${var.name}-provisioner-encryption"
  description = "Own and use only project state and application KMS keys and aliases"
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "CreateProjectKeysWithOwnershipTag"
        "Effect" = "Allow"
        "Action" = [
          "kms:CreateKey",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Sid"    = "TagNewProjectKeys"
        "Effect" = "Allow"
        "Action" = [
          "kms:TagResource",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"     = var.aws_region
            "aws:RequestTag/Project"  = var.name
            "aws:ResourceTag/Project" = var.name
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
        "Sid"    = "ManageOwnedKeys"
        "Effect" = "Allow"
        "Action" = [
          "kms:DescribeKey",
          "kms:GetKeyPolicy",
          "kms:PutKeyPolicy",
          "kms:EnableKeyRotation",
          "kms:ListResourceTags",
          "kms:GetKeyRotationStatus",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "aws:RequestedRegion"     = var.aws_region
          }
        }
      },
      {
        "Sid"    = "ManageExactProjectAliases"
        "Effect" = "Allow"
        "Action" = [
          "kms:CreateAlias",
          "kms:UpdateAlias",
          "kms:DeleteAlias",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:alias/${var.name}-terraform-state",
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:alias/${var.name}-application",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "AliasTargetsMustBeOwnedKeys"
        "Effect" = "Allow"
        "Action" = [
          "kms:CreateAlias",
          "kms:UpdateAlias",
          "kms:DeleteAlias",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "aws:RequestedRegion"     = var.aws_region
          }
        }
      },
      {
        "Sid"    = "DiscoverProjectAliases"
        "Effect" = "Allow"
        "Action" = [
          "kms:ListAliases",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Sid"    = "UseOwnedKeysThroughS3WithExactContext"
        "Effect" = "Allow"
        "Action" = [
          "kms:Encrypt",
          "kms:Decrypt",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project"          = var.name
            "aws:RequestedRegion"              = var.aws_region
            "kms:ViaService"                   = "s3.${var.aws_region}.amazonaws.com"
            "kms:EncryptionContext:aws:s3:arn" = "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-tfstate"
          }
        }
      },
      {
        "Sid"    = "UseOwnedKeysThroughS3ForSeedObjects"
        "Effect" = "Allow"
        "Action" = [
          "kms:Encrypt",
          "kms:Decrypt",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "aws:RequestedRegion"     = var.aws_region
            "kms:ViaService"          = "s3.${var.aws_region}.amazonaws.com"
          }
          "StringLike" = {
            "kms:EncryptionContext:aws:s3:arn" = "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-seed/*"
          }
        }
      },
      {
        "Sid"    = "UseOwnedKeysThroughNamedServices"
        "Effect" = "Allow"
        "Action" = [
          "kms:Encrypt",
          "kms:Decrypt",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "aws:RequestedRegion"     = var.aws_region
          }
          "StringLike" = {
            "kms:ViaService" = [
              "secretsmanager.${var.aws_region}.amazonaws.com",
              "logs.${var.aws_region}.amazonaws.com",
              "rds.${var.aws_region}.amazonaws.com",
              "sns.${var.aws_region}.amazonaws.com",
            ]
          }
        }
      },
      {
        "Sid"    = "RdsServiceGrantForOwnedKey"
        "Effect" = "Allow"
        "Action" = [
          "kms:CreateGrant",
          "kms:ListGrants",
          "kms:RevokeGrant",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "Bool" = {
            "kms:GrantIsForAWSResource" = "true"
          }
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "aws:RequestedRegion"     = var.aws_region
            "kms:ViaService"          = "rds.${var.aws_region}.amazonaws.com"
          }
        }
      },
      {
        "Sid"    = "RdsSecretsManagerGrantForOwnedKey"
        "Effect" = "Allow"
        "Action" = [
          "kms:CreateGrant",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:kms:${var.aws_region}:${var.aws_account_id}:key/*",
        ]
        "Condition" = {
          "Bool" = {
            "kms:GrantIsForAWSResource" = "true"
          }
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "aws:RequestedRegion"     = var.aws_region
            "kms:ViaService"          = "secretsmanager.${var.aws_region}.amazonaws.com"
          }
        }
      },
    ]
  })
}
