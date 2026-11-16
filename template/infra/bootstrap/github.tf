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
  # CI can only pass portal and lifecycle roles, never the db-admin roles that
  # hold the RDS master credential. The admin roles remain for provisioner use.
  task_role_arns = [
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-execution",
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-task",
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-lifecycle-task",
    "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-lifecycle-execution"
  ]
}

resource "aws_iam_role" "github_deploy" {
  count                = var.github_repository != null ? 1 : 0
  name                 = "${var.name}-github-deploy"
  max_session_duration = 3600
  permissions_boundary = "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:policy/${var.name}-workload-boundary"
  assume_role_policy = jsonencode(jsondecode(templatefile("${path.module}/policies/github-deploy-trust.json.tftpl", {
    github_oidc_provider_arn = local.github_provider_arn
    github_oidc_subject      = "repo:${var.github_repository}:${var.environment == "production" ? "ref:refs/heads/main" : "*"}"
  })))
}

resource "aws_iam_role_policy" "github_deploy" {
  count = var.github_repository != null ? 1 : 0
  role  = aws_iam_role.github_deploy[0].id
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/github-deploy-permissions.json.tftpl", {
    ecr_portal_repository_arn              = "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal"
    ecs_service_arn                        = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:service/${var.name}-cluster/${var.name}-*"
    ecs_task_definition_arn                = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task-definition/${var.name}-*:*"
    ecs_cluster_arn                        = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster"
    ecs_task_arn                           = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task/${var.name}-cluster/*"
    task_role_arns                         = local.task_role_arns
    elasticloadbalancing_target_group_arn  = "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:targetgroup/*"
    project_name                           = var.name
    elasticloadbalancing_listener_arns = [
      "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener/app/${var.name}-alb/*",
      "arn:${data.aws_partition.current.partition}:elasticloadbalancing:${var.aws_region}:${var.aws_account_id}:listener-rule/app/${var.name}-alb/*"
    ]
    log_group_arn                = "arn:${data.aws_partition.current.partition}:logs:${var.aws_region}:${var.aws_account_id}:log-group:/backstage/${var.name}/*"
    state_bucket_arn             = local.bucket_arn
    base_state_object_arn        = "${local.bucket_arn}/base/terraform.tfstate"
    environment_state_object_arn = "${local.bucket_arn}/env/*"
    state_kms_key_arn            = aws_kms_key.state.arn
  })))
}

output "github_deploy_role_arn" { value = try(aws_iam_role.github_deploy[0].arn, null) }
