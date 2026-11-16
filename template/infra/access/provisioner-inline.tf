resource "aws_ssoadmin_permission_set_inline_policy" "provisioner" {
  provider           = aws.identity_center
  instance_arn       = var.identity_center_instance_arn
  permission_set_arn = aws_ssoadmin_permission_set.provisioner.arn
  inline_policy = jsonencode(jsondecode(templatefile("${path.module}/policies/provisioner-inline.json.tftpl", {
    partition    = data.aws_partition.current.partition
    region       = var.aws_region
    account_id   = var.aws_account_id
    name         = var.name
    boundary_arn = aws_iam_policy.workload_boundary.arn
  })))
}
