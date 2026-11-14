variable "aws_account_id" {
  type        = string
  description = "Expected AWS account; the provider refuses credentials for another account."
  validation {
    condition     = can(regex("^[0-9]{12}$", var.aws_account_id))
    error_message = "Supply the intended 12-digit AWS account ID."
  }
}

variable "aws_region" {
  type        = string
  description = "AWS region for all resources."
  default     = "us-east-1"
}

variable "name" {
  type        = string
  description = "Short lowercase resource prefix."
  default     = "backstage"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{2,19}$", var.name))
    error_message = "Use 3-20 lowercase letters, numbers, and hyphens; begin with a letter."
  }
}

variable "environment" {
  type        = string
  description = "Deployment environment label."
  default     = "production"
}

variable "tags" {
  type    = map(string)
  default = {}
}

variable "domain_name" {
  type        = string
  description = "Public portal hostname, for example backstage.example.com."
  validation {
    condition     = can(regex("^[a-z0-9][a-z0-9.-]+\\.[a-z]{2,}$", var.domain_name))
    error_message = "Supply a lowercase DNS hostname without a URL scheme or path."
  }
}

variable "route53_zone_id" {
  type        = string
  description = "Existing public Route53 hosted zone that owns domain_name."
}

variable "vpc_cidr" {
  type        = string
  description = "An IPv4 /16 VPC range; six /24 subnets are allocated."
  default     = "10.42.0.0/16"
  validation {
    condition     = can(cidrnetmask(var.vpc_cidr)) && endswith(var.vpc_cidr, "/16")
    error_message = "Supply a valid IPv4 /16 CIDR block."
  }
}

variable "single_nat_gateway" {
  type        = bool
  description = "Use one NAT gateway to lower cost; false gives each AZ its own NAT gateway."
  default     = false
}

variable "allowed_ingress_cidrs" {
  type        = list(string)
  description = "Explicit trusted corporate/VPN IPv4 ranges allowed to reach HTTPS. No world-open default; the app also requires authentication."
  validation {
    condition     = length(var.allowed_ingress_cidrs) > 0 && alltrue([for cidr in var.allowed_ingress_cidrs : can(cidrnetmask(cidr)) && try(tonumber(split("/", cidr)[1]), 0) >= 8])
    error_message = "Supply explicit IPv4 CIDR ranges with prefixes /8 through /32; world-open access is rejected."
  }
}

variable "image_digest" {
  type        = string
  description = "App image digest in this stack's ECR repository (sha256:...). Null creates infrastructure only; rollout is explicitly disabled until a digest is applied."
  default     = null
  validation {
    condition     = var.image_digest == null ? true : can(regex("^sha256:[a-f0-9]{64}$", var.image_digest))
    error_message = "Supply sha256: followed by exactly 64 lowercase hexadecimal digits."
  }
}

variable "portal_desired_count" {
  type        = number
  description = "Task count requested by the approved rollout workflow. Terraform initially creates the service at zero; use 2 for HA."
  default     = 2
  validation {
    condition     = contains([1, 2, 3, 4], var.portal_desired_count)
    error_message = "Choose a task count between one and four."
  }
}

variable "portal_cpu" {
  type        = number
  description = "Fargate CPU units; memory is fixed to 4096 MiB."
  default     = 1024
  validation {
    condition     = contains([512, 1024, 2048], var.portal_cpu)
    error_message = "CPU must be 512, 1024, or 2048 units for this 4096 MiB task."
  }
}

variable "postgres_engine_version" {
  type        = string
  description = "PostgreSQL major version. RDS selects a currently available minor; inspect the recorded actual version."
  default     = "16"
  validation {
    condition     = var.postgres_engine_version == "16"
    error_message = "This starter uses the postgres16 parameter family; update it alongside a major upgrade."
  }
}

variable "database_instance_class" {
  type        = string
  description = "RDS instance size; size against catalog/plugin connection counts."
  default     = "db.t4g.small"
}

variable "database_multi_az" {
  type        = bool
  default     = true
  description = "Enable RDS standby across AZs. False is suitable for a disposable evaluation."
}

variable "database_name" {
  type        = string
  default     = "backstage"
  description = "Application database. Backstage must use schema-based plugin division."
  validation {
    condition     = can(regex("^[a-z][a-z0-9_]{0,30}$", var.database_name))
    error_message = "Use 1-31 lowercase letters, digits, and underscores, beginning with a letter."
  }
}

variable "database_application_username" {
  type        = string
  default     = "backstage_app"
  description = "Separate least-privilege PostgreSQL application role, created by the one-shot bootstrap task."
  validation {
    condition     = can(regex("^[a-z][a-z0-9_]{0,30}$", var.database_application_username)) && var.database_application_username != "backstage_admin"
    error_message = "Use a lowercase SQL identifier distinct from backstage_admin."
  }
}

variable "deletion_protection" {
  type        = bool
  default     = true
  description = "Protect the ALB and database from accidental deletion. Disable explicitly before teardown."
}

variable "log_retention_days" {
  type        = number
  default     = 30
  description = "CloudWatch retention."
  validation {
    condition     = contains([7, 14, 30, 60, 90, 180, 365], var.log_retention_days)
    error_message = "Choose one of the supported retention values."
  }
}
