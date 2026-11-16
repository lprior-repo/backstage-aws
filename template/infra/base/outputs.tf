output "aws_account_id" { value = var.aws_account_id }
output "aws_region" { value = var.aws_region }
output "ecr_repository_url" { value = aws_ecr_repository.portal.repository_url }
output "ecr_repository_name" { value = aws_ecr_repository.portal.name }
output "vpc_id" { value = aws_vpc.main.id }
output "portal_security_group_id" { value = aws_security_group.app.id }
output "database_application_secret_arn" { value = aws_secretsmanager_secret.app_db.arn }
output "session_secret_arn" { value = aws_secretsmanager_secret.session.arn }
output "identity_secret_arn" { value = aws_secretsmanager_secret.identity.arn }
output "application_secret_arns" { value = { for name, secret in aws_secretsmanager_secret.plugin : name => secret.arn } }
output "database_admin_secret_arn" { value = aws_db_instance.database.master_user_secret[0].secret_arn }
output "database_endpoint" { value = aws_db_instance.database.address }
output "seed_bucket_name" { value = aws_s3_bucket.seed.id }
output "seed_bucket_arn" { value = aws_s3_bucket.seed.arn }
output "ecs_cluster_name" { value = aws_ecs_cluster.main.name }
output "ecs_cluster_arn" { value = aws_ecs_cluster.main.arn }
output "db_admin_init_task_definition_arn" { value = aws_ecs_task_definition.db_admin_init.arn }
output "db_admin_rotate_task_definition_arn" { value = aws_ecs_task_definition.db_admin_rotate.arn }
output "db_admin_seed_task_definition_arn" { value = aws_ecs_task_definition.db_admin_clone.arn }
output "db_admin_seed_restore_task_definition_arn" { value = aws_ecs_task_definition.db_admin_seed_restore.arn }
output "db_admin_dump_task_definition_arn" { value = aws_ecs_task_definition.db_admin_dump.arn }
output "db_admin_network_configuration" {
  value = {
    awsvpcConfiguration = {
      subnets        = aws_subnet.application[*].id
      securityGroups = [aws_security_group.admin.id]
      assignPublicIp = var.nat_gateways == 0 && !var.fck_nat ? "ENABLED" : "DISABLED"
    }
  }
}
output "application_subnet_ids" { value = aws_subnet.application[*].id }
output "assign_public_ip" { value = var.nat_gateways == 0 && !var.fck_nat }
output "db_admin_security_group_id" { value = aws_security_group.admin.id }
output "kms_key_arn" { value = aws_kms_key.app.arn }
output "log_groups" { value = { db_admin = aws_cloudwatch_log_group.db_admin.name, portal = aws_cloudwatch_log_group.portal.name } }
output "listener_arn" { value = local.listener_arn }
output "load_balancer_dns_name" { value = aws_lb.main.dns_name }
output "url_scheme" { value = local.domain_enabled ? "https" : "http" }
output "application_task_role_arn" { value = aws_iam_role.application_task.arn }
output "application_execution_role_arn" { value = aws_iam_role.application_execution.arn }
output "db_lifecycle_task_role_arn" { value = aws_iam_role.db_lifecycle_task.arn }
output "db_lifecycle_execution_role_arn" { value = aws_iam_role.db_lifecycle_execution.arn }
output "db_lifecycle_secret_arn" { value = aws_secretsmanager_secret.db_lifecycle.arn }
output "database_identifier" { value = aws_db_instance.database.identifier }
output "fck_nat_ami_id" { value = var.fck_nat ? data.aws_ami.fck_nat[0].id : null }
output "fck_nat_private_ip" { value = var.fck_nat ? aws_instance.fck_nat[0].private_ip : null }
