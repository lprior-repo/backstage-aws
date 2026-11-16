resource "aws_iam_policy" "rds" {
  name        = "${var.name}-provisioner-rds"
  description = "Provision the project RDS database, subnet group, and parameter group"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/rds.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
  })))
}
