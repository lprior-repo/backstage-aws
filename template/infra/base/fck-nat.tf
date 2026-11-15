data "aws_ami" "fck_nat" {
  count       = var.fck_nat ? 1 : 0
  owners      = ["568608671756"]
  most_recent = false

  filter {
    name   = "architecture"
    values = ["arm64"]
  }
  filter {
    name   = "root-device-type"
    values = ["ebs"]
  }
  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
  filter {
    name   = "state"
    values = ["available"]
  }
  filter {
    name   = "name"
    values = ["fck-nat-al2023-hvm-1.4.0-*-arm64-ebs"]
  }
  filter {
    name   = "image-id"
    values = var.fck_nat_ami_id == "" ? ["*"] : [var.fck_nat_ami_id]
  }
}

resource "aws_security_group" "fck_nat" {
  count  = var.fck_nat ? 1 : 0
  name   = "${var.name}-fck-nat"
  vpc_id = aws_vpc.main.id
}

resource "aws_vpc_security_group_ingress_rule" "fck_nat_https" {
  for_each          = var.fck_nat ? { for index, subnet in aws_subnet.application : tostring(index) => subnet } : {}
  security_group_id = aws_security_group.fck_nat[0].id
  cidr_ipv4         = each.value.cidr_block
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
}

resource "aws_vpc_security_group_egress_rule" "fck_nat_https" {
  count             = var.fck_nat ? 1 : 0
  security_group_id = aws_security_group.fck_nat[0].id
  cidr_ipv4         = "0.0.0.0/0"
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
}

resource "aws_instance" "fck_nat" {
  count                       = var.fck_nat ? 1 : 0
  ami                         = data.aws_ami.fck_nat[0].id
  instance_type               = "t4g.nano"
  subnet_id                   = aws_subnet.public[0].id
  vpc_security_group_ids      = [aws_security_group.fck_nat[0].id]
  source_dest_check           = false
  monitoring                  = false
  disable_api_termination     = false
  iam_instance_profile        = null
  associate_public_ip_address = false
  credit_specification {
    cpu_credits = "standard"
  }
  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
    instance_metadata_tags      = "disabled"
  }
  root_block_device {
    encrypted             = true
    volume_type           = "gp3"
    delete_on_termination = true
    tags                  = { Project = var.name }
  }
}

resource "aws_eip" "fck_nat" {
  count      = var.fck_nat ? 1 : 0
  domain     = "vpc"
  instance   = aws_instance.fck_nat[0].id
  depends_on = [aws_internet_gateway.main]
}
