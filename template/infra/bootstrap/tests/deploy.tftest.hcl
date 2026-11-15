mock_provider "aws" {
  mock_data "aws_caller_identity" { defaults = { account_id = "123456789012" } }
  mock_data "aws_partition" { defaults = { partition = "aws" } }
}

variables {
  aws_account_id    = "123456789012"
  aws_region        = "us-east-1"
  name              = "backstage"
  github_repository = "example/backstage"
  environment       = "preview"
}

run "preview_role_trusts_the_repository_and_nothing_wider" {
  command = apply
  assert {
    condition = jsondecode(aws_iam_role.github_deploy[0].assume_role_policy).Statement[0].Condition == {
      StringEquals = { "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com" }
      StringLike   = { "token.actions.githubusercontent.com:sub" = "repo:example/backstage:*" }
    }
    error_message = "The preview role must accept this repository's runs only, with the STS audience."
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
  assert {
    condition     = aws_iam_role.github_deploy[0].permissions_boundary == "arn:aws:iam::123456789012:policy/backstage-workload-boundary"
    error_message = "The GitHub deploy role must attach the exact account and project workload permissions boundary."
  }
}

run "production_role_trusts_the_main_branch_only" {
  command = apply
  variables {
    environment = "production"
  }
  assert {
    condition = jsondecode(aws_iam_role.github_deploy[0].assume_role_policy).Statement[0].Condition == {
      StringEquals = { "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com" }
      StringLike   = { "token.actions.githubusercontent.com:sub" = "repo:example/backstage:ref:refs/heads/main" }
    }
    error_message = "A pull request must never be able to assume the production role."
  }
}
