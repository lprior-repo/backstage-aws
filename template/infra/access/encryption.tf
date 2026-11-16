resource "aws_iam_policy" "encryption" {
  name        = "${var.name}-provisioner-encryption"
  description = "Own and use only project state and application KMS keys and aliases"
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/encryption.json.tftpl", {
    partition  = data.aws_partition.current.partition
    region     = var.aws_region
    account_id = var.aws_account_id
    name       = var.name
  })))
}
