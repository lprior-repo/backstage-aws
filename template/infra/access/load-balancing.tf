resource "aws_iam_policy" "load_balancing" {
  name = "${var.name}-provisioner-load-balancing"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/load-balancing.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
  })))
}
