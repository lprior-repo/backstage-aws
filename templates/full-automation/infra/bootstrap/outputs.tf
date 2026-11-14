output "backend_config" {
  description = "Nonsecret backend settings for terraform init -backend-config=...; native S3 lockfiles are enabled."
  value = {
    bucket              = aws_s3_bucket.state.id
    key                 = var.state_key
    region              = var.aws_region
    encrypt             = true
    kms_key_id          = aws_kms_key.state.arn
    use_lockfile        = true
    allowed_account_ids = [var.aws_account_id]
  }
}

output "backend_hcl" {
  description = "Write this to a backend.hcl file outside version control, then pass it to core terraform init."
  value       = <<-EOT
    bucket       = "${aws_s3_bucket.state.id}"
    key          = "${var.state_key}"
    region       = "${var.aws_region}"
    encrypt      = true
    kms_key_id   = "${aws_kms_key.state.arn}"
    use_lockfile = true
    allowed_account_ids = ["${var.aws_account_id}"]
  EOT
}

output "state_bucket_arn" {
  value = aws_s3_bucket.state.arn
}

output "state_kms_key_arn" {
  value = aws_kms_key.state.arn
}
