mock_provider "aws" {
  mock_data "aws_caller_identity" { defaults = { account_id = "123456789012" } }
  mock_data "aws_partition" { defaults = { partition = "aws" } }
}
mock_provider "github" {}

variables {
  aws_account_id           = "123456789012"
  aws_region               = "us-east-1"
  name                     = "backstage"
  github_repository        = "example/backstage"
  github_environment       = "preview"
  github_actions_variables = { BACKSTAGE_AWS_NAME = "backstage" }
}

run "one_preview_environment_and_scoped_variables" {
  command = apply
  assert {
    condition     = github_repository_environment.deploy[0].environment == "preview"
    error_message = "Exactly the configured environment is created."
  }
  assert {
    condition     = length(github_repository_environment_deployment_policy.deploy) == 0 && length(github_repository_environment.deploy[0].reviewers) == 0
    error_message = "Preview has no production-only reviewer or branch policy."
  }
  assert {
    condition     = toset(keys(github_actions_environment_variable.settings)) == toset(["BACKSTAGE_AWS_NAME", "BACKSTAGE_AWS_DEPLOY_ROLE_ARN"])
    error_message = "User and computed variables are complete and environment-scoped."
  }
  assert {
    condition     = github_actions_environment_variable.settings["BACKSTAGE_AWS_NAME"].environment == "preview"
    error_message = "Actions variables must be scoped to the deployment environment."
  }
  assert {
    condition = jsondecode(aws_iam_role.github_deploy[0].assume_role_policy).Statement[0].Condition.StringEquals == {
      "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
      "token.actions.githubusercontent.com:sub" = "repo:example/backstage:environment:preview"
    }
    error_message = "OIDC trust must use exact audience and subject StringEquals conditions."
  }
  assert {
    condition = (
      length(jsondecode(aws_iam_role.github_deploy[0].assume_role_policy).Statement) == 1 &&
      jsondecode(aws_iam_role.github_deploy[0].assume_role_policy).Statement[0].Action == "sts:AssumeRoleWithWebIdentity" &&
      keys(jsondecode(aws_iam_role.github_deploy[0].assume_role_policy).Statement[0].Principal) == ["Federated"]
    )
    error_message = "Only the GitHub OIDC provider may assume the deploy role, and only with a web identity."
  }
  assert {
    condition = alltrue(flatten([
      for statement in jsondecode(aws_iam_role_policy.github_deploy[0].policy).Statement : [
        for action in flatten([statement.Action]) : contains([
          "ecr:GetAuthorizationToken", "ecs:RegisterTaskDefinition", "ecs:DeregisterTaskDefinition",
          "ecs:Describe*", "ecs:List*", "elasticloadbalancing:Describe*", "logs:DescribeLogGroups"
        ], action)
      ] if statement.Resource == "*" && !can(statement.Condition)
    ]))
    error_message = "Only read-only and task-definition actions may apply to every resource without a condition."
  }
  assert {
    condition     = aws_iam_role.github_deploy[0].max_session_duration == 3600
    error_message = "Deploy role sessions must be limited to one hour."
  }
  assert {
    condition = alltrue([
      for action in flatten([
        for statement in jsondecode(aws_iam_role_policy.github_deploy[0].policy).Statement : [statement.Action]
      ]) : !startswith(action, "iam:") || contains(["iam:PassRole", "iam:GetRole"], action)
    ])
    error_message = "Deploy policy may use only iam:PassRole and iam:GetRole."
  }
  assert {
    condition = alltrue([
      for action in flatten([
        for statement in jsondecode(aws_iam_role_policy.github_deploy[0].policy).Statement : [statement.Action]
      ]) : !endswith(action, ":*")
    ])
    error_message = "Deploy policy must not grant service-wide wildcard actions."
  }
  assert {
    condition = alltrue([
      for action in flatten([
        for statement in jsondecode(aws_iam_role_policy.github_deploy[0].policy).Statement : [statement.Action]
      ]) : !startswith(action, "secretsmanager:")
    ])
    error_message = "Deploy policy must not read, write, create or delete secrets."
  }
  assert {
    condition = one([
      for statement in jsondecode(aws_iam_role_policy.github_deploy[0].policy).Statement : statement
      if contains(flatten([statement.Action]), "iam:PassRole")
      ]).Resource == [
      "arn:aws:iam::123456789012:role/backstage-portal-execution",
      "arn:aws:iam::123456789012:role/backstage-portal-task",
      "arn:aws:iam::123456789012:role/backstage-db-admin-task",
      "arn:aws:iam::123456789012:role/backstage-db-admin-execution"
      ] && one([
        for statement in jsondecode(aws_iam_role_policy.github_deploy[0].policy).Statement : statement
        if contains(flatten([statement.Action]), "iam:PassRole")
    ]).Condition.StringEquals["iam:PassedToService"] == "ecs-tasks.amazonaws.com"
    error_message = "PassRole must be limited to the four ECS roles and ECS tasks."
  }
}

run "production_requires_reviewer_and_main_policy" {
  command = apply
  variables {
    github_environment = "production"
    owner_github_id    = "123456789"
  }
  assert {
    condition     = contains(github_repository_environment.deploy[0].reviewers[0].users, 123456789) && github_repository_environment_deployment_policy.deploy[0].branch_pattern == "main"
    error_message = "Production requires owner review and main-only deployments."
  }
}
