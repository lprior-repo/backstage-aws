terraform {
  required_version = "= 1.17.2"
  backend "s3" {}

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "6.73.0"
    }
  }
}

provider "aws" {
  region              = var.aws_region
  allowed_account_ids = [var.aws_account_id]
  default_tags {
    tags = merge(var.tags, {
      Project     = var.name
      Environment = var.environment
      ManagedBy   = "Terraform"
    })
  }
}
