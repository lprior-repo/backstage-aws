output "state_bucket_name" {
  value = local.bucket_name
}

output "state_kms_key_arn" {
  value = aws_kms_key.state.arn
}
