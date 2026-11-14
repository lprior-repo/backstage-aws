resource "aws_iam_role" "rollout" {
  name = "${var.name}-rollout"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "states.amazonaws.com" }
      Action    = "sts:AssumeRole"
      Condition = {
        StringEquals = { "aws:SourceAccount" = local.account_id }
        ArnEquals    = { "aws:SourceArn" = [local.rollout_arn, local.rotation_arn] }
      }
    }]
  })
}

resource "aws_iam_role_policy" "rollout" {
  role = aws_iam_role.rollout.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "RunOnlyApprovedDatabaseBootstrapRevision"
        Effect   = "Allow"
        Action   = "ecs:RunTask"
        Resource = [aws_ecs_task_definition.bootstrap.arn, aws_ecs_task_definition.rotate.arn]
        Condition = {
          ArnEquals = { "ecs:cluster" = aws_ecs_cluster.portal.arn }
        }
      },
      {
        Sid      = "ObserveAndCancelBootstrapTask"
        Effect   = "Allow"
        Action   = ["ecs:DescribeTasks", "ecs:StopTask"]
        Resource = "arn:${local.partition}:ecs:${var.aws_region}:${local.account_id}:task/${aws_ecs_cluster.portal.name}/*"
      },
      {
        Sid      = "DeployOnlyThisPortalService"
        Effect   = "Allow"
        Action   = "ecs:UpdateService"
        Resource = aws_ecs_service.portal.id
        Condition = {
          ArnEquals = { "ecs:task-definition" = aws_ecs_task_definition.portal.arn }
        }
      },
      {
        Sid      = "DescribeOnlyThisPortalService"
        Effect   = "Allow"
        Action   = "ecs:DescribeServices"
        Resource = aws_ecs_service.portal.id
      },
      {
        Sid    = "PassOnlyFixedTaskRolesToECS"
        Effect = "Allow"
        Action = "iam:PassRole"
        Resource = [
          aws_iam_role.bootstrap_execution.arn,
          aws_iam_role.bootstrap_task.arn,
          aws_iam_role.portal_execution.arn,
          aws_iam_role.portal_task.arn
        ]
        Condition = { StringEquals = { "iam:PassedToService" = "ecs-tasks.amazonaws.com" } }
      },
      {
        Sid      = "SerializeReleases"
        Effect   = "Allow"
        Action   = ["dynamodb:PutItem", "dynamodb:DeleteItem"]
        Resource = aws_dynamodb_table.release_lock.arn
      },
      {
        Sid       = "LockEncryption"
        Effect    = "Allow"
        Action    = ["kms:Decrypt", "kms:GenerateDataKey"]
        Resource  = aws_kms_key.application.arn
        Condition = { StringEquals = { "kms:ViaService" = "dynamodb.${var.aws_region}.amazonaws.com" } }
      },
      {
        Sid      = "ECSOptimizedIntegrationEvents"
        Effect   = "Allow"
        Action   = ["events:PutTargets", "events:PutRule", "events:DescribeRule"]
        Resource = "arn:${local.partition}:events:${var.aws_region}:${local.account_id}:rule/StepFunctionsGetEventsForECSTaskRule"
      },
      {
        Sid    = "WorkflowLogDeliveryAPIsRequireWildcardResource"
        Effect = "Allow"
        Action = [
          "logs:CreateLogDelivery", "logs:GetLogDelivery", "logs:UpdateLogDelivery",
          "logs:DeleteLogDelivery", "logs:ListLogDeliveries", "logs:PutResourcePolicy",
          "logs:DescribeResourcePolicies", "logs:DescribeLogGroups"
        ]
        Resource = "*"
      }
    ]
  })
}

