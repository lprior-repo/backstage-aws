terraform {
  required_version = "= 1.17.2"

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
    tags = {
      Project   = var.name
      ManagedBy = "Terraform"
      Purpose   = "TerraformState"
    }
  }
}
