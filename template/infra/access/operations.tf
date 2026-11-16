resource "aws_iam_policy" "operations" {
  name        = "${var.name}-provisioner-operations"
  description = "Provision project notifications, trail, analyzer, and budget"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/operations.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
  })))
}
