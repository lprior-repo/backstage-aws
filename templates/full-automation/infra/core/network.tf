resource "aws_vpc" "main" {
  cidr_block           = var.vpc_cidr
  enable_dns_hostnames = true
  enable_dns_support   = true
  tags                 = { Name = "${var.name}-vpc" }
}

resource "aws_internet_gateway" "main" {
  vpc_id = aws_vpc.main.id
}

resource "aws_subnet" "public" {
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(var.vpc_cidr, 8, count.index)
  availability_zone       = local.azs[count.index]
  map_public_ip_on_launch = false
  tags                    = { Name = "${var.name}-public-${count.index + 1}" }
}

resource "aws_subnet" "application" {
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(var.vpc_cidr, 8, 10 + count.index)
  availability_zone       = local.azs[count.index]
  map_public_ip_on_launch = false
  tags                    = { Name = "${var.name}-application-${count.index + 1}" }
}

resource "aws_subnet" "database" {
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(var.vpc_cidr, 8, 20 + count.index)
  availability_zone       = local.azs[count.index]
  map_public_ip_on_launch = false
  tags                    = { Name = "${var.name}-database-${count.index + 1}" }
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.main.id
}

resource "aws_route" "public_internet" {
  route_table_id         = aws_route_table.public.id
  destination_cidr_block = "0.0.0.0/0"
  gateway_id             = aws_internet_gateway.main.id
}

resource "aws_route_table_association" "public" {
  count          = 2
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}

resource "aws_eip" "nat" {
  count  = var.single_nat_gateway ? 1 : 2
  domain = "vpc"
}

resource "aws_nat_gateway" "main" {
  count         = var.single_nat_gateway ? 1 : 2
  allocation_id = aws_eip.nat[count.index].id
  subnet_id     = aws_subnet.public[count.index].id
  depends_on    = [aws_internet_gateway.main]
}

resource "aws_route_table" "application" {
  count  = 2
  vpc_id = aws_vpc.main.id
}

resource "aws_route" "application_egress" {
  count                  = 2
  route_table_id         = aws_route_table.application[count.index].id
  destination_cidr_block = "0.0.0.0/0"
  nat_gateway_id         = aws_nat_gateway.main[var.single_nat_gateway ? 0 : count.index].id
}

resource "aws_route_table_association" "application" {
  count          = 2
  subnet_id      = aws_subnet.application[count.index].id
  route_table_id = aws_route_table.application[count.index].id
}

# Database subnets deliberately have no default internet/NAT route.
resource "aws_route_table" "database" {
  vpc_id = aws_vpc.main.id
}

resource "aws_route_table_association" "database" {
  count          = 2
  subnet_id      = aws_subnet.database[count.index].id
  route_table_id = aws_route_table.database.id
}

resource "aws_vpc_endpoint" "s3" {
  vpc_id            = aws_vpc.main.id
  service_name      = "com.amazonaws.${var.aws_region}.s3"
  vpc_endpoint_type = "Gateway"
  route_table_ids   = aws_route_table.application[*].id
}

resource "aws_security_group" "alb" {
  name_prefix = "${var.name}-alb-"
  description = "Public HTTPS ingress; app traffic only to portal tasks"
  vpc_id      = aws_vpc.main.id
}

resource "aws_security_group" "portal" {
  name_prefix = "${var.name}-portal-"
  description = "Backstage ingress only from ALB"
  vpc_id      = aws_vpc.main.id
}

resource "aws_security_group" "bootstrap" {
  name_prefix = "${var.name}-db-init-"
  description = "One-shot database bootstrap; no inbound connections"
  vpc_id      = aws_vpc.main.id
}

resource "aws_security_group" "database" {
  name_prefix = "${var.name}-database-"
  description = "PostgreSQL accessible only from portal and bootstrap tasks"
  vpc_id      = aws_vpc.main.id
}

resource "aws_vpc_security_group_ingress_rule" "https" {
  for_each          = toset(var.allowed_ingress_cidrs)
  security_group_id = aws_security_group.alb.id
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
  cidr_ipv4         = each.value
}

resource "aws_vpc_security_group_ingress_rule" "http_redirect" {
  for_each          = toset(var.allowed_ingress_cidrs)
  security_group_id = aws_security_group.alb.id
  ip_protocol       = "tcp"
  from_port         = 80
  to_port           = 80
  cidr_ipv4         = each.value
}

resource "aws_vpc_security_group_egress_rule" "alb_to_portal" {
  security_group_id            = aws_security_group.alb.id
  referenced_security_group_id = aws_security_group.portal.id
  ip_protocol                  = "tcp"
  from_port                    = 7007
  to_port                      = 7007
}

resource "aws_vpc_security_group_ingress_rule" "portal_from_alb" {
  security_group_id            = aws_security_group.portal.id
  referenced_security_group_id = aws_security_group.alb.id
  ip_protocol                  = "tcp"
  from_port                    = 7007
  to_port                      = 7007
}

resource "aws_vpc_security_group_egress_rule" "portal_https" {
  security_group_id = aws_security_group.portal.id
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
  cidr_ipv4         = "0.0.0.0/0"
}

resource "aws_vpc_security_group_egress_rule" "bootstrap_https" {
  security_group_id = aws_security_group.bootstrap.id
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
  cidr_ipv4         = "0.0.0.0/0"
}

resource "aws_vpc_security_group_egress_rule" "portal_to_database" {
  security_group_id            = aws_security_group.portal.id
  referenced_security_group_id = aws_security_group.database.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}

resource "aws_vpc_security_group_egress_rule" "bootstrap_to_database" {
  security_group_id            = aws_security_group.bootstrap.id
  referenced_security_group_id = aws_security_group.database.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}

resource "aws_vpc_security_group_ingress_rule" "database_from_portal" {
  security_group_id            = aws_security_group.database.id
  referenced_security_group_id = aws_security_group.portal.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}

resource "aws_vpc_security_group_ingress_rule" "database_from_bootstrap" {
  security_group_id            = aws_security_group.database.id
  referenced_security_group_id = aws_security_group.bootstrap.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}
