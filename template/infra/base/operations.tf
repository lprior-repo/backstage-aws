resource "aws_sns_topic" "operations" {
  name              = "${var.name}-operations"
  kms_master_key_id = aws_kms_key.app.arn
}

resource "aws_sns_topic_subscription" "email" {
  count     = var.alert_email == null ? 0 : 1
  topic_arn = aws_sns_topic.operations.arn
  protocol  = "email"
  endpoint  = var.alert_email
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

resource "aws_cloudwatch_metric_alarm" "database_cpu" {
  alarm_name          = "${var.name}-database-cpu"
  namespace           = "AWS/RDS"
  metric_name         = "CPUUtilization"
  statistic           = "Average"
  period              = 300
  evaluation_periods  = 2
  threshold           = 80
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  dimensions          = { DBInstanceIdentifier = aws_db_instance.database.identifier }
  alarm_actions       = [aws_sns_topic.operations.arn]
}

resource "aws_cloudwatch_metric_alarm" "alb_5xx" {
  alarm_name          = "${var.name}-alb-5xx"
  namespace           = "AWS/ApplicationELB"
  metric_name         = "HTTPCode_ELB_5XX_Count"
  statistic           = "Sum"
  period              = 60
  evaluation_periods  = 2
  threshold           = 5
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  dimensions          = { LoadBalancer = aws_lb.main.arn_suffix }
  alarm_actions       = [aws_sns_topic.operations.arn]
}
