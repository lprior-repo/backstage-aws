data "aws_caller_identity" "current" {}
data "aws_partition" "current" {}

locals {
  bucket_name = coalesce(var.state_bucket_name, "${var.name}-${data.aws_caller_identity.current.account_id}-${var.aws_region}-tfstate")
  bucket_arn  = "arn:${data.aws_partition.current.partition}:s3:::${local.bucket_name}"
}

resource "aws_kms_key" "state" {
  description             = "Encrypt ${var.name} Terraform remote state"
  enable_key_rotation     = true
  deletion_window_in_days = 30
  lifecycle {
    prevent_destroy = true
  }
}

resource "aws_kms_alias" "state" {
  name          = "alias/${var.name}-terraform-state"
  target_key_id = aws_kms_key.state.key_id
}

# The CLI owns the state bucket lifecycle; these blocks only forget old state entries
# and never delete the live bucket, objects or configuration.
removed {
  from = aws_s3_bucket.state
  lifecycle {
    destroy = false
  }
}

removed {
  from = aws_s3_bucket_versioning.state
  lifecycle {
    destroy = false
  }
}

removed {
  from = aws_s3_bucket_public_access_block.state
  lifecycle {
    destroy = false
  }
}

removed {
  from = aws_s3_bucket_ownership_controls.state
  lifecycle {
    destroy = false
  }
}

removed {
  from = aws_s3_bucket_server_side_encryption_configuration.state
  lifecycle {
    destroy = false
  }
}

removed {
  from = aws_s3_bucket_policy.state
  lifecycle {
    destroy = false
  }
}
