variable "name" {
  type    = string
  default = "backstage"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{2,22}$", var.name))
    error_message = "Use 3-23 lowercase letters, digits or hyphens, starting with a letter."
  }
}
variable "region" { type = string }
variable "service_count" {
  type        = number
  default     = 0
  description = "Keep at 0 until the database bootstrap task succeeds, then set to 2."
  validation {
    condition     = contains([0, 2], var.service_count)
    error_message = "Use 0 during bootstrap or 2 for the running service."
  }
}
variable "hostname" { type = string }
variable "route53_zone_id" { type = string }
variable "image" {
  type = string
  validation {
    condition     = can(regex("^[0-9]{12}\\.dkr\\.ecr\\.[a-z0-9-]+\\.amazonaws\\.com/[a-z0-9/_-]+@sha256:[a-f0-9]{64}$", var.image))
    error_message = "Use an immutable private ECR image URI with a sha256 digest."
  }
}
variable "app_secret_arn" {
  type        = string
  description = "Existing Secrets Manager JSON: github_client_id, github_client_secret, github_token. Values never enter Terraform."
}
variable "github_repository" {
  type    = string
  default = "lprior-repo/backstage-aws"
}
variable "github_oidc_provider_arn" {
  type        = string
  description = "Existing GitHub Actions OIDC provider ARN with audience sts.amazonaws.com."
}
variable "allowed_cidrs" {
  type        = list(string)
  description = "IPv4 networks allowed to reach HTTPS."
  validation {
    condition     = length(var.allowed_cidrs) > 0 && alltrue([for cidr in var.allowed_cidrs : can(cidrnetmask(cidr))])
    error_message = "Supply at least one valid IPv4 CIDR."
  }
}
