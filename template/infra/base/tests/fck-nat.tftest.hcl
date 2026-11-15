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
  mock_resource "aws_instance" {
    override_during = plan
    defaults = {
      id                           = "i-1234567890abcdef0"
      primary_network_interface_id = "eni-1234567890abcdef0"
      private_ip                   = "10.42.0.10"
      key_name                     = ""
      iam_instance_profile         = ""
    }
  }
  mock_resource "aws_route_table" {
    override_during = plan
    defaults        = { route = [] }
  }
}
mock_provider "random" {}

run "fck_nat_routes_private_subnets" {
  command = plan
  override_data {
    target = data.aws_availability_zones.available
    values = { names = ["us-east-1a", "us-east-1b"] }
  }
  override_data {
    target = data.aws_caller_identity.current
    values = { account_id = "123456789012", arn = "arn:aws:iam::123456789012:user/test", user_id = "AIDATEST" }
  }
  override_data {
    target = data.aws_partition.current
    values = { partition = "aws" }
  }
  override_data {
    target = data.aws_ami.fck_nat[0]
    values = { id = "ami-12345678" }
  }
  override_resource {
    target          = aws_db_instance.database
    override_during = plan
    values          = { master_user_secret = [{ secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:master" }] }
  }
  variables {
    aws_account_id        = "123456789012"
    aws_region            = "us-east-1"
    name                  = "backstage"
    environment           = "preview"
    domain_name           = "preview.example.com"
    route53_zone_id       = "Z123456789ABCDE"
    allowed_ingress_cidrs = ["192.0.2.0/24"]
    fck_nat               = true
  }
  assert {
    condition     = length(aws_instance.fck_nat) == 1 && aws_instance.fck_nat[0].instance_type == "t4g.nano" && aws_instance.fck_nat[0].source_dest_check == false && aws_instance.fck_nat[0].iam_instance_profile == "" && aws_instance.fck_nat[0].key_name == "" && aws_instance.fck_nat[0].credit_specification[0].cpu_credits == "standard"
    error_message = "fck-nat must be a single least-privilege instance with source/destination checking disabled and no SSH key or instance profile."
  }
  assert {
    condition     = alltrue([for route in aws_route.application : route.network_interface_id == aws_instance.fck_nat[0].primary_network_interface_id && route.gateway_id == null && route.nat_gateway_id == null]) && !output.assign_public_ip && output.db_admin_network_configuration.awsvpcConfiguration.assignPublicIp == "DISABLED"
    error_message = "Both application subnets must route through the shared NAT ENI and tasks must remain private."
  }
  assert {
    condition     = length(aws_vpc_security_group_ingress_rule.fck_nat_https) == 2 && alltrue([for rule in aws_vpc_security_group_ingress_rule.fck_nat_https : rule.from_port == 443 && rule.to_port == 443 && contains([for subnet in aws_subnet.application : subnet.cidr_block], rule.cidr_ipv4)]) && aws_vpc_security_group_egress_rule.fck_nat_https[0].cidr_ipv4 == "0.0.0.0/0" && aws_vpc_security_group_egress_rule.fck_nat_https[0].from_port == 443 && aws_vpc_security_group_egress_rule.fck_nat_https[0].to_port == 443
    error_message = "The NAT security group must allow only HTTPS from the two app subnet CIDRs and HTTPS outbound."
  }
  assert {
    condition     = length(aws_route_table.database.route) == 0 && aws_instance.fck_nat[0].metadata_options[0].http_tokens == "required" && aws_instance.fck_nat[0].metadata_options[0].http_put_response_hop_limit == 1 && aws_instance.fck_nat[0].metadata_options[0].instance_metadata_tags == "disabled" && aws_instance.fck_nat[0].root_block_device[0].encrypted && aws_instance.fck_nat[0].root_block_device[0].volume_type == "gp3" && aws_eip.fck_nat[0].instance == aws_instance.fck_nat[0].id
    error_message = "NAT must retain isolated database routes and use hardened IMDSv2, encrypted gp3 storage, and one attached EIP."
  }
  assert {
    condition     = toset(data.aws_ami.fck_nat[0].owners) == toset(["568608671756"]) && data.aws_ami.fck_nat[0].most_recent == false && output.fck_nat_ami_id == "ami-12345678" && output.fck_nat_private_ip == aws_instance.fck_nat[0].private_ip
    error_message = "The NAT image must be pinned to the official publisher and outputs must expose its selected AMI and private address."
  }
}

run "fck_nat_rejects_managed_nat_conflict" {
  command = plan
  variables {
    aws_account_id        = "123456789012"
    domain_name           = "preview.example.com"
    route53_zone_id       = "Z123456789ABCDE"
    allowed_ingress_cidrs = ["192.0.2.0/24"]
    fck_nat               = true
    nat_gateways          = 1
  }
  expect_failures = [var.nat_gateways]
}

run "fck_nat_rejects_invalid_ami" {
  command = plan
  variables {
    aws_account_id        = "123456789012"
    domain_name           = "preview.example.com"
    route53_zone_id       = "Z123456789ABCDE"
    allowed_ingress_cidrs = ["192.0.2.0/24"]
    fck_nat               = true
    fck_nat_ami_id        = "ami-ABCDEF12"
  }
  expect_failures = [var.fck_nat_ami_id]
}

run "ami_pin_requires_fck_nat" {
  command = plan
  variables {
    aws_account_id        = "123456789012"
    domain_name           = "preview.example.com"
    route53_zone_id       = "Z123456789ABCDE"
    allowed_ingress_cidrs = ["192.0.2.0/24"]
    fck_nat_ami_id        = "ami-12345678"
  }
  expect_failures = [var.fck_nat_ami_id]
}

run "managed_nat_preserves_per_az_private_egress" {
  command = plan
  override_resource {
    target          = aws_nat_gateway.main[0]
    override_during = plan
    values          = { id = "nat-00000000000000001" }
  }
  override_resource {
    target          = aws_nat_gateway.main[1]
    override_during = plan
    values          = { id = "nat-00000000000000002" }
  }
  override_resource {
    target          = aws_db_instance.database
    override_during = plan
    values          = { master_user_secret = [{ secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:master" }] }
  }
  variables {
    aws_account_id        = "123456789012"
    domain_name           = "preview.example.com"
    route53_zone_id       = "Z123456789ABCDE"
    allowed_ingress_cidrs = ["192.0.2.0/24"]
    nat_gateways          = 2
  }
  assert {
    condition     = length(aws_instance.fck_nat) == 0 && length(aws_nat_gateway.main) == 2 && !output.assign_public_ip && output.db_admin_network_configuration.awsvpcConfiguration.assignPublicIp == "DISABLED" && alltrue([for index, route in aws_route.application : route.nat_gateway_id == aws_nat_gateway.main[index].id])
    error_message = "Managed NAT must retain per-AZ egress and private task addresses without creating fck-nat."
  }
}
