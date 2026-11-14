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
  validation {
    condition     = can(regex("^[a-z]{2}-[a-z]+-[0-9]+$", var.aws_region))
    error_message = "Supply a valid AWS region."
  }
}

variable "name" {
  description = "Short lowercase deployment identifier."
  type        = string
  default     = "backstage"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{2,19}$", var.name))
    error_message = "Use 3-20 lowercase letters, numbers, and hyphens; begin with a letter."
  }
}

variable "state_bucket_name" {
  description = "Globally unique S3 bucket name. Null uses name-account-region-tfstate, which is what the CLI expects."
  type        = string
  default     = null
}
