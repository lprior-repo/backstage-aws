#!/usr/bin/env bash
# Publish a release, register its approved task definitions, then activate it.
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
if [[ $# -gt 1 ]]; then
  printf 'Usage: scripts/deploy.sh [unique-image-tag]\n' >&2
  exit 2
fi
load_stack
BACKSTAGE_IDENTITY_ARN="$(stack_output identity_secret_arn)"
if ! aws --region "$BACKSTAGE_AWS_REGION" secretsmanager describe-secret \
  --secret-id "$BACKSTAGE_IDENTITY_ARN" --query VersionIdsToStages --output json \
  | jq -e 'type == "object" and any(.[]?; index("AWSCURRENT") != null)' >/dev/null; then
  printf 'The identity secret has no readable AWSCURRENT version. Run scripts/set-identity.sh before deploying.\n' >&2
  exit 1
fi
"$BACKSTAGE_PROJECT_DIR/scripts/publish-image.sh" "$@"
BACKSTAGE_EXPECTED_IMAGE_DIGEST="$(jq -er '.image_digest' "$BACKSTAGE_PROJECT_DIR/infra/core/image.auto.tfvars.json")"
# An explicit final var-file takes precedence over other auto tfvars and
# TF_CLI_ARGS_apply settings. Verify the resulting approved value before rollout.
terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/core" apply -var-file=image.auto.tfvars.json
BACKSTAGE_APPROVED_IMAGE_DIGEST="$(stack_output approved_image_digest)"
if [[ "$BACKSTAGE_APPROVED_IMAGE_DIGEST" != "$BACKSTAGE_EXPECTED_IMAGE_DIGEST" ]]; then
  printf 'The Terraform-approved image digest differs from the image just published. No rollout was started.\n' >&2
  exit 1
fi
"$BACKSTAGE_PROJECT_DIR/scripts/rollout.sh"

