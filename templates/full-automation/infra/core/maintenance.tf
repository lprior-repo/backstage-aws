# Maintenance is not reachable through the portal task's StartExecution policy.
# It reuses the same fixed JSONata states and release mutex but accepts only the
# explicit rotation operation. Operator/CI access is a separate trust boundary.
resource "aws_sfn_state_machine" "database_rotation" {
  name     = local.rotation_name
  role_arn = aws_iam_role.rollout.arn
  type     = "STANDARD"
  publish  = true
  logging_configuration {
    log_destination        = "${aws_cloudwatch_log_group.workflow.arn}:*"
    include_execution_data = false
    level                  = "ERROR"
  }
  definition = jsonencode(merge(jsondecode(aws_sfn_state_machine.rollout.definition), {
    Comment = "Explicit database credential maintenance; JSONata-only, fixed tasks, serialized release lock."
    States = merge(jsondecode(aws_sfn_state_machine.rollout.definition).States, {
      ValidateRequest = {
        Type = "Choice"
        Choices = [{
          Condition = "{% $type($states.input.requestId) = 'string' ? ($exists($match($states.input.requestId, /^[A-Za-z0-9_-]{1,64}$/)) and $states.input.operation = 'rotateDatabase') : false %}"
          Next      = var.image_digest == null ? "ImageNotPublished" : "RememberRequest"
        }]
        Default = "InvalidRequest"
      }
    })
  }))
}
output "rotation_state_machine_arn" { value = aws_sfn_state_machine.database_rotation.arn }
resource "aws_cloudwatch_metric_alarm" "rotation_failure" {
  for_each            = toset(["ExecutionsFailed", "ExecutionsTimedOut", "ExecutionsAborted"])
  alarm_name          = "${var.name}-rotation-${each.key}"
  namespace           = "AWS/States"
  metric_name         = each.key
  statistic           = "Sum"
  period              = 60
  evaluation_periods  = 1
  threshold           = 0
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  dimensions          = { StateMachineArn = local.rotation_arn }
  alarm_actions       = [aws_sns_topic.operations.arn]
}
output "rotation_definition" {
  description = "Rendered JSONata maintenance definition for AWS validation. Contains no secret values."
  value       = aws_sfn_state_machine.database_rotation.definition
}
