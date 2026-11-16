mock_provider "aws" {
  mock_data "aws_partition" {
    defaults = { partition = "aws", dns_suffix = "amazonaws.com" }
  }
  mock_resource "aws_iam_policy" {
    defaults = { arn = "arn:aws:iam::123456789012:policy/portal-workload-boundary" }
  }
}

mock_provider "aws" {
  alias = "identity_center"
  mock_resource "aws_ssoadmin_permission_set" {
    defaults = { arn = "arn:aws:sso:::permissionSet/ssoins-0123456789abcdef/ps-0123456789abcdef" }
  }
}

variables {
  name                         = "portal"
  aws_account_id               = "123456789012"
  identity_center_instance_arn = "arn:aws:sso:::instance/ssoins-0123456789abcdef"
  principal_id                 = "01234567-89ab-cdef-0123-456789abcdef"
}

run "default_access_cannot_enable_dns_or_account_controls" {
  command = apply

  assert {
    condition = (
      length(aws_iam_policy.dns) == 0 &&
      length(aws_iam_policy.account_controls) == 0 &&
      length(aws_ssoadmin_customer_managed_policy_attachment.provisioner) == 8
    )
    error_message = "Default access must omit DNS, certificate and account-control capabilities."
  }
  assert {
    condition = (
      aws_ssoadmin_permission_set.provisioner.session_duration == "PT1H" &&
      aws_ssoadmin_account_assignment.provisioner.principal_type == "USER" &&
      aws_ssoadmin_account_assignment.provisioner.principal_id == var.principal_id &&
      aws_ssoadmin_account_assignment.provisioner.target_id == var.aws_account_id
    )
    error_message = "Only the selected existing user and account may receive the one-hour provisioner."
  }
  assert {
    condition = one([
      for statement in jsondecode(aws_ssoadmin_permission_set_inline_policy.provisioner.inline_policy).Statement :
      statement.Condition.ArnEquals["iam:PermissionsBoundary"]
      if try(statement.Sid, "") == "CreateOnlyBoundedRoles"
    ]) == "arn:aws:iam::123456789012:policy/portal-workload-boundary"
    error_message = "Role creation must require the owner-controlled workload boundary."
  }
  assert {
    condition = one([
      for statement in jsondecode(aws_ssoadmin_permission_set_inline_policy.provisioner.inline_policy).Statement :
      statement.Effect == "Deny" && statement.Action == "iam:DeleteRolePermissionsBoundary" && statement.Resource == "arn:aws:iam::123456789012:role/portal-*"
      if try(statement.Sid, "") == "NoBoundaryRemoval"
    ])
    error_message = "The provisioner must not remove workload boundaries."
  }
  assert {
    condition = one([
      for statement in jsondecode(aws_ssoadmin_permission_set_inline_policy.provisioner.inline_policy).Statement :
      statement.Effect == "Deny" && contains(statement.Action, "iam:CreatePolicyVersion") && contains(statement.Resource, "arn:aws:iam::123456789012:policy/portal-provisioner-*")
      if try(statement.Sid, "") == "NoBoundaryPolicyEditing"
    ])
    error_message = "The provisioner must not rewrite its own capabilities or the workload boundary."
  }
}

run "optional_permissions_are_scoped_and_fit_aws_policy_limits" {
  command = apply
  variables {
    name                    = "abcdefghijklmnopqr"
    aws_region              = "ap-southeast-1"
    identity_center_region  = "us-east-1"
    hosted_zone_id          = "Z123456789ABC"
    domain_name             = "preview.example.com"
    enable_account_controls = true
  }

  assert {
    condition = (
      length(aws_ssoadmin_customer_managed_policy_attachment.provisioner) == 10 &&
      length(aws_iam_policy.account_controls) == 1 &&
      alltrue([for policy in [
        aws_iam_policy.workload_boundary, aws_iam_policy.load_balancing,
        aws_iam_policy.network, aws_iam_policy.ec2, aws_iam_policy.storage,
        aws_iam_policy.encryption, aws_iam_policy.rds, aws_iam_policy.data_services,
        aws_iam_policy.operations, aws_iam_policy.dns[0], aws_iam_policy.account_controls[0]
      ] : length(policy.policy) <= 6144]) &&
      length(aws_ssoadmin_permission_set_inline_policy.provisioner.inline_policy) <= 32768
    )
    error_message = "All optional capabilities must remain within AWS managed-policy size and attachment limits."
  }
  assert {
    condition = one([
      for statement in jsondecode(aws_iam_policy.dns[0].policy).Statement :
      statement.Resource == "arn:aws:route53:::hostedzone/Z123456789ABC" &&
      toset(statement.Condition["ForAllValues:StringLike"]["route53:ChangeResourceRecordSetsNormalizedRecordNames"]) == toset(["preview.example.com", "*.preview.example.com"])
      if statement.Sid == "ChangeOnlyApplicationRecords"
    ])
    error_message = "DNS changes must stay within the selected zone and application's domain names."
  }
  assert {
    condition = one([
      for statement in jsondecode(aws_iam_policy.dns[0].policy).Statement :
      statement.Effect == "Deny" && statement.Condition.StringEquals["acm:Export"] == "ENABLED"
      if statement.Sid == "NoPaidExportableCertificates"
    ])
    error_message = "Access must not enable billable exportable certificates."
  }
  assert {
    condition = one([
      for statement in jsondecode(aws_iam_policy.account_controls[0].policy).Statement :
      length(setsubtract(
        toset(["Project", "ManagedBy", "Purpose"]),
        toset(statement.Condition["ForAllValues:StringEquals"]["aws:TagKeys"])
      )) == 0 &&
      statement.Condition.StringEquals["aws:RequestTag/Project"] == var.name &&
      statement.Resource == ["arn:aws:cloudtrail:ap-southeast-1:123456789012:trail/abcdefghijklmnopqr-account"]
      if try(statement.Sid, "") == "TagProjectTrail"
    ])
    error_message = "The exact project trail must accept every bootstrap provider-default tag without allowing another project."
  }
}

run "dns_permissions_are_removed_when_disabled_again" {
  command = apply
  assert {
    condition     = length(aws_iam_policy.dns) == 0 && length(aws_iam_policy.account_controls) == 0 && length(aws_ssoadmin_customer_managed_policy_attachment.provisioner) == 8
    error_message = "Reapplying with optional permissions disabled must remove those policy attachments."
  }
}

run "zone_without_domain_is_rejected" {
  command = plan
  variables { hosted_zone_id = "Z123456789ABC" }
  expect_failures = [var.hosted_zone_id]
}

run "domain_without_zone_is_rejected" {
  command = plan
  variables { domain_name = "preview.example.com" }
  expect_failures = [var.hosted_zone_id]
}
