variable "application_secrets" {
  type = map(object({
    environment_variable = string
    json_key             = optional(string)
    description          = optional(string, "Backstage plugin credential")
  }))
  default = {}
  validation {
    condition = alltrue([for name, secret in var.application_secrets :
      can(regex("^[a-z][a-z0-9_-]{0,40}$", name)) &&
      can(regex("^[A-Z][A-Z0-9_]{0,80}$", secret.environment_variable)) &&
      !contains(["POSTGRES_USER", "POSTGRES_PASSWORD", "AUTH_GITHUB_CLIENT_ID", "AUTH_GITHUB_CLIENT_SECRET", "AUTH_SESSION_SECRET", "NODE_ENV", "NODE_OPTIONS", "AWS_REGION", "AWS_ROLLOUT_STATE_MACHINE_ARN", "APP_BASE_URL", "BACKEND_BASE_URL", "POSTGRES_HOST", "POSTGRES_PORT", "POSTGRES_DB", "PGSSLROOTCERT"], secret.environment_variable) &&
      (secret.json_key == null ? true : can(regex("^[A-Za-z0-9_-]{1,80}$", secret.json_key)))
    ]) && length(distinct([for secret in values(var.application_secrets) : secret.environment_variable])) == length(var.application_secrets)
    error_message = "Use unique nonreserved environment names and valid secret names/JSON keys."
  }
}
resource "aws_secretsmanager_secret" "application" {
  for_each                = var.application_secrets
  name                    = "${var.name}/${var.environment}/plugin/${each.key}"
  description             = each.value.description
  kms_key_id              = aws_kms_key.application.arn
  recovery_window_in_days = 30
}
output "application_secret_arns" {
  value = { for key, secret in aws_secretsmanager_secret.application : key => secret.arn }
}
