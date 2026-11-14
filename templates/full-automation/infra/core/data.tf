data "aws_caller_identity" "current" {}
data "aws_partition" "current" {}
data "aws_availability_zones" "available" {
  state = "available"
}

locals {
  azs             = slice(data.aws_availability_zones.available.names, 0, 2)
  account_id      = data.aws_caller_identity.current.account_id
  partition       = data.aws_partition.current.partition
  portal_base_url = "https://${var.domain_name}"
  portal_image    = var.image_digest == null ? "${aws_ecr_repository.portal.repository_url}:bootstrap-not-pushed" : "${aws_ecr_repository.portal.repository_url}@${var.image_digest}"
  rotation_name   = "${var.name}-database-rotation"
  rotation_arn    = "arn:${local.partition}:states:${var.aws_region}:${local.account_id}:stateMachine:${local.rotation_name}"
  rollout_name    = "${var.name}-portal-rollout"
  rollout_arn     = "arn:${local.partition}:states:${var.aws_region}:${local.account_id}:stateMachine:${local.rollout_name}"
}
