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

run "state_key_and_bucket_name_are_stable" {
  command = plan
  assert {
    condition     = aws_kms_key.state.enable_key_rotation && aws_kms_alias.state.name == "alias/backstage-terraform-state"
    error_message = "The key that encrypts remote state must rotate and keep its alias."
  }
  assert {
    condition     = output.state_bucket_name == "backstage-123456789012-us-east-1-tfstate"
    error_message = "The CLI finds state by this bucket name; Terraform only reports it."
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
