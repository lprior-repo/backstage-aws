#!/usr/bin/env bash
# Builds from the pinned source and writes the pushed digest to a local tfvars file.
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
if [[ $# -gt 1 ]]; then
  printf 'Usage: scripts/publish-image.sh [unique-image-tag]\n' >&2
  exit 2
fi
load_stack
require_tool node
require_tool docker
BACKSTAGE_IMAGE_TAG="${1:-backstage-1.56.5-$(date -u +%Y%m%dT%H%M%SZ)}"
if [[ ! "$BACKSTAGE_IMAGE_TAG" =~ ^[a-zA-Z0-9_][a-zA-Z0-9_.-]{0,127}$ ]]; then
  printf 'Invalid ECR image tag.\n' >&2
  exit 2
fi
BACKSTAGE_ECR_URL="$(stack_output ecr_repository_url)"
BACKSTAGE_ECR_NAME="$(stack_output ecr_repository_name)"
BACKSTAGE_ECR_REGISTRY="${BACKSTAGE_ECR_URL%%/*}"

cd "$BACKSTAGE_PROJECT_DIR/app"
node ../scripts/check-owner.mjs
# The Docker build owns dependency installation, compilation, tests, and native
# build tools. Operators do not need Python/C++/libsecret headers on their host.
docker build --platform linux/amd64 --file packages/backend/Dockerfile --tag "$BACKSTAGE_ECR_URL:$BACKSTAGE_IMAGE_TAG" .
aws --region "$BACKSTAGE_AWS_REGION" ecr get-login-password \
  | docker login --username AWS --password-stdin "$BACKSTAGE_ECR_REGISTRY"
docker push "$BACKSTAGE_ECR_URL:$BACKSTAGE_IMAGE_TAG"
BACKSTAGE_IMAGE_DIGEST="$(aws --region "$BACKSTAGE_AWS_REGION" ecr describe-images \
  --repository-name "$BACKSTAGE_ECR_NAME" --image-ids "imageTag=$BACKSTAGE_IMAGE_TAG" \
  --query 'imageDetails[0].imageDigest' --output text)"
if [[ ! "$BACKSTAGE_IMAGE_DIGEST" =~ ^sha256:[a-f0-9]{64}$ ]]; then
  printf 'ECR did not return a valid digest; no Terraform image variable was written.\n' >&2
  exit 1
fi
BACKSTAGE_DIGEST_TEMP="$(mktemp "$BACKSTAGE_PROJECT_DIR/infra/core/image.auto.tfvars.json.XXXXXX")"
trap 'rm -f -- "$BACKSTAGE_DIGEST_TEMP"' EXIT
jq -n --arg digest "$BACKSTAGE_IMAGE_DIGEST" '{image_digest: $digest}' > "$BACKSTAGE_DIGEST_TEMP"
mv -f -- "$BACKSTAGE_DIGEST_TEMP" "$BACKSTAGE_PROJECT_DIR/infra/core/image.auto.tfvars.json"
printf '\nPublished immutable image: %s@%s\n' "$BACKSTAGE_ECR_URL" "$BACKSTAGE_IMAGE_DIGEST"
printf 'The selected digest is in infra/core/image.auto.tfvars.json.\n'

