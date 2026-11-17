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

# IAM can reject a newly created policy as a permissions boundary for a short time.
# Waiting here lets the deploy role attach it in the same apply without a manual retry.
resource "time_sleep" "workload_boundary" {
  create_duration = "30s"
  depends_on      = [aws_iam_policy.workload_boundary]
}
