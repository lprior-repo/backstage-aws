variable "enable_account_baseline" {
  type        = bool
  default     = true
  description = "Enable once in a new account. Import existing account-wide controls before enabling in an established account."
}
variable "budget_email" {
  type    = string
  default = null
  validation {
    condition     = var.budget_email == null || can(regex("^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$", var.budget_email))
    error_message = "Supply a valid budget email address."
  }
}
variable "monthly_budget_usd" {
  type    = number
  default = 300
  validation {
    condition     = var.monthly_budget_usd >= 1 && var.monthly_budget_usd <= 100000
    error_message = "Monthly budget must be between 1 and 100000 USD."
  }
}
resource "aws_s3_account_public_access_block" "account" {
  count                   = var.enable_account_baseline ? 1 : 0
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}
resource "aws_iam_account_password_policy" "account" {
  count                          = var.enable_account_baseline ? 1 : 0
  minimum_password_length        = 20
  require_lowercase_characters   = true
  require_numbers                = true
  require_uppercase_characters   = true
  require_symbols                = true
  allow_users_to_change_password = true
  password_reuse_prevention      = 24
}
resource "aws_accessanalyzer_analyzer" "account" {
  count         = var.enable_account_baseline ? 1 : 0
  analyzer_name = "${var.name}-account"
  type          = "ACCOUNT"
}
resource "aws_s3_bucket" "audit" {
  count         = var.enable_account_baseline ? 1 : 0
  bucket        = "${var.name}-${var.aws_account_id}-${var.aws_region}-audit"
  force_destroy = false
  lifecycle { prevent_destroy = true }
}
resource "aws_s3_bucket_public_access_block" "audit" {
  count                   = var.enable_account_baseline ? 1 : 0
  bucket                  = aws_s3_bucket.audit[0].id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}
resource "aws_s3_bucket_versioning" "audit" {
  count  = var.enable_account_baseline ? 1 : 0
  bucket = aws_s3_bucket.audit[0].id
  versioning_configuration { status = "Enabled" }
}
resource "aws_s3_bucket_server_side_encryption_configuration" "audit" {
  count  = var.enable_account_baseline ? 1 : 0
  bucket = aws_s3_bucket.audit[0].id
  rule {
    apply_server_side_encryption_by_default { sse_algorithm = "AES256" }
  }
}
resource "aws_s3_bucket_policy" "audit" {
  count  = var.enable_account_baseline ? 1 : 0
  bucket = aws_s3_bucket.audit[0].id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid       = "TrailACL", Effect = "Allow", Principal = { Service = "cloudtrail.amazonaws.com" },
        Action    = "s3:GetBucketAcl", Resource = aws_s3_bucket.audit[0].arn,
        Condition = { StringEquals = { "aws:SourceArn" = "arn:${data.aws_partition.current.partition}:cloudtrail:${var.aws_region}:${var.aws_account_id}:trail/${var.name}-account" } }
      },
      {
        Sid    = "TrailWrite", Effect = "Allow", Principal = { Service = "cloudtrail.amazonaws.com" },
        Action = "s3:PutObject", Resource = "${aws_s3_bucket.audit[0].arn}/AWSLogs/${var.aws_account_id}/*",
        Condition = { StringEquals = {
          "s3:x-amz-acl"  = "bucket-owner-full-control",
          "aws:SourceArn" = "arn:${data.aws_partition.current.partition}:cloudtrail:${var.aws_region}:${var.aws_account_id}:trail/${var.name}-account"
        } }
      },
      {
        Sid       = "TLSOnly", Effect = "Deny", Principal = "*", Action = "s3:*",
        Resource  = [aws_s3_bucket.audit[0].arn, "${aws_s3_bucket.audit[0].arn}/*"],
        Condition = { Bool = { "aws:SecureTransport" = "false" } }
      }
    ]
  })
}
resource "aws_cloudtrail" "account" {
  count                         = var.enable_account_baseline ? 1 : 0
  name                          = "${var.name}-account"
  s3_bucket_name                = aws_s3_bucket.audit[0].id
  is_multi_region_trail         = true
  include_global_service_events = true
  enable_log_file_validation    = true
  enable_logging                = true
  event_selector {
    read_write_type           = "All"
    include_management_events = true
  }
  depends_on = [aws_s3_bucket_policy.audit]
}
resource "aws_budgets_budget" "account" {
  count        = var.budget_email != null ? 1 : 0
  name         = "${var.name}-monthly"
  budget_type  = "COST"
  limit_amount = tostring(var.monthly_budget_usd)
  limit_unit   = "USD"
  time_unit    = "MONTHLY"
  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 80
    threshold_type             = "PERCENTAGE"
    notification_type          = "ACTUAL"
    subscriber_email_addresses = [var.budget_email]
  }
  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 100
    threshold_type             = "PERCENTAGE"
    notification_type          = "FORECASTED"
    subscriber_email_addresses = [var.budget_email]
  }
}
output "account_trail_arn" {
  value = try(aws_cloudtrail.account[0].arn, null)
}
