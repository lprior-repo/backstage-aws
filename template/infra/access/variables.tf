variable "name" {
  type        = string
  default     = "portal"
  description = "Project prefix shared with bootstrap and base."
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{1,17}$", var.name))
    error_message = "Use a 2-18 character lowercase project prefix."
  }
}

variable "aws_account_id" {
  type        = string
  description = "Existing account receiving the scoped permission set."
  validation {
    condition     = can(regex("^[0-9]{12}$", var.aws_account_id))
    error_message = "Supply the 12-digit deployment account ID."
  }
}

variable "aws_region" {
  type        = string
  default     = "us-east-1"
  description = "Region in which the provisioner may create regional deployment resources."
  validation {
    condition     = length(var.aws_region) <= 16 && can(regex("^[a-z]{2}(-[a-z0-9]+)+-[0-9]+$", var.aws_region))
    error_message = "Supply an AWS deployment region."
  }
}

variable "identity_center_region" {
  type        = string
  default     = "us-east-1"
  description = "Region hosting the existing IAM Identity Center instance."
  validation {
    condition     = length(var.identity_center_region) <= 16 && can(regex("^[a-z]{2}(-[a-z0-9]+)+-[0-9]+$", var.identity_center_region))
    error_message = "Supply the existing Identity Center region."
  }
}

variable "identity_center_instance_arn" {
  type        = string
  description = "Existing IAM Identity Center instance ARN; this stack does not enable Identity Center."
  validation {
    condition     = can(regex("^arn:[^:]+:sso:::instance/ssoins-[a-f0-9-]+$", var.identity_center_instance_arn))
    error_message = "Supply an existing Identity Center instance ARN."
  }
}

variable "principal_id" {
  type        = string
  description = "Existing Identity Center user ID, not a group ID."
  validation {
    condition     = can(regex("^[a-f0-9-]{16,64}$", var.principal_id))
    error_message = "Supply an existing Identity Center user ID."
  }
}

variable "hosted_zone_id" {
  type        = string
  default     = ""
  description = "Optional existing hosted zone ID; DNS permissions require both DNS inputs."
  validation {
    condition     = can(regex("^(Z[A-Z0-9]{1,31})?$", var.hosted_zone_id)) && ((var.hosted_zone_id == "") == (var.domain_name == ""))
    error_message = "Supply both a hosted zone ID and domain name, or leave both empty."
  }
}

variable "domain_name" {
  type        = string
  default     = ""
  description = "Optional domain name; leave empty until the public hosted zone exists."
  validation {
    condition     = var.domain_name == "" || (length(var.domain_name) <= 253 && can(regex("^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\\.)+[a-z0-9-]{2,63}$", var.domain_name)))
    error_message = "Supply a lowercase DNS domain name, or leave it empty."
  }
}

variable "enable_account_controls" {
  type        = bool
  default     = false
  description = "Temporarily allow account hardening and the project audit baseline; disable after first bootstrap."
}
