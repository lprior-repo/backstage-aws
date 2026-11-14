terraform {
  required_version = "= 1.17.2"
  backend "s3" {}
  required_providers {
    aws    = { source = "hashicorp/aws", version = "6.73.0" }
    random = { source = "hashicorp/random", version = "3.7.2" }
  }
}
provider "aws" {
  region              = var.aws_region
  allowed_account_ids = [var.aws_account_id]
  default_tags { tags = { Project = var.name, Environment = var.environment, ManagedBy = "Terraform" } }
}
data "aws_caller_identity" "current" {}
data "aws_availability_zones" "available" { state = "available" }
