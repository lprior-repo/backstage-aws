variable "github_repository" {
  type    = string
  default = null
  validation {
    condition     = var.github_repository == null || can(regex("^[A-Za-z0-9-]{1,39}/[A-Za-z0-9_.-]{1,100}$", var.github_repository))
    error_message = "Use an owner/repository GitHub identifier."
  }
}

variable "environment" {
  type        = string
  default     = "preview"
  description = "`production` accepts workflow runs on the main branch only; any other value accepts every run in the repository."
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{1,19}$", var.environment))
    error_message = "Use a 2-20 character lowercase environment name."
  }
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
  task_role_arns = [
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-execution",
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-task",
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-task",
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-execution"
  ]
}

resource "aws_iam_role" "github_deploy" {
  count                = var.github_repository != null ? 1 : 0
  name                 = "${var.name}-github-deploy"
  max_session_duration = 3600
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRoleWithWebIdentity"
      Principal = { Federated = local.github_provider_arn }
      # GitHub's token names the repository and what triggered the run: a pull
      # request is `repo:<owner>/<repo>:pull_request`, a push to main is
      # `repo:<owner>/<repo>:ref:refs/heads/main`. Production must never be
      # reachable from a pull request.
      Condition = {
        StringEquals = { "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com" }
        StringLike = {
          "token.actions.githubusercontent.com:sub" = "repo:${var.github_repository}:${var.environment == "production" ? "ref:refs/heads/main" : "*"}"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "github_deploy" {
  count = var.github_repository != null ? 1 : 0
  role  = aws_iam_role.github_deploy[0].id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      { Effect = "Allow", Action = "ecr:GetAuthorizationToken", Resource = "*" },
      {
        Effect = "Allow"
        Action = [
          "ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:CompleteLayerUpload",
          "ecr:DescribeImages", "ecr:DescribeImageScanFindings", "ecr:DescribeRepositories", "ecr:GetDownloadUrlForLayer",
          "ecr:InitiateLayerUpload", "ecr:PutImage", "ecr:UploadLayerPart", "ecr:StartImageScan"
        ]
        Resource = "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal"
      },
      {
        Effect   = "Allow"
        Action   = ["ecs:CreateService", "ecs:DeleteService", "ecs:TagResource", "ecs:UpdateService"]
        Resource = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:service/${var.name}-cluster/${var.name}-*"
      },
      {
        # Task definitions are registered with the provider's default tags.
        Effect   = "Allow"
        Action   = "ecs:TagResource"
        Resource = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task-definition/${var.name}-*:*"
      },
      {
        Effect   = "Allow"
        Action   = ["ecs:RegisterTaskDefinition", "ecs:DeregisterTaskDefinition", "ecs:Describe*", "ecs:List*"]
        Resource = "*"
      },
      # Read-only load balancer calls take no resource-level permissions.
      { Effect = "Allow", Action = "elasticloadbalancing:Describe*", Resource = "*" },
      {
        # Clones and drops a namespace database; no other database task is allowed.
        Effect   = "Allow"
        Action   = "ecs:RunTask"
        Resource = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task-definition/${var.name}-db-admin-clone:*"
        Condition = {
          ArnEquals = { "ecs:cluster" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster" }
        }
      },
      {
        Effect   = "Allow"
        Action   = "ecs:DescribeTasks"
        Resource = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task/${var.name}-cluster/*"
      },
      {
        Effect    = "Allow"
        Action    = "iam:PassRole"
        Resource  = local.task_role_arns
        Condition = { StringEquals = { "iam:PassedToService" = "ecs-tasks.amazonaws.com" } }
      },
      {
        Effect    = "Allow"
        Action    = ["elasticloadbalancing:CreateTargetGroup", "elasticloadbalancing:AddTags"]
        Resource  = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/*"
        Condition = { StringEquals = { "aws:RequestTag/Project" = var.name } }
      },
      {
        Effect    = "Allow"
        Action    = ["elasticloadbalancing:ModifyTargetGroup", "elasticloadbalancing:ModifyTargetGroupAttributes", "elasticloadbalancing:DeleteTargetGroup"]
        Resource  = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/*"
        Condition = { StringEquals = { "aws:ResourceTag/Project" = var.name } }
      },
      {
        Effect = "Allow"
        Action = ["elasticloadbalancing:CreateRule", "elasticloadbalancing:ModifyRule", "elasticloadbalancing:DeleteRule", "elasticloadbalancing:AddTags"]
        Resource = [
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener/app/${var.name}-alb/*",
          "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener-rule/app/${var.name}-alb/*"
        ]
      },
      {
        Effect = "Allow"
        Action = [
          "logs:CreateLogGroup", "logs:DeleteLogGroup", "logs:ListTagsForResource",
          "logs:PutRetentionPolicy", "logs:DeleteRetentionPolicy", "logs:AssociateKmsKey",
          "logs:DisassociateKmsKey", "logs:TagResource", "logs:UntagResource"
        ]
        Resource = "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*"
      },
      { Effect = "Allow", Action = "logs:DescribeLogGroups", Resource = "*" },
      { Effect = "Allow", Action = "s3:ListBucket", Resource = aws_s3_bucket.state.arn },
      { Effect = "Allow", Action = "s3:GetObject", Resource = "${aws_s3_bucket.state.arn}/base/terraform.tfstate" },
      { Effect = "Allow", Action = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"], Resource = "${aws_s3_bucket.state.arn}/env/*" },
      # State objects are encrypted with the bucket's default key.
      { Effect = "Allow", Action = ["kms:Decrypt", "kms:GenerateDataKey"], Resource = aws_kms_key.state.arn },
    ]
  })
}

output "github_deploy_role_arn" { value = try(aws_iam_role.github_deploy[0].arn, null) }
