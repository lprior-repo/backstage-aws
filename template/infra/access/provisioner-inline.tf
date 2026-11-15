resource "aws_ssoadmin_permission_set_inline_policy" "provisioner" {
  provider           = aws.identity_center
  instance_arn       = var.identity_center_instance_arn
  permission_set_arn = aws_ssoadmin_permission_set.provisioner.arn
  inline_policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "AccountDiscovery"
        "Effect" = "Allow"
        "Action" = [
          "sts:GetCallerIdentity",
          "iam:GetAccountSummary",
          "iam:GetAccountPasswordPolicy",
          "iam:ListOpenIDConnectProviders",
          "organizations:DescribeOrganization",
          "sso:ListInstances",
          "s3:GetAccountPublicAccessBlock",
        ]
        "Resource" = "*"
      },
      {
        "Sid"    = "ScopedRoleManagement"
        "Effect" = "Allow"
        "Action" = [
          "iam:DeleteRole",
          "iam:GetRole",
          "iam:UpdateRole",
          "iam:UpdateAssumeRolePolicy",
          "iam:TagRole",
          "iam:UntagRole",
          "iam:PutRolePolicy",
          "iam:DeleteRolePolicy",
          "iam:GetRolePolicy",
          "iam:ListRolePolicies",
          "iam:ListAttachedRolePolicies",
          "iam:ListInstanceProfilesForRole",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-execution",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-execution",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-flow-logs",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-github-deploy",
        ]
      },
      {
        "Sid"    = "CreateOnlyBoundedRoles"
        "Effect" = "Allow"
        "Action" = [
          "iam:CreateRole",
          "iam:PutRolePermissionsBoundary",
        ]
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-execution",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-execution",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-flow-logs",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-github-deploy",
        ]
        "Condition" = {
          "ArnEquals" = {
            "iam:PermissionsBoundary" = aws_iam_policy.workload_boundary.arn
          }
        }
      },
      {
        "Sid"    = "PassEcsRoles"
        "Effect" = "Allow"
        "Action" = "iam:PassRole"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-portal-execution",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-task",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-db-admin-execution",
        ]
        "Condition" = {
          "StringEquals" = {
            "iam:PassedToService" = "ecs-tasks.amazonaws.com"
          }
        }
      },
      {
        "Sid"      = "PassFlowLogRole"
        "Effect"   = "Allow"
        "Action"   = "iam:PassRole"
        "Resource" = "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-flow-logs"
        "Condition" = {
          "StringEquals" = {
            "iam:PassedToService" = "vpc-flow-logs.amazonaws.com"
          }
        }
      },
      {
        "Sid"    = "RequiredServiceLinkedRoles"
        "Effect" = "Allow"
        "Action" = "iam:CreateServiceLinkedRole"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/aws-service-role/ecs.amazonaws.com/AWSServiceRoleForECS",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/aws-service-role/rds.amazonaws.com/AWSServiceRoleForRDS",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/aws-service-role/elasticloadbalancing.amazonaws.com/AWSServiceRoleForElasticLoadBalancing",
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/aws-service-role/access-analyzer.amazonaws.com/AWSServiceRoleForAccessAnalyzer",
        ]
        "Condition" = {
          "StringEquals" = {
            "iam:AWSServiceName" = [
              "ecs.amazonaws.com",
              "rds.amazonaws.com",
              "elasticloadbalancing.amazonaws.com",
              "access-analyzer.amazonaws.com",
            ]
          }
        }
      },
      {
        "Sid"    = "FixedGitHubOidcProvider"
        "Effect" = "Allow"
        "Action" = [
          "iam:CreateOpenIDConnectProvider",
          "iam:GetOpenIDConnectProvider",
          "iam:DeleteOpenIDConnectProvider",
          "iam:AddClientIDToOpenIDConnectProvider",
          "iam:RemoveClientIDFromOpenIDConnectProvider",
          "iam:UpdateOpenIDConnectProviderThumbprint",
          "iam:TagOpenIDConnectProvider",
          "iam:UntagOpenIDConnectProvider",
          "iam:ListOpenIDConnectProviderTags",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:oidc-provider/token.actions.githubusercontent.com"
      },
      {
        "Sid"      = "NoBoundaryRemoval"
        "Effect"   = "Deny"
        "Action"   = "iam:DeleteRolePermissionsBoundary"
        "Resource" = "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:role/${var.name}-*"
      },
      {
        "Sid"    = "NoBoundaryPolicyEditing"
        "Effect" = "Deny"
        "Action" = [
          "iam:CreatePolicy",
          "iam:DeletePolicy",
          "iam:CreatePolicyVersion",
          "iam:DeletePolicyVersion",
          "iam:SetDefaultPolicyVersion",
          "iam:TagPolicy",
          "iam:UntagPolicy",
        ]
        "Resource" = [
          aws_iam_policy.workload_boundary.arn,
          "arn:${data.aws_partition.current.partition}:iam::${var.aws_account_id}:policy/${var.name}-provisioner-*",
        ]
      },
      {
        "Sid"    = "ProjectCluster"
        "Effect" = "Allow"
        "Action" = [
          "ecs:CreateCluster",
          "ecs:DeleteCluster",
          "ecs:DescribeClusters",
          "ecs:PutClusterCapacityProviders",
          "ecs:UpdateClusterSettings",
          "ecs:ListTagsForResource",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster"
      },
      {
        "Sid"    = "ProjectServices"
        "Effect" = "Allow"
        "Action" = [
          "ecs:CreateService",
          "ecs:DeleteService",
          "ecs:DescribeServices",
          "ecs:UpdateService",
          "ecs:TagResource",
          "ecs:UntagResource",
          "ecs:ListTagsForResource",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:service/${var.name}-cluster/${var.name}-*"
      },
      {
        "Sid"    = "ProjectDefinitions"
        "Effect" = "Allow"
        "Action" = [
          "ecs:RegisterTaskDefinition",
          "ecs:TagResource",
          "ecs:UntagResource",
          "ecs:ListTagsForResource",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task-definition/${var.name}-*:*"
      },
      {
        "Sid"    = "TagOwnedCluster"
        "Effect" = "Allow"
        "Action" = [
          "ecs:TagResource",
          "ecs:UntagResource",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster"
      },
      {
        "Sid"      = "RunProjectDatabaseJobs"
        "Effect"   = "Allow"
        "Action"   = "ecs:RunTask"
        "Resource" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task-definition/${var.name}-db-admin-*:*"
        "Condition" = {
          "ArnEquals" = {
            "ecs:cluster" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster"
          }
        }
      },
      {
        "Sid"    = "ProjectTaskLifecycle"
        "Effect" = "Allow"
        "Action" = [
          "ecs:DescribeTasks",
          "ecs:StopTask",
          "ecs:TagResource",
          "ecs:UntagResource",
          "ecs:ListTagsForResource",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:task/${var.name}-cluster/*"
      },
      {
        "Sid"    = "ListProjectTasks"
        "Effect" = "Allow"
        "Action" = [
          "ecs:ListTasks",
          "ecs:ListServices",
        ]
        "Resource" = "*"
        "Condition" = {
          "ArnEquals" = {
            "ecs:cluster" = "arn:${data.aws_partition.current.partition}:ecs:${var.aws_region}:${var.aws_account_id}:cluster/${var.name}-cluster"
          }
        }
      },
      {
        "Sid"    = "RegionalUnscopedEcsApis"
        "Effect" = "Allow"
        "Action" = [
          "ecs:ListClusters",
          "ecs:ListTaskDefinitions",
          "ecs:DescribeTaskDefinition",
          "ecs:DeregisterTaskDefinition",
        ]
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion" = var.aws_region
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = "ec2:CreateTags"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:internet-gateway/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:subnet/*",
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
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "Project",
              "Environment",
              "ManagedBy",
            ]
          }
          "StringEqualsIfExists" = {
            "aws:RequestTag/Project" = var.name
          }
        }
      },
      {
        "Effect" = "Allow"
        "Action" = "ec2:DeleteTags"
        "Resource" = [
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:vpc/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:internet-gateway/*",
          "arn:${data.aws_partition.current.partition}:ec2:${var.aws_region}:${var.aws_account_id}:subnet/*",
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
            "aws:ResourceTag/Project" = var.name
            "ec2:Region"              = var.aws_region
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "Environment",
              "ManagedBy",
            ]
          }
        }
      },
    ]
  })
}
