mock_provider "aws" {
  mock_data "aws_availability_zones" {
    defaults = { names = ["us-east-1a", "us-east-1b"] }
  }
  mock_data "aws_caller_identity" {
    defaults = { account_id = "123456789012", arn = "arn:aws:sts::123456789012:assumed-role/test/session", user_id = "AROA:test" }
  }
  mock_data "aws_partition" {
    defaults = { partition = "aws" }
  }
}
mock_provider "random" {}

variables {
  aws_account_id        = "123456789012"
  aws_region            = "us-east-1"
  name                  = "backstage"
  environment           = "preview"
  domain_name           = ""
  route53_zone_id       = ""
  allowed_ingress_cidrs = ["192.0.2.0/24"]
}

run "domain_free_mode_serves_plain_http_to_the_allow_list_only" {
  command = plan
  override_resource {
    target          = aws_db_instance.database
    override_during = plan
    values          = { master_user_secret = [{ secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:master" }] }
  }
  assert {
    condition     = length(aws_acm_certificate.wildcard) == 0 && length(aws_route53_record.validation) == 0 && length(aws_route53_record.apex) == 0 && length(aws_route53_record.wildcard) == 0 && length(aws_lb_listener.https) == 0
    error_message = "Domain-free mode creates no certificate, DNS record or HTTPS listener."
  }
  assert {
    condition     = length(aws_vpc_security_group_ingress_rule.https) == 0 && toset([for rule in aws_vpc_security_group_ingress_rule.http : "${rule.cidr_ipv4}:${rule.from_port}-${rule.to_port}"]) == toset(["192.0.2.0/24:80-80"])
    error_message = "Only the allow-list on port 80 may reach the load balancer."
  }
  assert {
    condition     = aws_lb_listener.http.default_action[0].type == "fixed-response" && aws_lb_listener.http.default_action[0].fixed_response[0].status_code == "404"
    error_message = "The HTTP listener defaults to a fixed 404 without a domain."
  }
  assert {
    condition     = output.url_scheme == "http"
    error_message = "Domain-free mode reports the http scheme."
  }
}
