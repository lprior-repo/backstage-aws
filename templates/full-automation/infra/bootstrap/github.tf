variable "github_repository" {
  type        = string
  default     = null
  description = "Exact owner/repo. Null creates no CI roles. Use protected GitHub environments."
}
variable "github_environment" {
  type    = string
  default = "production-infrastructure"
}
variable "github_build_environment" {
  type    = string
  default = "production-build"
}
variable "existing_github_oidc_provider_arn" {
  type        = string
  default     = null
  description = "Use an existing account OIDC provider instead of creating a duplicate."
}
resource "aws_iam_openid_connect_provider" "github" {
  count          = var.github_repository != null && var.existing_github_oidc_provider_arn == null ? 1 : 0
  url            = "https://token.actions.githubusercontent.com"
  client_id_list = ["sts.amazonaws.com"]
}
locals {
  github_provider_arn = var.github_repository == null ? null : coalesce(var.existing_github_oidc_provider_arn, try(aws_iam_openid_connect_provider.github[0].arn, null))
  core_role_names     = ["portal-execution", "bootstrap-execution", "portal-task", "bootstrap-task", "rollout", "flow-logs"]
  core_role_arns      = [for suffix in local.core_role_names : "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-${suffix}"]
}
resource "aws_iam_role" "github_build" {
  count                = var.github_repository != null ? 1 : 0
  name                 = "${var.name}-github-build"
  max_session_duration = 7200
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{ Effect = "Allow", Action = "sts:AssumeRoleWithWebIdentity",
      Principal = { Federated = local.github_provider_arn },
      Condition = { StringEquals = {
        "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com",
        "token.actions.githubusercontent.com:sub" = "repo:${var.github_repository}:environment:${var.github_build_environment}"
      } }
    }]
  })
}
resource "aws_iam_role_policy" "github_build" {
  count = var.github_repository != null ? 1 : 0
  role  = aws_iam_role.github_build[0].id
  policy = jsonencode({
    Version = "2012-10-17", Statement = [
      { Effect = "Allow", Action = "ecr:GetAuthorizationToken", Resource = "*" },
      { Effect = "Allow", Action = ["ecr:BatchCheckLayerAvailability", "ecr:InitiateLayerUpload", "ecr:UploadLayerPart", "ecr:CompleteLayerUpload", "ecr:PutImage", "ecr:DescribeImages", "ecr:DescribeImageScanFindings", "ecr:StartImageScan"],
      Resource = "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal" }
    ]
  })
}
# This is a privileged infrastructure role, deliberately distinct from the image
# builder. Its IAM grants exclude bootstrap/CI-role mutation and direct secret
# retrieval. It can deploy code using application roles: it remains privileged.
# The trusted repository/environment is a security boundary; review IaC first.
resource "aws_iam_role" "github_infrastructure" {
  count                = var.github_repository != null ? 1 : 0
  name                 = "${var.name}-github-infrastructure"
  max_session_duration = 7200
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{ Effect = "Allow", Action = "sts:AssumeRoleWithWebIdentity",
      Principal = { Federated = local.github_provider_arn },
      Condition = { StringEquals = {
        "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com",
        "token.actions.githubusercontent.com:sub" = "repo:${var.github_repository}:environment:${var.github_environment}"
      } }
    }]
  })
}
resource "aws_iam_role_policy" "github_infrastructure" {
  count = var.github_repository != null ? 1 : 0
  role  = aws_iam_role.github_infrastructure[0].id
  policy = jsonencode({
    Version = "2012-10-17", Statement = [
      {
        Sid      = "InfrastructureControlPlane", Effect = "Allow",
        Action   = ["ec2:*", "ecs:*", "elasticloadbalancing:*", "rds:*", "acm:*", "route53:*", "logs:*", "cloudwatch:*", "events:*", "states:*", "sns:*", "dynamodb:*", "ecr:Describe*", "ecr:List*", "ecr:Get*", "ecr:CreateRepository", "ecr:StartImageScan", "ecr:DeleteRepository", "ecr:PutLifecyclePolicy", "ecr:DeleteLifecyclePolicy", "ecr:SetRepositoryPolicy", "ecr:DeleteRepositoryPolicy", "ecr:TagResource", "ecr:UntagResource", "ecr:PutImageTagMutability", "ecr:PutImageScanningConfiguration", "secretsmanager:CreateSecret", "secretsmanager:DescribeSecret", "secretsmanager:ListSecrets", "secretsmanager:UpdateSecret", "secretsmanager:DeleteSecret", "secretsmanager:RestoreSecret", "secretsmanager:TagResource", "secretsmanager:UntagResource", "secretsmanager:GetResourcePolicy", "secretsmanager:PutResourcePolicy", "secretsmanager:DeleteResourcePolicy", "kms:CreateKey", "kms:ListKeys", "kms:ListAliases", "kms:CreateAlias", "kms:UpdateAlias", "kms:DeleteAlias", "kms:DescribeKey", "kms:GetKeyPolicy", "kms:PutKeyPolicy", "kms:GetKeyRotationStatus", "kms:EnableKeyRotation", "kms:DisableKeyRotation", "kms:ListResourceTags", "kms:TagResource", "kms:UntagResource", "kms:CreateGrant", "kms:ListGrants", "kms:RevokeGrant", "kms:DescribeCustomKeyStores"],
        Resource = "*"
      },
      {
        Sid      = "ProjectS3", Effect = "Allow", Action = "s3:*",
        Resource = ["arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-alb-logs", "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-alb-logs/*"]
      },
      {
        Sid = "CoreStateBucketMetadata", Effect = "Allow", Action = ["s3:ListBucket", "s3:GetBucketLocation"], Resource = aws_s3_bucket.state.arn
      },
      {
        Sid = "OnlyCoreState", Effect = "Allow", Action = ["s3:GetObject", "s3:PutObject"], Resource = ["${aws_s3_bucket.state.arn}/${var.state_key}", "${aws_s3_bucket.state.arn}/${var.state_key}.tflock"]
      },
      {
        Sid = "OnlyCoreStateLockDelete", Effect = "Allow", Action = "s3:DeleteObject", Resource = "${aws_s3_bucket.state.arn}/${var.state_key}.tflock"
      },
      { Sid = "StateEncryption", Effect = "Allow", Action = ["kms:Encrypt", "kms:Decrypt", "kms:GenerateDataKey"], Resource = aws_kms_key.state.arn },
      {
        Sid       = "LockEncryption", Effect = "Allow", Action = ["kms:Decrypt", "kms:GenerateDataKey"], Resource = "*",
        Condition = { StringEquals = { "kms:ViaService" = "dynamodb.${var.aws_region}.amazonaws.com" } }
      },
      {
        Sid      = "FixedApplicationRoles", Effect = "Allow",
        Action   = ["iam:CreateRole", "iam:GetRole", "iam:UpdateAssumeRolePolicy", "iam:DeleteRole", "iam:PutRolePolicy", "iam:GetRolePolicy", "iam:DeleteRolePolicy", "iam:ListRolePolicies", "iam:ListRoleTags", "iam:ListAttachedRolePolicies", "iam:TagRole", "iam:UntagRole", "iam:ListInstanceProfilesForRole"],
        Resource = local.core_role_arns
      },
      { Effect = "Allow", Action = "iam:PassRole", Resource = local.core_role_arns,
      Condition = { StringEquals = { "iam:PassedToService" = ["ecs-tasks.amazonaws.com", "states.amazonaws.com", "vpc-flow-logs.amazonaws.com"] } } },
      { Effect = "Allow", Action = "iam:CreateServiceLinkedRole", Resource = "*",
      Condition = { StringEquals = { "iam:AWSServiceName" = ["ecs.amazonaws.com", "rds.amazonaws.com", "elasticloadbalancing.amazonaws.com"] } } },
      { Effect = "Allow", Action = ["access-analyzer:ValidatePolicy", "iam:GetAccountSummary", "cloudtrail:GetTrailStatus"], Resource = "*" }
    ]
  })
}
output "github_build_role_arn" { value = try(aws_iam_role.github_build[0].arn, null) }
output "github_infrastructure_role_arn" { value = try(aws_iam_role.github_infrastructure[0].arn, null) }
