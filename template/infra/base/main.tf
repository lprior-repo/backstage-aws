locals {
  account_id = data.aws_caller_identity.current.account_id
  # A domain and its Route 53 zone are set together or not at all. Without them
  # the load balancer serves plain HTTP on port 80 to the ingress allow-list.
  domain_enabled = var.domain_name != ""
  listener_arn   = local.domain_enabled ? aws_lb_listener.https[0].arn : aws_lb_listener.http.arn
  partition      = data.aws_partition.current.partition
  azs            = slice(data.aws_availability_zones.available.names, 0, 2)
}

data "aws_partition" "current" {}

resource "aws_kms_key" "app" {
  description             = "${var.name} shared application encryption"
  enable_key_rotation     = true
  deletion_window_in_days = 30
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/application-kms-key.json.tftpl", {
    root_principal_arn          = "arn:${local.partition}:iam::${local.account_id}:root"
    logs_service_principal      = "logs.${var.aws_region}.amazonaws.com"
    logs_encryption_context_arn = "arn:${local.partition}:logs:${var.aws_region}:${local.account_id}:log-group:/backstage/${var.name}/*"
  })))
}

resource "aws_kms_alias" "app" {
  name          = "alias/${var.name}-application"
  target_key_id = aws_kms_key.app.key_id
}

resource "aws_vpc" "main" {
  cidr_block           = "10.42.0.0/16"
  enable_dns_hostnames = true
  enable_dns_support   = true
}

resource "aws_internet_gateway" "main" {
  vpc_id = aws_vpc.main.id
}

resource "aws_subnet" "public" {
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(aws_vpc.main.cidr_block, 8, count.index)
  availability_zone       = local.azs[count.index]
  map_public_ip_on_launch = false
}

resource "aws_subnet" "application" {
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(aws_vpc.main.cidr_block, 8, 10 + count.index)
  availability_zone       = local.azs[count.index]
  map_public_ip_on_launch = false
}

resource "aws_subnet" "database" {
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(aws_vpc.main.cidr_block, 8, 20 + count.index)
  availability_zone       = local.azs[count.index]
  map_public_ip_on_launch = false
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.main.id
}

resource "aws_route" "public" {
  route_table_id         = aws_route_table.public.id
  destination_cidr_block = "0.0.0.0/0"
  gateway_id             = aws_internet_gateway.main.id
}

resource "aws_route_table_association" "public" {
  count          = 2
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}

# NAT gateways cost more than the rest of a small deployment. Without either
# NAT mode, tasks get public addresses; security groups still restrict access.
resource "aws_eip" "nat" {
  count  = var.nat_gateways
  domain = "vpc"
}

resource "aws_nat_gateway" "main" {
  count         = var.nat_gateways
  allocation_id = aws_eip.nat[count.index].id
  subnet_id     = aws_subnet.public[count.index].id
  depends_on    = [aws_internet_gateway.main]
}

resource "aws_route_table" "application" {
  count  = 2
  vpc_id = aws_vpc.main.id
}

resource "aws_route" "application" {
  count                  = 2
  route_table_id         = aws_route_table.application[count.index].id
  destination_cidr_block = "0.0.0.0/0"
  gateway_id             = var.nat_gateways == 0 && !var.fck_nat ? aws_internet_gateway.main.id : null
  nat_gateway_id         = var.nat_gateways == 0 || var.fck_nat ? null : aws_nat_gateway.main[count.index % var.nat_gateways].id
  network_interface_id   = var.fck_nat ? aws_instance.fck_nat[0].primary_network_interface_id : null
}

resource "aws_route_table_association" "application" {
  count          = 2
  subnet_id      = aws_subnet.application[count.index].id
  route_table_id = aws_route_table.application[count.index].id
}

resource "aws_route_table" "database" {
  vpc_id = aws_vpc.main.id
}

resource "aws_route_table_association" "database" {
  count          = 2
  subnet_id      = aws_subnet.database[count.index].id
  route_table_id = aws_route_table.database.id
}

resource "aws_security_group" "alb" {
  name   = "${var.name}-alb"
  vpc_id = aws_vpc.main.id
}

resource "aws_security_group" "app" {
  name   = "${var.name}-app"
  vpc_id = aws_vpc.main.id
}

resource "aws_security_group" "admin" {
  name   = "${var.name}-db-admin"
  vpc_id = aws_vpc.main.id
}

resource "aws_security_group" "database" {
  name   = "${var.name}-database"
  vpc_id = aws_vpc.main.id
}

resource "aws_vpc_security_group_ingress_rule" "https" {
  for_each          = local.domain_enabled ? toset(var.allowed_ingress_cidrs) : toset([])
  security_group_id = aws_security_group.alb.id
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
  cidr_ipv4         = each.value
}

resource "aws_vpc_security_group_ingress_rule" "http" {
  for_each          = toset(var.allowed_ingress_cidrs)
  security_group_id = aws_security_group.alb.id
  ip_protocol       = "tcp"
  from_port         = 80
  to_port           = 80
  cidr_ipv4         = each.value
}

