resource "aws_iam_policy" "network" {
  name = "${var.name}-provisioner-network"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/network.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
  })))
}
