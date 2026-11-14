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
    tags = {
      Project = var.name
    }
  }
}

data "terraform_remote_state" "base" {
  backend = "s3"
  config = {
    bucket = var.state_bucket_name
    key    = "base/terraform.tfstate"
    region = var.aws_region
  }
}
