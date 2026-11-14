resource "aws_rds_cluster_parameter_group" "db" {
  name_prefix = "${var.name}-"
  family      = "aurora-postgresql17"
  parameter {
    name         = "rds.force_ssl"
    value        = "1"
    apply_method = "immediate"
  }
}
resource "aws_rds_cluster" "db" {
  cluster_identifier              = var.name
  engine                          = "aurora-postgresql"
  engine_version                  = "17.7"
  database_name                   = "backstage"
  master_username                 = "backstage_admin"
  manage_master_user_password     = true
  storage_encrypted               = true
  db_subnet_group_name            = module.vpc.database_subnet_group_name
  vpc_security_group_ids          = [aws_security_group.db.id]
  db_cluster_parameter_group_name = aws_rds_cluster_parameter_group.db.name
  backup_retention_period         = 7
  deletion_protection             = true
  skip_final_snapshot             = false
  final_snapshot_identifier       = "${var.name}-final"
  enabled_cloudwatch_logs_exports = ["postgresql"]
  serverlessv2_scaling_configuration {
    min_capacity = 0.5
    max_capacity = 4
  }
}
resource "aws_rds_cluster_instance" "db" {
  count                = 2
  identifier           = "${var.name}-${count.index}"
  cluster_identifier   = aws_rds_cluster.db.id
  instance_class       = "db.serverless"
  engine               = aws_rds_cluster.db.engine
  engine_version       = aws_rds_cluster.db.engine_version
  publicly_accessible  = false
  db_subnet_group_name = module.vpc.database_subnet_group_name
}
