resource "aws_iam_policy" "workload_boundary" {
  name        = "${var.name}-workload-boundary"
  description = "Owner-controlled workload boundary for ${var.name}; never editable by the provisioner."
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/workload-boundary.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
    dns_suffix = data.aws_partition.current.dns_suffix
  })))
}
