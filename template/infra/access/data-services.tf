resource "aws_iam_policy" "data_services" {
  name        = "${var.name}-provisioner-data-services"
  description = "Provision project application secrets and ECR repository"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/data-services.json.tftpl", {
    project_secrets_arn = "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:${var.name}/*"
    rds_secret_arn      = "arn:${data.aws_partition.current.partition}:secretsmanager:${var.aws_region}:${var.aws_account_id}:secret:rds!db-*"
    rds_instance_arn    = "arn:${data.aws_partition.current.partition}:rds:${var.aws_region}:${var.aws_account_id}:db:${var.name}-postgres"
    repository_arn      = "arn:${data.aws_partition.current.partition}:ecr:${var.aws_region}:${var.aws_account_id}:repository/${var.name}/portal"
    region              = var.aws_region
    project             = var.name
  })))
}
