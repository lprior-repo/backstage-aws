variable "alert_email" {
  type        = string
  default     = null
  description = "SNS email requires the recipient to confirm the subscription."
}
resource "aws_dynamodb_table" "release_lock" {
  name         = "${var.name}-release-lock"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "LockId"
  attribute {
    name = "LockId"
    type = "S"
  }
  server_side_encryption {
    enabled     = true
    kms_key_arn = aws_kms_key.application.arn
  }
  point_in_time_recovery { enabled = true }
  deletion_protection_enabled = var.deletion_protection
}
resource "aws_sns_topic" "operations" {
  name = "${var.name}-operations"
}
resource "aws_sns_topic_subscription" "email" {
  count     = var.alert_email == null ? 0 : 1
  topic_arn = aws_sns_topic.operations.arn
  protocol  = "email"
  endpoint  = var.alert_email
}
resource "aws_cloudwatch_metric_alarm" "release_failure" {
  for_each            = toset(["ExecutionsFailed", "ExecutionsTimedOut", "ExecutionsAborted"])
  alarm_name          = "${var.name}-${each.key}"
  namespace           = "AWS/States"
  metric_name         = each.key
  statistic           = "Sum"
  period              = 60
  evaluation_periods  = 1
  threshold           = 0
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  dimensions          = { StateMachineArn = local.rollout_arn }
  alarm_actions       = [aws_sns_topic.operations.arn]
}
resource "aws_cloudwatch_metric_alarm" "unhealthy_targets" {
  alarm_name          = "${var.name}-unhealthy-targets"
  namespace           = "AWS/ApplicationELB"
  metric_name         = "UnHealthyHostCount"
  statistic           = "Maximum"
  period              = 60
  evaluation_periods  = 2
  threshold           = 0
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  dimensions = {
    LoadBalancer = aws_lb.portal.arn_suffix
    TargetGroup  = aws_lb_target_group.portal.arn_suffix
  }
  alarm_actions = [aws_sns_topic.operations.arn]
}
resource "aws_cloudwatch_metric_alarm" "alb_errors" {
  alarm_name          = "${var.name}-target-5xx"
  namespace           = "AWS/ApplicationELB"
  metric_name         = "HTTPCode_Target_5XX_Count"
  statistic           = "Sum"
  period              = 60
  evaluation_periods  = 2
  threshold           = 5
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  dimensions          = { LoadBalancer = aws_lb.portal.arn_suffix }
  alarm_actions       = [aws_sns_topic.operations.arn]
}
resource "aws_cloudwatch_metric_alarm" "database_storage" {
  alarm_name          = "${var.name}-database-storage"
  namespace           = "AWS/RDS"
  metric_name         = "FreeStorageSpace"
  statistic           = "Minimum"
  period              = 300
  evaluation_periods  = 2
  threshold           = 5368709120
  comparison_operator = "LessThanThreshold"
  treat_missing_data  = "missing"
  dimensions          = { DBInstanceIdentifier = aws_db_instance.database.identifier }
  alarm_actions       = [aws_sns_topic.operations.arn]
}
resource "aws_cloudwatch_dashboard" "operations" {
  dashboard_name = "${var.name}-operations"
  dashboard_body = jsonencode({ widgets = [
    { type = "metric", x = 0, y = 0, width = 12, height = 6, properties = {
      title = "ECS utilization", region = var.aws_region, view = "timeSeries", metrics = [
        ["AWS/ECS", "CPUUtilization", "ClusterName", aws_ecs_cluster.portal.name, "ServiceName", aws_ecs_service.portal.name],
        ["AWS/ECS", "MemoryUtilization", "ClusterName", aws_ecs_cluster.portal.name, "ServiceName", aws_ecs_service.portal.name]
    ] } },
    { type = "metric", x = 12, y = 0, width = 12, height = 6, properties = {
      title = "Release executions", region = var.aws_region, view = "timeSeries", stat = "Sum", metrics = [
        ["AWS/States", "ExecutionsFailed", "StateMachineArn", local.rollout_arn],
        ["AWS/States", "ExecutionsSucceeded", "StateMachineArn", local.rollout_arn]
    ] } }
  ] })
}
output "release_lock_table" { value = aws_dynamodb_table.release_lock.name }
output "database_identifier" { value = aws_db_instance.database.identifier }
output "target_group_arn" { value = aws_lb_target_group.portal.arn }
output "alb_arn" { value = aws_lb.portal.arn }
output "operations_topic_arn" { value = aws_sns_topic.operations.arn }
output "dashboard_name" { value = aws_cloudwatch_dashboard.operations.dashboard_name }