resource "aws_vpc_security_group_ingress_rule" "app" {
  security_group_id            = aws_security_group.app.id
  referenced_security_group_id = aws_security_group.alb.id
  ip_protocol                  = "tcp"
  from_port                    = 7007
  to_port                      = 7007
}

resource "aws_vpc_security_group_ingress_rule" "db_admin" {
  security_group_id            = aws_security_group.database.id
  referenced_security_group_id = aws_security_group.admin.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}

resource "aws_vpc_security_group_ingress_rule" "db_app" {
  security_group_id            = aws_security_group.database.id
  referenced_security_group_id = aws_security_group.app.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}

# Terraform removes the allow-all egress rule AWS adds to a new security group,
# so every outbound path is listed here. The database group has none.
resource "aws_vpc_security_group_egress_rule" "alb_to_app" {
  security_group_id            = aws_security_group.alb.id
  referenced_security_group_id = aws_security_group.app.id
  ip_protocol                  = "tcp"
  from_port                    = 7007
  to_port                      = 7007
}

resource "aws_vpc_security_group_egress_rule" "to_database" {
  for_each                     = { app = aws_security_group.app.id, admin = aws_security_group.admin.id }
  security_group_id            = each.value
  referenced_security_group_id = aws_security_group.database.id
  ip_protocol                  = "tcp"
  from_port                    = 5432
  to_port                      = 5432
}

# Image pulls, Secrets Manager, CloudWatch Logs, S3, and the APIs Backstage
# integrates with, all over HTTPS through the NAT gateway.
resource "aws_vpc_security_group_egress_rule" "https" {
  for_each          = { app = aws_security_group.app.id, admin = aws_security_group.admin.id }
  security_group_id = each.value
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
  cidr_ipv4         = "0.0.0.0/0"
}

resource "aws_lb" "main" {
  name                       = "${var.name}-alb"
  internal                   = false
  load_balancer_type         = "application"
  security_groups            = [aws_security_group.alb.id]
  subnets                    = aws_subnet.public[*].id
  enable_deletion_protection = true
  drop_invalid_header_fields = true
  desync_mitigation_mode     = "strictest"
}

# Port 80 is the only listener without a domain: environments attach their
# host-header rules here and it serves plain HTTP to the ingress allow-list.
# With a domain it only redirects, and domain.tf adds the HTTPS listener.
resource "aws_lb_listener" "http" {
  load_balancer_arn = aws_lb.main.arn
  port              = 80
  protocol          = "HTTP"
  default_action {
    type = local.domain_enabled ? "redirect" : "fixed-response"
    dynamic "redirect" {
      for_each = local.domain_enabled ? [1] : []
      content {
        port        = "443"
        protocol    = "HTTPS"
        status_code = "HTTP_301"
      }
    }
    dynamic "fixed_response" {
      for_each = local.domain_enabled ? [] : [1]
      content {
        content_type = "text/plain"
        message_body = "Not Found"
        status_code  = "404"
      }
    }
  }
}

resource "aws_ecs_cluster" "main" {
  name = "${var.name}-cluster"
  # Container Insights bills per task and metric. The alarms in operations.tf
  # use the standard RDS and load balancer metrics.
  setting {
    name  = "containerInsights"
    value = "disabled"
  }
}

resource "aws_ecs_cluster_capacity_providers" "main" {
  cluster_name       = aws_ecs_cluster.main.name
  capacity_providers = ["FARGATE", "FARGATE_SPOT"]
}

# Image tags never move, except the BuildKit layer cache, which every build
# overwrites.
resource "aws_ecr_repository" "portal" {
  name                 = "${var.name}/portal"
  image_tag_mutability = "IMMUTABLE_WITH_EXCLUSION"
  image_tag_mutability_exclusion_filter {
    filter      = "buildcache"
    filter_type = "WILDCARD"
  }
  image_scanning_configuration {
    scan_on_push = true
  }
}

resource "aws_ecr_lifecycle_policy" "portal" {
  repository = aws_ecr_repository.portal.name
  policy = jsonencode({
    rules = [{
      rulePriority = 1
      description  = "Expire untagged images after 14 days"
      selection = {
        tagStatus   = "untagged"
        countType   = "sinceImagePushed"
        countUnit   = "days"
        countNumber = 14
      }
      action = { type = "expire" }
      }, {
      # Every build pushes a uniquely tagged image; the newest ones are the ones in use.
      rulePriority = 2
      description  = "Keep the 50 most recent images"
      selection = {
        tagStatus   = "any"
        countType   = "imageCountMoreThan"
        countNumber = 50
      }
      action = { type = "expire" }
    }]
  })
}

resource "aws_secretsmanager_secret" "app_db" {
  name                    = "${var.name}/database-application"
  kms_key_id              = aws_kms_key.app.arn
  recovery_window_in_days = 7
}

