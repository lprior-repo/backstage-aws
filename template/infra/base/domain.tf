# Everything here exists only when a domain and its Route 53 zone are set.
# Without them the stack serves plain HTTP from main.tf and creates none of it.
# Adding a domain later is a re-apply of this stack.

moved {
  from = aws_acm_certificate.wildcard
  to   = aws_acm_certificate.wildcard[0]
}

moved {
  from = aws_acm_certificate_validation.wildcard
  to   = aws_acm_certificate_validation.wildcard[0]
}

moved {
  from = aws_lb_listener.https
  to   = aws_lb_listener.https[0]
}

moved {
  from = aws_route53_record.apex
  to   = aws_route53_record.apex[0]
}

moved {
  from = aws_route53_record.wildcard
  to   = aws_route53_record.wildcard[0]
}

resource "aws_acm_certificate" "wildcard" {
  count                     = local.domain_enabled ? 1 : 0
  domain_name               = var.domain_name
  subject_alternative_names = ["*.${var.domain_name}"]
  validation_method         = "DNS"
  lifecycle {
    create_before_destroy = true
  }
}

# The apex and wildcard names validate with the same record, hence the overwrite.
resource "aws_route53_record" "validation" {
  for_each        = local.domain_enabled ? toset([var.domain_name, "*.${var.domain_name}"]) : toset([])
  allow_overwrite = true
  zone_id         = var.route53_zone_id
  name            = one([for option in aws_acm_certificate.wildcard[0].domain_validation_options : option.resource_record_name if option.domain_name == each.key])
  type            = one([for option in aws_acm_certificate.wildcard[0].domain_validation_options : option.resource_record_type if option.domain_name == each.key])
  records         = [one([for option in aws_acm_certificate.wildcard[0].domain_validation_options : option.resource_record_value if option.domain_name == each.key])]
  ttl             = 60
}

resource "aws_acm_certificate_validation" "wildcard" {
  count                   = local.domain_enabled ? 1 : 0
  certificate_arn         = aws_acm_certificate.wildcard[0].arn
  validation_record_fqdns = [for record in aws_route53_record.validation : record.fqdn]
}

resource "aws_lb_listener" "https" {
  count             = local.domain_enabled ? 1 : 0
  load_balancer_arn = aws_lb.main.arn
  port              = 443
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS13-1-2-2021-06"
  certificate_arn   = aws_acm_certificate_validation.wildcard[0].certificate_arn
  default_action {
    type = "fixed-response"
    fixed_response {
      content_type = "text/plain"
      message_body = "Not Found"
      status_code  = "404"
    }
  }
}

resource "aws_route53_record" "apex" {
  count   = local.domain_enabled ? 1 : 0
  zone_id = var.route53_zone_id
  name    = var.domain_name
  type    = "A"
  alias {
    name                   = aws_lb.main.dns_name
    zone_id                = aws_lb.main.zone_id
    evaluate_target_health = true
  }
}

resource "aws_route53_record" "wildcard" {
  count   = local.domain_enabled ? 1 : 0
  zone_id = var.route53_zone_id
  name    = "*.${var.domain_name}"
  type    = "A"
  alias {
    name                   = aws_lb.main.dns_name
    zone_id                = aws_lb.main.zone_id
    evaluate_target_health = true
  }
}
