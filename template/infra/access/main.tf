data "aws_partition" "current" {}

locals {
  has_dns = var.hosted_zone_id != "" && var.domain_name != ""
  provisioner_policies = merge({
    load_balancing = aws_iam_policy.load_balancing.name
    network        = aws_iam_policy.network.name
    ec2            = aws_iam_policy.ec2.name
    storage        = aws_iam_policy.storage.name
    encryption     = aws_iam_policy.encryption.name
    rds            = aws_iam_policy.rds.name
    data_services  = aws_iam_policy.data_services.name
    operations     = aws_iam_policy.operations.name
    }, local.has_dns ? {
    dns = aws_iam_policy.dns[0].name
    } : {}, var.enable_account_controls ? {
    account_controls = aws_iam_policy.account_controls[0].name
  } : {})
}

resource "aws_ssoadmin_permission_set" "provisioner" {
  provider         = aws.identity_center
  name             = "${var.name}-provisioner"
  description      = "Project-scoped Terraform and CLI provisioning; not an administrator role."
  instance_arn     = var.identity_center_instance_arn
  session_duration = "PT1H"
}

resource "aws_ssoadmin_customer_managed_policy_attachment" "provisioner" {
  provider           = aws.identity_center
  for_each           = local.provisioner_policies
  instance_arn       = var.identity_center_instance_arn
  permission_set_arn = aws_ssoadmin_permission_set.provisioner.arn
  customer_managed_policy_reference {
    name = each.value
    path = "/"
  }
}

resource "aws_ssoadmin_account_assignment" "provisioner" {
  provider           = aws.identity_center
  instance_arn       = var.identity_center_instance_arn
  permission_set_arn = aws_ssoadmin_permission_set.provisioner.arn
  principal_id       = var.principal_id
  principal_type     = "USER"
  target_id          = var.aws_account_id
  target_type        = "AWS_ACCOUNT"

  depends_on = [
    aws_ssoadmin_customer_managed_policy_attachment.provisioner,
    aws_ssoadmin_permission_set_inline_policy.provisioner,
  ]
}
