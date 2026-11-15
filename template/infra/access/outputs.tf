output "provisioner_permission_set_arn" {
  description = "Permission set for the project-scoped provisioner."
  value       = aws_ssoadmin_permission_set.provisioner.arn
}

output "workload_boundary_arn" {
  description = "Owner-controlled boundary required by the bootstrap and base workload roles."
  value       = aws_iam_policy.workload_boundary.arn
}

output "account_controls_reminder" {
  description = "Disable temporary account-control permissions after the initial account baseline."
  value       = "Never grant the provisioner control of this access configuration or its owner-managed policies."
}
