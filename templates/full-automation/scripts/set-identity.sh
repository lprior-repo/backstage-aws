#!/usr/bin/env bash
# Secret values travel through a file reference, never CLI arguments or Terraform.
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
if [[ $# -ne 1 ]] || [[ ! -f "$1" ]]; then
  printf 'Usage: scripts/set-identity.sh /absolute/path/to/github-identity.json\n' >&2
  exit 2
fi
load_stack
BACKSTAGE_IDENTITY_FILE="$(cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1")"
chmod 600 "$BACKSTAGE_IDENTITY_FILE"
jq -e -s 'length == 1 and (.[0] | type == "object" and (.clientId | type == "string" and length > 0 and length <= 256) and (.clientSecret | type == "string" and length >= 16 and length <= 1024) and (.sessionSecret | type == "string" and length >= 64 and length <= 256))' \
  "$BACKSTAGE_IDENTITY_FILE" >/dev/null
BACKSTAGE_IDENTITY_ARN="$(stack_output identity_secret_arn)"
aws --region "$BACKSTAGE_AWS_REGION" secretsmanager put-secret-value \
  --secret-id "$BACKSTAGE_IDENTITY_ARN" --secret-string "file://$BACKSTAGE_IDENTITY_FILE" \
  --query ARN --output text
printf 'Identity credentials saved. Existing tasks receive changed values on the next rollout.\n'

