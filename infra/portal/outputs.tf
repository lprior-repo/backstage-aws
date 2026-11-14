output "url" { value = "https://${var.hostname}" }
output "deploy_role_arn" { value = aws_iam_role.deploy.arn }
output "ecs_cluster" { value = aws_ecs_cluster.portal.name }
output "ecs_service" { value = aws_ecs_service.portal.name }
output "ecs_task_family" { value = aws_ecs_task_definition.portal.family }
output "techdocs_bucket" { value = aws_s3_bucket.techdocs.id }
