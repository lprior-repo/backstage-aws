# Discovery gap observed with backstage-deploy: route53:ListHostedZones is denied.
# Optional owner-reviewed addition to AccountDiscovery (Resource = "*"):
# "route53:ListHostedZones". This lists zone metadata only; it grants no DNS writes.
# Do not expand the zone-scoped mutation policy or create a placeholder zone.
resource "aws_iam_policy" "dns" {
  count       = local.has_dns ? 1 : 0
  name        = "${var.name}-provisioner-dns"
  description = "Optional DNS and certificate management limited to the supplied hosted zone and domain."
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/dns.json.tftpl", {
    hosted_zone_arn = "arn:${data.aws_partition.current.partition}:route53:::hostedzone/${var.hosted_zone_id}"
    change_arn      = "arn:${data.aws_partition.current.partition}:route53:::change/*"
    certificate_arn = "arn:${data.aws_partition.current.partition}:acm:${var.aws_region}:${var.aws_account_id}:certificate/*"
    domain          = var.domain_name
    wildcard_domain = "*.${var.domain_name}"
    region          = var.aws_region
    project         = var.name
  })))
}
