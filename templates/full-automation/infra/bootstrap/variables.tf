variable "aws_account_id" {
  type        = string
  description = "Expected AWS account; the provider refuses credentials for another account."
  validation {
    condition     = can(regex("^[0-9]{12}$", var.aws_account_id))
    error_message = "Supply the intended 12-digit AWS account ID."
  }
}

variable "aws_region" {
  description = "AWS region for the remote state bucket and encryption key."
  type        = string
  default     = "us-east-1"
}

variable "name" {
  description = "Short lowercase deployment identifier."
  type        = string
  default     = "backstage"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{2,23}$", var.name))
    error_message = "Use 3-24 lowercase letters, numbers, and hyphens; begin with a letter."
  }
}

variable "state_bucket_name" {
  description = "Globally unique S3 bucket name. Null uses name-account-region-tfstate."
  type        = string
  default     = null
}

variable "state_key" {
  description = "Object key for the core stack state. bootstrap/terraform.tfstate is reserved for the bootstrap stack itself."
  type        = string
  default     = "backstage/core.tfstate"
  validation {
    condition     = trimspace(var.state_key) != "" && var.state_key != "bootstrap/terraform.tfstate" && !endswith(var.state_key, ".tflock")
    error_message = "Choose a nonempty core state key distinct from reserved bootstrap/terraform.tfstate and lockfile keys ending in .tflock."
  }
}
