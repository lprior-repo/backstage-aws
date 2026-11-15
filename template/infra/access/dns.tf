resource "aws_iam_policy" "dns" {
  count       = local.has_dns ? 1 : 0
  name        = "${var.name}-provisioner-dns"
  description = "Optional DNS and certificate management limited to the supplied hosted zone and domain."
  policy = jsonencode({
    "Version" = "2012-10-17"
    "Statement" = [
      {
        "Sid"    = "ReadSelectedZone"
        "Effect" = "Allow"
        "Action" = [
          "route53:ListResourceRecordSets",
          "route53:GetHostedZone",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:route53:::hostedzone/${var.hosted_zone_id}"
      },
      {
        "Sid"      = "ChangeOnlyApplicationRecords"
        "Effect"   = "Allow"
        "Action"   = "route53:ChangeResourceRecordSets"
        "Resource" = "arn:${data.aws_partition.current.partition}:route53:::hostedzone/${var.hosted_zone_id}"
        "Condition" = {
          "ForAllValues:StringLike" = {
            "route53:ChangeResourceRecordSetsNormalizedRecordNames" = [
              var.domain_name,
              "*.${var.domain_name}",
            ]
          }
          "ForAllValues:StringEquals" = {
            "route53:ChangeResourceRecordSetsActions" = [
              "CREATE",
              "UPSERT",
              "DELETE",
            ]
            "route53:ChangeResourceRecordSetsRecordTypes" = [
              "A",
              "CNAME",
            ]
          }
        }
      },
      {
        "Sid"      = "ReadChangeStatus"
        "Effect"   = "Allow"
        "Action"   = "route53:GetChange"
        "Resource" = "arn:${data.aws_partition.current.partition}:route53:::change/*"
      },
      {
        "Sid"      = "RequestOnlyDnsValidatedApplicationCertificates"
        "Effect"   = "Allow"
        "Action"   = "acm:RequestCertificate"
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "aws:RequestedRegion"    = var.aws_region
            "aws:RequestTag/Project" = var.name
            "acm:ValidationMethod"   = "DNS"
          }
          "ForAllValues:StringEquals" = {
            "acm:DomainNames" = [
              var.domain_name,
              "*.${var.domain_name}",
            ]
          }
        }
      },
      {
        "Sid"      = "NoPaidExportableCertificates"
        "Effect"   = "Deny"
        "Action"   = "acm:RequestCertificate"
        "Resource" = "*"
        "Condition" = {
          "StringEquals" = {
            "acm:Export" = "ENABLED"
          }
        }
      },
      {
        "Sid"    = "MaintainOwnedCertificates"
        "Effect" = "Allow"
        "Action" = [
          "acm:DescribeCertificate",
          "acm:DeleteCertificate",
          "acm:ListTagsForCertificate",
          "acm:AddTagsToCertificate",
          "acm:RemoveTagsFromCertificate",
        ]
        "Resource" = "arn:${data.aws_partition.current.partition}:acm:${var.aws_region}:${var.aws_account_id}:certificate/*"
        "Condition" = {
          "StringEquals" = {
            "aws:ResourceTag/Project" = var.name
            "aws:RequestedRegion"     = var.aws_region
          }
        }
      },
    ]
  })
}
