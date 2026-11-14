module "vpc" {
  source                 = "terraform-aws-modules/vpc/aws"
  version                = "6.7.3"
  name                   = var.name
  cidr                   = "10.42.0.0/16"
  azs                    = slice(data.aws_availability_zones.available.names, 0, 2)
  public_subnets         = ["10.42.0.0/24", "10.42.1.0/24"]
  private_subnets        = ["10.42.16.0/24", "10.42.17.0/24"]
  database_subnets       = ["10.42.32.0/24", "10.42.33.0/24"]
  enable_nat_gateway     = true
  single_nat_gateway     = false
  one_nat_gateway_per_az = true
  enable_dns_hostnames   = true
}
resource "aws_security_group" "alb" {
  name_prefix = "${var.name}-alb-"
  vpc_id      = module.vpc.vpc_id
}
resource "aws_security_group" "app" {
  name_prefix = "${var.name}-app-"
  vpc_id      = module.vpc.vpc_id
}
resource "aws_security_group" "db" {
  name_prefix = "${var.name}-db-"
  vpc_id      = module.vpc.vpc_id
}
resource "aws_vpc_security_group_ingress_rule" "https" {
  for_each          = toset(var.allowed_cidrs)
  security_group_id = aws_security_group.alb.id
  cidr_ipv4         = each.value
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
}
resource "aws_vpc_security_group_egress_rule" "alb_app" {
  security_group_id            = aws_security_group.alb.id
  referenced_security_group_id = aws_security_group.app.id
  ip_protocol                  = "tcp"
  from_port                    = 7007
  to_port                      = 7007
}
resource "aws_vpc_security_group_ingress_rule" "app" {
  security_group_id            = aws_security_group.app.id
  referenced_security_group_id = aws_security_group.alb.id
  ip_protocol                  = "tcp"
  from_port                    = 7007
  to_port                      = 7007
}
resource "aws_vpc_security_group_egress_rule" "app_https" {
  security_group_id = aws_security_group.app.id
  cidr_ipv4         = "0.0.0.0/0"
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
}
resource "aws_vpc_security_group_egress_rule" "app_db" {
  security_group_id            = aws_security_group.app.id
  referenced_security_group_id = aws_security_group.db.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}
resource "aws_vpc_security_group_ingress_rule" "db" {
  security_group_id            = aws_security_group.db.id
  referenced_security_group_id = aws_security_group.app.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}
