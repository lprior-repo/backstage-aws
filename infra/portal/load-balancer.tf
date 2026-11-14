resource "aws_acm_certificate" "portal" {
  domain_name       = var.hostname
  validation_method = "DNS"
  lifecycle { create_before_destroy = true }
}
resource "aws_route53_record" "validation" {
  zone_id = var.route53_zone_id
  name    = one(aws_acm_certificate.portal.domain_validation_options).resource_record_name
  type    = one(aws_acm_certificate.portal.domain_validation_options).resource_record_type
  records = [one(aws_acm_certificate.portal.domain_validation_options).resource_record_value]
  ttl     = 60
}
resource "aws_acm_certificate_validation" "portal" {
  certificate_arn         = aws_acm_certificate.portal.arn
  validation_record_fqdns = [aws_route53_record.validation.fqdn]
}
resource "aws_lb" "portal" {
  name                       = var.name
  internal                   = false
  load_balancer_type         = "application"
  subnets                    = module.vpc.public_subnets
  security_groups            = [aws_security_group.alb.id]
  enable_deletion_protection = true
  drop_invalid_header_fields = true
}
resource "aws_lb_target_group" "portal" {
  name        = var.name
  port        = 7007
  protocol    = "HTTP"
  target_type = "ip"
  vpc_id      = module.vpc.vpc_id
  health_check {
    path                = "/.backstage/health/v1/readiness"
    matcher             = "200"
    healthy_threshold   = 2
    unhealthy_threshold = 3
  }
}
resource "aws_lb_listener" "https" {
  load_balancer_arn = aws_lb.portal.arn
  port              = 443
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS13-1-2-2021-06"
  certificate_arn   = aws_acm_certificate_validation.portal.certificate_arn
  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.portal.arn
  }
}
resource "aws_route53_record" "portal" {
  zone_id = var.route53_zone_id
  name    = var.hostname
  type    = "A"
  alias {
    name                   = aws_lb.portal.dns_name
    zone_id                = aws_lb.portal.zone_id
    evaluate_target_health = true
  }
}
