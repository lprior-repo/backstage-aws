terraform {
  required_version = ">= 1.10, < 2.0"
  backend "s3" {}
  required_providers {
    aws = { source = "hashicorp/aws", version = "6.73.0" }
  }
}
provider "aws" {
  region = var.region
  default_tags { tags = { Project = var.name, ManagedBy = "terraform" } }
}
data "aws_availability_zones" "available" { state = "available" }
data "aws_caller_identity" "current" {}
data "aws_partition" "current" {}
