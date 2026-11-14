mock_provider "aws" {
  mock_data "aws_caller_identity" {
    defaults = { account_id = "123456789012" }
  }
  mock_data "aws_partition" {
    defaults = { partition = "aws" }
  }
}

variables {
  aws_account_id = "123456789012"
  aws_region     = "us-east-1"
  name           = "backstage"
}

run "remote_state_is_private_versioned_and_locked" {
  command = plan
  assert {
    condition     = aws_s3_bucket_public_access_block.state.block_public_acls && aws_s3_bucket_public_access_block.state.block_public_policy && aws_s3_bucket_public_access_block.state.ignore_public_acls && aws_s3_bucket_public_access_block.state.restrict_public_buckets
    error_message = "Remote state must block every public-access path."
  }
  assert {
    condition     = aws_s3_bucket_versioning.state.versioning_configuration[0].status == "Enabled"
    error_message = "Versioning is required for state recovery."
  }
  assert {
    condition     = one(aws_s3_bucket_server_side_encryption_configuration.state.rule).apply_server_side_encryption_by_default[0].sse_algorithm == "aws:kms"
    error_message = "State must use the customer-managed KMS key."
  }
  assert {
    condition     = aws_s3_bucket.state.bucket == "backstage-123456789012-us-east-1-tfstate" && aws_kms_key.state.enable_key_rotation
    error_message = "The CLI finds state by this bucket name; the key that encrypts it must rotate."
  }
}

run "new_account_baseline_is_explicit" {
  command = plan
  variables {
    enable_account_baseline = true
    budget_email            = "operator@example.com"
  }
  assert {
    condition     = aws_s3_account_public_access_block.account[0].block_public_policy && aws_s3_account_public_access_block.account[0].block_public_acls
    error_message = "A new account must block public S3 access."
  }
  assert {
    condition     = aws_cloudtrail.account[0].is_multi_region_trail && aws_cloudtrail.account[0].enable_log_file_validation && aws_cloudtrail.account[0].enable_logging
    error_message = "Account audit trail must be multi-region, active and validated."
  }
  assert {
    condition     = aws_accessanalyzer_analyzer.account[0].type == "ACCOUNT"
    error_message = "Account-level IAM access analysis must be created."
  }
}
