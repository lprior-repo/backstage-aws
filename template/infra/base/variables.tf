variable "aws_account_id" {
  type = string
  validation {
    condition     = can(regex("^[0-9]{12}$", var.aws_account_id))
    error_message = "AWS account ID must contain 12 digits."
  }
}

variable "aws_region" {
  type    = string
  default = "us-east-1"
  validation {
    condition     = can(regex("^[a-z]{2}-[a-z]+-[0-9]+$", var.aws_region))
    error_message = "Invalid AWS region."
  }
}

variable "name" {
  type    = string
  default = "backstage"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{2,19}$", var.name))
    error_message = "Invalid resource prefix."
  }
}

variable "environment" {
  type    = string
  default = "preview"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{1,19}$", var.environment))
    error_message = "Invalid environment."
  }
}

variable "domain_name" {
  type = string
  validation {
    condition     = length(var.domain_name) <= 253 && can(regex("^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$", var.domain_name))
    error_message = "Invalid lowercase DNS apex."
  }
}

variable "route53_zone_id" {
  type = string
  validation {
    condition     = can(regex("^Z[A-Z0-9]{5,32}$", var.route53_zone_id))
    error_message = "Invalid Route 53 zone ID."
  }
}

variable "allowed_ingress_cidrs" {
  type = list(string)
  validation {
    condition = length(var.allowed_ingress_cidrs) >= 1 && length(var.allowed_ingress_cidrs) <= 50 && alltrue([
      for cidr in var.allowed_ingress_cidrs : can(cidrnetmask(cidr)) && try(tonumber(split("/", cidr)[1]), 0) >= 8 && try(tonumber(split("/", cidr)[1]), 33) <= 32
    ])
    error_message = "Provide 1-50 IPv4 networks with prefixes /8 to /32."
  }
}

variable "single_nat_gateway" {
  type    = bool
  default = true
}

variable "database_multi_az" {
  type    = bool
  default = false
}

variable "alert_email" {
  type    = string
  default = null
  validation {
    condition     = var.alert_email == null || can(regex("^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$", var.alert_email))
    error_message = "Invalid alert email."
  }
}

variable "log_retention_days" {
  type    = number
  default = 30
  validation {
    condition     = contains([7, 14, 30, 60, 90, 180, 365], var.log_retention_days)
    error_message = "Unsupported log retention."
  }
}

variable "database_application_username" {
  type    = string
  default = "backstage_app"
  validation {
    condition     = can(regex("^[a-z][a-z0-9_]{0,30}$", var.database_application_username)) && var.database_application_username != "backstage_admin"
    error_message = "Invalid database role."
  }
}

variable "application_secrets" {
  type = map(object({
    environment_variable = string
    json_key             = optional(string)
  }))
  default = {}
  validation {
    condition     = alltrue([for name in keys(var.application_secrets) : can(regex("^[a-z][a-z0-9_-]{0,40}$", name))])
    error_message = "Application secret names are lowercase letters, digits, `_` and `-`."
  }
}
