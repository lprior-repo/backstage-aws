#!/usr/bin/env bash
# Creates AWS resources. Terraform presents each plan for operator approval.
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
require_tool terraform
require_tool aws

if [[ $# -ne 0 ]]; then
  printf 'Usage: scripts/provision.sh\n' >&2
  exit 2
fi
for stack in bootstrap core; do
  if [[ ! -f "$BACKSTAGE_PROJECT_DIR/infra/$stack/terraform.tfvars" && ! -f "$BACKSTAGE_PROJECT_DIR/infra/$stack/terraform.tfvars.json" ]]; then
    printf 'Copy and edit infra/%s/terraform.tfvars.example first.\n' "$stack" >&2
    exit 1
  fi
done

if [[ -f "$BACKSTAGE_PROJECT_DIR/infra/bootstrap/backend.local.tf" ]]; then
  if [[ ! -f "$BACKSTAGE_PROJECT_DIR/infra/core/backend.local.hcl" ]]; then
    printf 'Restore infra/core/backend.local.hcl before initializing this existing deployment.\n' >&2
    exit 1
  fi
  terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" init -migrate-state \
    -backend-config=../core/backend.local.hcl \
    -backend-config=key=bootstrap/terraform.tfstate
  if ! terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" output -raw state_bucket_arn >/dev/null; then
    printf 'Existing bootstrap backend has no readable state. Restore/migrate it before applying again.\n' >&2
    exit 1
  fi
else
  terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" init
fi
terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" apply
BACKSTAGE_EXPECTED_STATE_BUCKET="$(terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" output -raw state_bucket_arn)"
BACKSTAGE_BACKEND_TEMP="$(mktemp "$BACKSTAGE_PROJECT_DIR/infra/core/backend.local.hcl.XXXXXX")"
trap 'rm -f -- "$BACKSTAGE_BACKEND_TEMP"' EXIT
terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" output -raw backend_hcl > "$BACKSTAGE_BACKEND_TEMP"
if [[ ! -s "$BACKSTAGE_BACKEND_TEMP" ]]; then
  printf 'Terraform returned empty backend configuration.\n' >&2
  exit 1
fi
mv -f -- "$BACKSTAGE_BACKEND_TEMP" "$BACKSTAGE_PROJECT_DIR/infra/core/backend.local.hcl"

# Move the bootstrap's own state to a distinct key in the newly created bucket.
# Retain backend.local.tf locally; the file is ignored by git and packaging.
if [[ ! -f "$BACKSTAGE_PROJECT_DIR/infra/bootstrap/backend.local.tf" ]]; then
  cat > "$BACKSTAGE_PROJECT_DIR/infra/bootstrap/backend.local.tf" <<'HCL'
terraform {
  backend "s3" {}
}
HCL
  terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" init -migrate-state \
    -backend-config=../core/backend.local.hcl \
    -backend-config=key=bootstrap/terraform.tfstate
fi

# A successful init command alone does not prove the migration was accepted.
# Confirm the expected bootstrap resources are in the selected remote state.
if ! BACKSTAGE_REMOTE_STATE_BUCKET="$(terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/bootstrap" output -raw state_bucket_arn)" \
  || [[ "$BACKSTAGE_REMOTE_STATE_BUCKET" != "$BACKSTAGE_EXPECTED_STATE_BUCKET" ]]; then
  printf 'Bootstrap remote state could not be confirmed. Restore/migrate the bootstrap state before initializing core.\n' >&2
  exit 1
fi

terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/core" init -backend-config=backend.local.hcl
terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/core" apply
printf '\nInfrastructure is ready. Initial ECS services stay at zero until a successful rollout; existing service revisions/counts remain owned by the workflow.\n'
printf 'Next: configure GitHub OAuth, set the identity secret, then run scripts/deploy.sh.\n'

