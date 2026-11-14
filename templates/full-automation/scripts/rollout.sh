#!/usr/bin/env bash
# Launches only the state machine selected by this stack; at most 180 status polls.
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
if [[ $# -gt 1 ]]; then
  printf 'Usage: scripts/rollout.sh [unique-request-id]\n' >&2
  exit 2
fi
load_stack
BACKSTAGE_REQUEST_ID="${1:-deploy-$(date -u +%Y%m%dT%H%M%SZ)}"
if [[ ! "$BACKSTAGE_REQUEST_ID" =~ ^[a-zA-Z0-9_-]{1,64}$ ]]; then
  printf 'Request ID must be 1–64 letters, digits, underscores or hyphens.\n' >&2
  exit 2
fi
BACKSTAGE_STATE_MACHINE="$(stack_output rollout_state_machine_arn)"
BACKSTAGE_EXECUTION_ARN="$(aws --region "$BACKSTAGE_AWS_REGION" stepfunctions start-execution \
  --state-machine-arn "$BACKSTAGE_STATE_MACHINE" --name "portal-$BACKSTAGE_REQUEST_ID" \
  --input "$(jq -nc --arg id "$BACKSTAGE_REQUEST_ID" '{operation:"rollout",requestId:$id}')" \
  --query executionArn --output text)"
printf 'Started %s\n' "$BACKSTAGE_EXECUTION_ARN"
for ((BACKSTAGE_ATTEMPT=0; BACKSTAGE_ATTEMPT<180; BACKSTAGE_ATTEMPT++)); do
  BACKSTAGE_EXECUTION_STATUS="$(aws --region "$BACKSTAGE_AWS_REGION" stepfunctions describe-execution \
    --execution-arn "$BACKSTAGE_EXECUTION_ARN" --query status --output text)"
  case "$BACKSTAGE_EXECUTION_STATUS" in
    SUCCEEDED)
      printf 'Rollout succeeded: %s\n' "$(stack_output portal_url)"
      exit 0
      ;;
    RUNNING) sleep 10 ;;
    *)
      aws --region "$BACKSTAGE_AWS_REGION" stepfunctions describe-execution \
        --execution-arn "$BACKSTAGE_EXECUTION_ARN" --query '{status:status,error:error,cause:cause}' --output json
      exit 1
      ;;
  esac
done
printf 'Local wait limit reached. The remote execution was not cancelled: %s\n' "$BACKSTAGE_EXECUTION_ARN" >&2
exit 1

