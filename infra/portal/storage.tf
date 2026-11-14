resource "aws_s3_bucket" "techdocs" {
  bucket_prefix = "${var.name}-techdocs-"
  lifecycle { prevent_destroy = true }
}
resource "aws_s3_bucket_public_access_block" "techdocs" {
  bucket                  = aws_s3_bucket.techdocs.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}
resource "aws_s3_bucket_versioning" "techdocs" {
  bucket = aws_s3_bucket.techdocs.id
  versioning_configuration { status = "Enabled" }
}
resource "aws_s3_bucket_server_side_encryption_configuration" "techdocs" {
  bucket = aws_s3_bucket.techdocs.id
  rule {
    apply_server_side_encryption_by_default { sse_algorithm = "AES256" }
  }
}
resource "aws_s3_bucket_policy" "techdocs" {
  bucket = aws_s3_bucket.techdocs.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Deny", Principal = "*", Action = "s3:*"
      Resource  = [aws_s3_bucket.techdocs.arn, "${aws_s3_bucket.techdocs.arn}/*"]
      Condition = { Bool = { "aws:SecureTransport" = "false" } }
    }]
  })
}
resource "aws_cloudwatch_log_group" "portal" {
  name              = "/ecs/${var.name}"
  retention_in_days = 30
}
