resource "aws_iam_policy" "ec2" {
  name = "${var.name}-provisioner-ec2"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/ec2.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
  })))
}