resource "aws_sfn_state_machine" "rollout" {
  name     = local.rollout_name
  role_arn = aws_iam_role.rollout.arn
  type     = "STANDARD"
  publish  = true
  logging_configuration {
    log_destination        = "${aws_cloudwatch_log_group.workflow.arn}:*"
    include_execution_data = false
    level                  = "ERROR"
  }

  definition = jsonencode({
    Comment        = "Approved portal rollout: idempotent DB bootstrap, fixed ECS deployment, and bounded readiness verification. Request data cannot choose infrastructure."
    QueryLanguage  = "JSONata"
    StartAt        = "ValidateRequest"
    TimeoutSeconds = 1200
    States = {
      ValidateRequest = {
        Type = "Choice"
        Choices = [{
          Condition = "{% $type($states.input.requestId) = 'string' ? ($exists($match($states.input.requestId, /^[A-Za-z0-9_-]{1,64}$/)) and $states.input.operation = 'rollout') : false %}"
          Next      = var.image_digest == null ? "ImageNotPublished" : "RememberRequest"
        }]
        Default = "InvalidRequest"
      }
      RememberRequest = {
        Type   = "Pass"
        Assign = { requestId = "{% $states.input.requestId %}", operation = "{% $states.input.operation %}" }
        Output = { requestId = "{% $states.input.requestId %}", operation = "{% $states.input.operation %}" }
        Next   = "AcquireReleaseLock"
      }
      AcquireReleaseLock = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::dynamodb:putItem"
        TimeoutSeconds = 30
        Arguments = {
          TableName = aws_dynamodb_table.release_lock.name
          Item = {
            LockId = { S = "portal" }
            Owner  = { S = "{% $states.context.Execution.Id %}" }
          }
          ConditionExpression = "attribute_not_exists(LockId)"
        }
        Retry = [{
          ErrorEquals     = ["DynamoDB.ProvisionedThroughputExceededException", "DynamoDB.RequestLimitExceeded", "DynamoDB.InternalServerError"]
          IntervalSeconds = 2
          MaxAttempts     = 2
          BackoffRate     = 2
          MaxDelaySeconds = 10
          JitterStrategy  = "FULL"
        }]
        Catch  = [{ ErrorEquals = ["States.ALL"], Next = "LockNotAcquired" }]
        Output = { requestId = "{% $requestId %}" }
        Next   = "SelectOperation"
      }
      ReleaseSuccessfulLock = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::dynamodb:deleteItem"
        TimeoutSeconds = 30
        Arguments = {
          TableName                 = aws_dynamodb_table.release_lock.name
          Key                       = { LockId = { S = "portal" } }
          ConditionExpression       = "#owner = :owner"
          ExpressionAttributeNames  = { "#owner" = "Owner" }
          ExpressionAttributeValues = { ":owner" = { S = "{% $states.context.Execution.Id %}" } }
        }
        Retry = [{
          ErrorEquals     = ["DynamoDB.ProvisionedThroughputExceededException", "DynamoDB.RequestLimitExceeded", "DynamoDB.InternalServerError"]
          IntervalSeconds = 2
          MaxAttempts     = 2
          BackoffRate     = 2
          MaxDelaySeconds = 10
          JitterStrategy  = "FULL"
        }]
        Catch = [{ ErrorEquals = ["States.ALL"], Next = "LockReleaseFailed" }]
        Next  = "RolloutSucceeded"
      }
      ReleaseFailedLock = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::dynamodb:deleteItem"
        TimeoutSeconds = 30
        Arguments = {
          TableName                 = aws_dynamodb_table.release_lock.name
          Key                       = { LockId = { S = "portal" } }
          ConditionExpression       = "#owner = :owner"
          ExpressionAttributeNames  = { "#owner" = "Owner" }
          ExpressionAttributeValues = { ":owner" = { S = "{% $states.context.Execution.Id %}" } }
        }
        Retry = [{
          ErrorEquals     = ["DynamoDB.ProvisionedThroughputExceededException", "DynamoDB.RequestLimitExceeded", "DynamoDB.InternalServerError"]
          IntervalSeconds = 2
          MaxAttempts     = 2
          BackoffRate     = 2
          MaxDelaySeconds = 10
          JitterStrategy  = "FULL"
        }]
        Catch = [{ ErrorEquals = ["States.ALL"], Next = "LockReleaseFailed" }]
        Next  = "ReleaseFailed"
      }
      ReleaseFailed = {
        Type  = "Fail"
        Error = "{% $failureError %}"
        Cause = "{% $failureCause %}"
      }
      LockNotAcquired = {
        Type  = "Fail"
        Error = "ReleaseLockNotAcquired"
        Cause = "Another release holds the lock, or DynamoDB denied acquisition. No service mutation was attempted. Check the lock and execution history."
      }
      LockReleaseFailed = {
        Type  = "Fail"
        Error = "ReleaseLockRecoveryRequired"
        Cause = "The lock could not be released conditionally. Inspect this execution and ECS before explicit lock recovery."
      }
      SelectOperation = {
        Type    = "Choice"
        Choices = [{ Condition = "{% $operation = 'rotateDatabase' %}", Next = "DrainPortal" }]
        Default = "BootstrapDatabase"
      }
      DrainPortal = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::aws-sdk:ecs:updateService"
        TimeoutSeconds = 30
        Arguments = {
          Cluster        = aws_ecs_cluster.portal.arn
          Service        = aws_ecs_service.portal.name
          TaskDefinition = aws_ecs_task_definition.portal.arn
          DesiredCount   = 0
        }
        Assign = { drainAttempts = 0 }
        Catch  = [{ ErrorEquals = ["States.ALL"], Next = "RotationFailed" }]
        Next   = "WaitForDrain"
      }
      WaitForDrain = { Type = "Wait", Seconds = 5, Next = "DescribeDrain" }
      DescribeDrain = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::aws-sdk:ecs:describeServices"
        TimeoutSeconds = 30
        Arguments      = { Cluster = aws_ecs_cluster.portal.arn, Services = [aws_ecs_service.portal.name] }
        Catch          = [{ ErrorEquals = ["States.ALL"], Next = "RotationFailed" }]
        Output         = "{% $states.result %}"
        Next           = "CheckDrain"
      }
      CheckDrain = {
        Type = "Choice"
        Choices = [
          { Condition = "{% $count($states.input.Failures) = 0 and $count($states.input.Services) = 1 and $states.input.Services[0].Status = 'ACTIVE' and $states.input.Services[0].DesiredCount = 0 and $states.input.Services[0].RunningCount = 0 and $states.input.Services[0].PendingCount = 0 %}", Next = "RotateDatabase" },
          { Condition = "{% $drainAttempts >= 29 %}", Next = "RotationFailed" }
        ]
        Default = "IncrementDrain"
      }
      IncrementDrain = {
        Type   = "Pass"
        Assign = { drainAttempts = "{% $drainAttempts + 1 %}" }
        Next   = "WaitForDrain"
      }
      RotateDatabase = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::ecs:runTask.sync"
        TimeoutSeconds = 300
        Arguments = {
          Cluster              = aws_ecs_cluster.portal.arn
          TaskDefinition       = aws_ecs_task_definition.rotate.arn
          LaunchType           = "FARGATE"
          Count                = 1
          EnableExecuteCommand = false
          StartedBy            = "backstage-db-rotation"
          NetworkConfiguration = { AwsvpcConfiguration = {
            Subnets        = aws_subnet.application[*].id
            SecurityGroups = [aws_security_group.bootstrap.id]
            AssignPublicIp = "DISABLED"
          } }
        }
        Catch  = [{ ErrorEquals = ["States.ALL"], Next = "RotationFailed" }]
        Output = "{% $states.result %}"
        Next   = "CheckBootstrapExit"
      }
      RotationFailed = {
        Type   = "Pass"
        Assign = { failureError = "DatabaseRotationFailed", failureCause = "Rotation maintenance did not complete. Service may be drained. Keep it stopped until the AWSCURRENT application secret and database are reconciled by a normal rollout." }
        Next   = "ReleaseFailedLock"
      }
      BootstrapDatabase = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::ecs:runTask.sync"
        TimeoutSeconds = 300
        Arguments = {
          Cluster              = aws_ecs_cluster.portal.arn
          TaskDefinition       = aws_ecs_task_definition.bootstrap.arn
          LaunchType           = "FARGATE"
          Count                = 1
          EnableExecuteCommand = false
          StartedBy            = "backstage-rollout"
          NetworkConfiguration = {
            AwsvpcConfiguration = {
              Subnets        = aws_subnet.application[*].id
              SecurityGroups = [aws_security_group.bootstrap.id]
              AssignPublicIp = "DISABLED"
            }
          }
        }
        Retry = [{
          ErrorEquals     = ["ECS.ThrottlingException", "ECS.ServerException"]
          IntervalSeconds = 2
          MaxAttempts     = 2
          BackoffRate     = 2
          MaxDelaySeconds = 10
          JitterStrategy  = "FULL"
        }]
        Catch  = [{ ErrorEquals = ["States.ALL"], Next = "BootstrapTaskFailed" }]
        Output = "{% $states.result %}"
        Next   = "CheckBootstrapExit"
      }
      CheckBootstrapExit = {
        Type = "Choice"
        Choices = [{
          Condition = "{% $count($states.input.Containers[Name = 'db-bootstrap']) = 1 and $states.input.Containers[Name = 'db-bootstrap'].ExitCode = 0 %}"
          Next      = "DeployPortal"
        }]
        Default = "BootstrapContainerFailed"
      }
      DeployPortal = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::aws-sdk:ecs:updateService"
        TimeoutSeconds = 30
        Arguments = {
          Cluster            = aws_ecs_cluster.portal.arn
          Service            = aws_ecs_service.portal.name
          TaskDefinition     = aws_ecs_task_definition.portal.arn
          DesiredCount       = var.portal_desired_count
          ForceNewDeployment = true
        }
        Retry = [{
          ErrorEquals     = ["Ecs.ThrottlingException", "Ecs.ServerException"]
          IntervalSeconds = 2
          MaxAttempts     = 2
          BackoffRate     = 2
          MaxDelaySeconds = 10
          JitterStrategy  = "FULL"
        }]
        Catch  = [{ ErrorEquals = ["States.ALL"], Next = "UpdateServiceFailed" }]
        Output = "{% $states.result %}"
        Next   = "ValidateDeploymentReceipt"
      }
      ValidateDeploymentReceipt = {
        Type = "Choice"
        Choices = [{
          Condition = "{% $count($states.input.Service.Deployments[Status = 'PRIMARY']) = 1 and $states.input.Service.Deployments[Status = 'PRIMARY'].TaskDefinition = '${aws_ecs_task_definition.portal.arn}' %}"
          Next      = "RememberDeployment"
        }]
        Default = "InvalidDeploymentReceipt"
      }
      RememberDeployment = {
        Type = "Pass"
        Assign = {
          deploymentId = "{% $states.input.Service.Deployments[Status = 'PRIMARY'].Id %}"
          attempts     = 0
        }
        Output = { requestId = "{% $requestId %}" }
        Next   = "WaitForService"
      }
      WaitForService = {
        Type    = "Wait"
        Seconds = 10
        Next    = "DescribeService"
      }
      DescribeService = {
        Type           = "Task"
        Resource       = "arn:${local.partition}:states:::aws-sdk:ecs:describeServices"
        TimeoutSeconds = 30
        Arguments = {
          Cluster  = aws_ecs_cluster.portal.arn
          Services = [aws_ecs_service.portal.name]
        }
        Retry = [{
          ErrorEquals     = ["Ecs.ThrottlingException", "Ecs.ServerException"]
          IntervalSeconds = 2
          MaxAttempts     = 2
          BackoffRate     = 2
          MaxDelaySeconds = 10
          JitterStrategy  = "FULL"
        }]
        Catch  = [{ ErrorEquals = ["States.ALL"], Next = "DescribeServiceFailed" }]
        Output = "{% $states.result %}"
        Next   = "ValidateServiceResponse"
      }
      ValidateServiceResponse = {
        Type = "Choice"
        Choices = [{
          Condition = "{% $count($states.input.Failures) = 0 and $count($states.input.Services) = 1 and $states.input.Services[0].Status = 'ACTIVE' %}"
          Next      = "CheckDeployment"
        }]
        Default = "DescribeServiceFailed"
      }
      CheckDeployment = {
        Type = "Choice"
        Choices = [
          {
            Condition = "{% $states.input.Services[0].Deployments[Id = $deploymentId].RolloutState = 'FAILED' %}"
            Next      = "DeploymentFailed"
          },
          {
            Condition = "{% $count($states.input.Services[0].Deployments) = 1 and $states.input.Services[0].Deployments[0].Id = $deploymentId and $states.input.Services[0].Deployments[0].Status = 'PRIMARY' and $states.input.Services[0].Deployments[0].TaskDefinition = '${aws_ecs_task_definition.portal.arn}' and $states.input.Services[0].Deployments[0].RolloutState = 'COMPLETED' and $states.input.Services[0].DesiredCount = ${var.portal_desired_count} and $states.input.Services[0].RunningCount = ${var.portal_desired_count} and $states.input.Services[0].PendingCount = 0 %}"
            Next      = "ReleaseSuccessfulLock"
          },
          {
            Condition = "{% $attempts >= 59 %}"
            Next      = "RolloutTimedOut"
          }
        ]
        Default = "IncrementAttempts"
      }
      IncrementAttempts = {
        Type   = "Pass"
        Assign = { attempts = "{% $attempts + 1 %}" }
        Output = { requestId = "{% $requestId %}" }
        Next   = "WaitForService"
      }
      RolloutSucceeded = {
        Type = "Succeed"
        Output = {
          requestId         = "{% $requestId %}"
          operation         = "{% $operation %}"
          status            = "SUCCEEDED"
          deploymentId      = "{% $deploymentId %}"
          serviceArn        = aws_ecs_service.portal.id
          taskDefinitionArn = aws_ecs_task_definition.portal.arn
          portalUrl         = local.portal_base_url
        }
      }
      InvalidRequest = {
        Type  = "Fail"
        Error = "InvalidRequest"
        Cause = "Expected operation=rollout and requestId containing 1-64 letters, digits, hyphens, or underscores."
      }
      ImageNotPublished = {
        Type  = "Fail"
        Error = "ImageNotPublished"
        Cause = "Publish the application image to this stack's ECR repository and apply its image_digest before rollout."
      }
      BootstrapTaskFailed = {
        Type   = "Pass"
        Assign = { failureError = "BootstrapTaskFailed", failureCause = "The approved database task could not run or exceeded its five-minute limit. Inspect ECS stopped reason and DB-bootstrap logs." }
        Next   = "ReleaseFailedLock"
      }
      BootstrapContainerFailed = {
        Type   = "Pass"
        Assign = { failureError = "BootstrapContainerFailed", failureCause = "The db-bootstrap container did not report exit code zero. The portal was not updated. Inspect DB-bootstrap logs." }
        Next   = "ReleaseFailedLock"
      }
      UpdateServiceFailed = {
        Type   = "Pass"
        Assign = { failureError = "UpdateServiceFailed", failureCause = "ECS rejected or timed out the approved service update. Inspect execution history and ECS service events." }
        Next   = "ReleaseFailedLock"
      }
      InvalidDeploymentReceipt = {
        Type   = "Pass"
        Assign = { failureError = "InvalidDeploymentReceipt", failureCause = "ECS did not return one primary deployment using the approved task definition. No success is assumed." }
        Next   = "ReleaseFailedLock"
      }
      DescribeServiceFailed = {
        Type   = "Pass"
        Assign = { failureError = "DescribeServiceFailed", failureCause = "ECS did not return exactly one active service without failures." }
        Next   = "ReleaseFailedLock"
      }
      DeploymentFailed = {
        Type   = "Pass"
        Assign = { failureError = "DeploymentFailed", failureCause = "The requested ECS deployment failed. The ECS circuit breaker may restore the previous deployment." }
        Next   = "ReleaseFailedLock"
      }
      RolloutTimedOut = {
        Type   = "Pass"
        Assign = { failureError = "RolloutTimedOut", failureCause = "The requested deployment did not stabilize within 60 polls. A replacement deployment or rollback cannot satisfy this check. Inspect ECS events." }
        Next   = "ReleaseFailedLock"
      }
    }
  })
  depends_on = [
    aws_iam_role_policy.rollout,
    aws_iam_role_policy.bootstrap_execution,
    aws_iam_role_policy.bootstrap_task,
    aws_route.application_egress
  ]
}
