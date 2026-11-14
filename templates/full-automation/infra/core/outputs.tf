output "aws_account_id" {
  value = local.account_id
}

output "aws_region" {
  value = var.aws_region
}

output "ecr_repository_url" {
  value = aws_ecr_repository.portal.repository_url
}

output "ecr_repository_name" {
  value = aws_ecr_repository.portal.name
}

output "identity_secret_arn" {
  description = "Populate externally with clientId, clientSecret, sessionSecret. Values never enter Terraform state."
  value       = aws_secretsmanager_secret.identity.arn
}

output "database_application_secret_arn" {
  value = aws_secretsmanager_secret.database_application.arn
}

output "database_admin_secret_arn" {
  description = "Managed by RDS; available only to the DB bootstrap task role, never the portal task."
  value       = aws_db_instance.database.master_user_secret[0].secret_arn
}

output "database_endpoint" {
  value = aws_db_instance.database.address
}

output "database_name" {
  value = var.database_name
}

output "database_engine_version_actual" {
  value = aws_db_instance.database.engine_version_actual
}

output "rollout_state_machine_arn" {
  value = aws_sfn_state_machine.rollout.arn
}

output "rollout_definition" {
  description = "Rendered ASL for AWS ValidateStateMachineDefinition or review; it contains ARNs but no secrets."
  value       = aws_sfn_state_machine.rollout.definition
}

output "rollout_enabled" {
  description = "False while the infrastructure-only placeholder image is configured."
  value       = var.image_digest != null
}

output "approved_image_digest" {
  description = "Digest fixed in the approved task definitions and rollout; null during the infrastructure-only initial stage."
  value       = var.image_digest
}

output "portal_url" {
  value = local.portal_base_url
}

output "ecs_cluster_name" {
  value = aws_ecs_cluster.portal.name
}

output "ecs_cluster_arn" {
  value = aws_ecs_cluster.portal.arn
}

output "ecs_service_name" {
  value = aws_ecs_service.portal.name
}

output "ecs_service_arn" {
  value = aws_ecs_service.portal.id
}

output "portal_task_definition_arn" {
  value = aws_ecs_task_definition.portal.arn
}

output "bootstrap_task_definition_arn" {
  value = aws_ecs_task_definition.bootstrap.arn
}

output "bootstrap_network_configuration" {
  description = "AWS CLI run-task network configuration, also used by the fixed rollout workflow."
  value = {
    awsvpcConfiguration = {
      subnets        = aws_subnet.application[*].id
      securityGroups = [aws_security_group.bootstrap.id]
      assignPublicIp = "DISABLED"
    }
  }
}

output "application_subnet_ids" {
  value = aws_subnet.application[*].id
}

output "bootstrap_security_group_id" {
  value = aws_security_group.bootstrap.id
}

output "kms_key_arn" {
  value = aws_kms_key.application.arn
}

output "log_groups" {
  value = {
    portal    = aws_cloudwatch_log_group.portal.name
    bootstrap = aws_cloudwatch_log_group.bootstrap.name
    rollout   = aws_cloudwatch_log_group.workflow.name
    vpc_flow  = aws_cloudwatch_log_group.flow.name
  }
}

output "operator_release_policy_json" {
  description = "Optional narrow day-2 operator permissions for image publishing, identity-secret setup, and rollout. This is NOT the broader Terraform provisioner policy and creates no principal."
  value = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ECRLogin"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid      = "PublishOnlyPortalRepository"
        Effect   = "Allow"
        Action   = ["ecr:BatchCheckLayerAvailability", "ecr:InitiateLayerUpload", "ecr:UploadLayerPart", "ecr:CompleteLayerUpload", "ecr:PutImage", "ecr:DescribeImages", "ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"]
        Resource = aws_ecr_repository.portal.arn
      },
      {
        Sid      = "SetOnlyPortalIdentitySecret"
        Effect   = "Allow"
        Action   = ["secretsmanager:PutSecretValue", "secretsmanager:DescribeSecret"]
        Resource = aws_secretsmanager_secret.identity.arn
      },
      {
        Sid      = "EncryptOnlyIdentitySecret"
        Effect   = "Allow"
        Action   = ["kms:GenerateDataKey", "kms:Decrypt"]
        Resource = aws_kms_key.application.arn
        Condition = { StringEquals = {
          "kms:ViaService"                  = "secretsmanager.${var.aws_region}.amazonaws.com"
          "kms:EncryptionContext:SecretARN" = aws_secretsmanager_secret.identity.arn
        } }
      },
      {
        Sid      = "StartOnlyApprovedRollout"
        Effect   = "Allow"
        Action   = "states:StartExecution"
        Resource = local.rollout_arn
      },
      {
        Sid      = "ObserveOnlyApprovedRollout"
        Effect   = "Allow"
        Action   = ["states:DescribeExecution", "states:GetExecutionHistory"]
        Resource = "arn:${local.partition}:states:${var.aws_region}:${local.account_id}:execution:${local.rollout_name}:*"
      }
    ]
  })
}

output "rollout_state_machine_version_arn" {
  description = "Immutable published workflow version for release receipts. Policies remain scoped to this workflow."
  value       = aws_sfn_state_machine.rollout.state_machine_version_arn
}
