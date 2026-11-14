#!/usr/bin/env bash
# Shared by operator scripts; never evaluates Terraform output as shell code.
set -euo pipefail
umask 077

BACKSTAGE_PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
export AWS_PAGER=""

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'Required tool is missing: %s\n' "$1" >&2
    exit 1
  fi
}

stack_output() {
  terraform -chdir="$BACKSTAGE_PROJECT_DIR/infra/core" output -raw "$1"
}

load_stack() {
  require_tool terraform
  require_tool aws
  require_tool jq
  BACKSTAGE_AWS_REGION="$(stack_output aws_region)"
  BACKSTAGE_AWS_ACCOUNT="$(stack_output aws_account_id)"
  if [[ ! "$BACKSTAGE_AWS_REGION" =~ ^[a-z][a-z0-9-]{1,31}$ ]] || [[ ! "$BACKSTAGE_AWS_ACCOUNT" =~ ^[0-9]{12}$ ]]; then
    printf 'Terraform returned invalid account or region metadata.\n' >&2
    exit 1
  fi
  local caller_account
  caller_account="$(aws --region "$BACKSTAGE_AWS_REGION" sts get-caller-identity --query Account --output text)"
  if [[ "$caller_account" != "$BACKSTAGE_AWS_ACCOUNT" ]]; then
    printf 'AWS account mismatch: stack is %s but credentials are for %s.\n' "$BACKSTAGE_AWS_ACCOUNT" "$caller_account" >&2
    exit 1
  fi
}