ephemeral "random_password" "app_db" {
  length           = 48
  special          = true
  override_special = "!#$%&*()-_=+[]{}<>:?"
}

resource "aws_secretsmanager_secret_version" "app_db" {
  secret_id                = aws_secretsmanager_secret.app_db.id
  secret_string_wo         = jsonencode({ username = var.database_application_username, password = ephemeral.random_password.app_db.result })
  secret_string_wo_version = 1
}

# Dedicated lifecycle DB login secret: ephemeral, write-only Terraform pattern.
# Used by the clone/drop task to connect as a CREATEDB role (not master).
resource "aws_secretsmanager_secret" "db_lifecycle" {
  name                    = "${var.name}/database-lifecycle"
  kms_key_id              = aws_kms_key.app.arn
  recovery_window_in_days = 7
}

ephemeral "random_password" "db_lifecycle" {
  length           = 48
  special          = true
  override_special = "!#$%&*()-_=+[]{}<>:?"
}

resource "aws_secretsmanager_secret_version" "db_lifecycle" {
  secret_id                = aws_secretsmanager_secret.db_lifecycle.id
  secret_string_wo         = jsonencode({ username = "backstage_lifecycle", password = ephemeral.random_password.db_lifecycle.result })
  secret_string_wo_version = 1
}

resource "aws_secretsmanager_secret" "session" {
  name                    = "${var.name}/session"
  kms_key_id              = aws_kms_key.app.arn
  recovery_window_in_days = 7
}

ephemeral "random_password" "session" {
  length           = 48
  special          = true
  override_special = "!#$%&*()-_=+[]{}<>:?"
}

resource "aws_secretsmanager_secret_version" "session" {
  secret_id                = aws_secretsmanager_secret.session.id
  secret_string_wo         = ephemeral.random_password.session.result
  secret_string_wo_version = 1
}

# GitHub OAuth credentials for environments that sign in with GitHub. The
# value is stored by `backstage-aws secrets set-identity`, never by Terraform.
resource "aws_secretsmanager_secret" "identity" {
  name                    = "${var.name}/identity"
  kms_key_id              = aws_kms_key.app.arn
  recovery_window_in_days = 7
}

# Plugin credentials, shared by every environment in the account so that a
# preview starts without anyone filling in secrets for it. Store each value
# once with `aws secretsmanager put-secret-value`.
resource "aws_secretsmanager_secret" "plugin" {
  for_each                = var.application_secrets
  name                    = "${var.name}/plugin/${each.key}"
  kms_key_id              = aws_kms_key.app.arn
  recovery_window_in_days = 7
}

resource "aws_s3_bucket" "seed" {
  bucket        = "${var.name}-${local.account_id}-${var.aws_region}-seed"
  force_destroy = false
}

resource "aws_s3_bucket_public_access_block" "seed" {
  bucket                  = aws_s3_bucket.seed.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_server_side_encryption_configuration" "seed" {
  bucket = aws_s3_bucket.seed.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm     = "aws:kms"
      kms_master_key_id = aws_kms_key.app.arn
    }
  }
}

resource "aws_s3_bucket_policy" "seed" {
  bucket = aws_s3_bucket.seed.id
  policy = jsonencode(jsondecode(templatefile("${path.module}/policies/seed-bucket.json.tftpl", {
    bucket_arns = [aws_s3_bucket.seed.arn, "${aws_s3_bucket.seed.arn}/*"]
  })))
}

resource "aws_db_subnet_group" "database" {
  name       = "${var.name}-database"
  subnet_ids = aws_subnet.database[*].id
}

resource "aws_db_parameter_group" "database" {
  name_prefix = "${var.name}-postgres16-"
  family      = "postgres16"
  parameter {
    name         = "rds.force_ssl"
    value        = "1"
    apply_method = "pending-reboot"
  }
  parameter {
    name         = "password_encryption"
    value        = "scram-sha-256"
    apply_method = "pending-reboot"
  }
  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_db_instance" "database" {
  identifier                    = "${var.name}-postgres"
  engine                        = "postgres"
  engine_version                = "16"
  instance_class                = var.database_instance_class
  storage_type                  = "gp3"
  allocated_storage             = 20
  max_allocated_storage         = 100
  username                      = "backstage_admin"
  manage_master_user_password   = true
  master_user_secret_kms_key_id = aws_kms_key.app.arn
  db_subnet_group_name          = aws_db_subnet_group.database.name
  parameter_group_name          = aws_db_parameter_group.database.name
  vpc_security_group_ids        = [aws_security_group.database.id]
  publicly_accessible           = false
  storage_encrypted             = true
  kms_key_id                    = aws_kms_key.app.arn
  multi_az                      = var.database_multi_az
  deletion_protection           = true
  backup_retention_period       = 7
  skip_final_snapshot           = false
  final_snapshot_identifier     = "${var.name}-final"
}
