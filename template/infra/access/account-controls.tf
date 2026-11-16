# First-bootstrap denials with backstage-deploy confirmed these existing grants
# must be enabled and attached by the account owner (enable_account_controls=true):
# iam:UpdateAccountPasswordPolicy and s3:PutAccountPublicAccessBlock on "*";
# access-analyzer:TagResource on the exact project analyzer; and
# s3:CreateBucket on the exact project audit bucket. Do not widen other resources
# or fall back to an administrator profile. Disable these controls after bootstrap.
resource "aws_iam_policy" "account_controls" {
  count       = var.enable_account_controls ? 1 : 0
  name        = "${var.name}-provisioner-account-controls"
  description = "Temporary account baseline permissions; remove by setting enable_account_controls false after bootstrap."
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/account-controls.json.tftpl", {
    audit_bucket_arn = "arn:${data.aws_partition.current.partition}:s3:::${var.name}-${var.aws_account_id}-${var.aws_region}-audit"
    trail_arn        = "arn:${data.aws_partition.current.partition}:cloudtrail:${var.aws_region}:${var.aws_account_id}:trail/${var.name}-account"
    analyzer_arn     = "arn:${data.aws_partition.current.partition}:access-analyzer:${var.aws_region}:${var.aws_account_id}:analyzer/${var.name}-account"
    region           = var.aws_region
    project          = var.name
  })))
}
