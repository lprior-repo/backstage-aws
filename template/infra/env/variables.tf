variable "aws_account_id" {
  type = string
  validation {
    condition     = can(regex("^[0-9]{12}$", var.aws_account_id))
    error_message = "AWS account ID must contain 12 digits."
  }
}

variable "aws_region" {
  type = string
  validation {
    condition     = can(regex("^[a-z]{2}-[a-z]+-[0-9]+$", var.aws_region))
    error_message = "Invalid AWS region."
  }
}

variable "name" {
  type = string
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{2,19}$", var.name))
    error_message = "Invalid resource name."
  }
}

variable "namespace" {
  type = string
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{0,30}$", var.namespace)) && !endswith(var.namespace, "-") && !contains(["seed", "postgres", "template0", "template1"], var.namespace)
    error_message = "Invalid or reserved namespace."
  }
}

variable "hostname" {
  type = string
  validation {
    condition     = length(var.hostname) <= 253 && can(regex("^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*$", var.hostname)) && (var.hostname == var.domain_name || (endswith(var.hostname, ".${var.domain_name}") && length(split(".", var.hostname)) == length(split(".", var.domain_name)) + 1))
    error_message = "Hostname must equal the apex or have exactly one extra label."
  }
}

variable "domain_name" {
  type = string
  validation {
    condition     = length(var.domain_name) <= 253 && can(regex("^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$", var.domain_name))
    error_message = "Invalid lowercase DNS name."
  }
}

variable "image_digest" {
  type = string
  validation {
    condition     = can(regex("^sha256:[a-f0-9]{64}$", var.image_digest))
    error_message = "Image must be pinned by a SHA256 digest."
  }
}

variable "desired_count" {
  type    = number
  default = 1
  validation {
    condition     = var.desired_count >= 1 && var.desired_count <= 4 && floor(var.desired_count) == var.desired_count
    error_message = "Desired count must be an integer from 1 to 4."
  }
}

variable "auth_mode" {
  type    = string
  default = "guest"
  validation {
    condition     = contains(["guest", "github"], var.auth_mode)
    error_message = "Auth mode must be guest or github."
  }
}

variable "database_name" {
  type = string
  validation {
    condition     = can(regex("^[a-z][a-z0-9_]{0,30}$", var.database_name)) && !contains(["seed", "postgres", "template0", "template1"], var.database_name)
    error_message = "Invalid or reserved database identifier."
  }
}

variable "application_secrets" {
  type = map(object({
    environment_variable = string
    json_key             = optional(string)
  }))
  default = {}
  validation {
    condition = alltrue([
      for key, secret in var.application_secrets :
      can(regex("^[a-z][a-z0-9_-]{0,40}$", key)) &&
      can(regex("^[A-Z][A-Z0-9_]{0,80}$", secret.environment_variable)) &&
      !startswith(secret.environment_variable, "AWS_") &&
      !startswith(secret.environment_variable, "POSTGRES_") &&
      !startswith(secret.environment_variable, "AUTH_") &&
      !startswith(secret.environment_variable, "NODE_") &&
      !startswith(secret.environment_variable, "APP_") &&
      !startswith(secret.environment_variable, "BACKEND_") &&
      !startswith(secret.environment_variable, "PGSSL") &&
      (secret.json_key == null || can(regex("^[A-Za-z0-9_-]{1,80}$", secret.json_key)))
    ]) && length(distinct([for secret in values(var.application_secrets) : secret.environment_variable])) == length(var.application_secrets)
    error_message = "Application secrets must use valid unique, nonreserved environment names and valid keys."
  }
}

variable "state_bucket_name" {
  type = string
}
