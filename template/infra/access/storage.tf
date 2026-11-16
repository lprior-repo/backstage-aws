resource "aws_iam_policy" "storage" {
  name        = "${var.name}-provisioner-storage"
  description = "Project Terraform state, seed upload, and audit bucket access"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/storage.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
  })))
}
