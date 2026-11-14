#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
for script in scripts/*.sh platform; do bash -n "$script"; done
for script in automation/*.mjs; do node --check "$script"; done
terraform fmt -check -recursive infra
node --test automation/*.test.mjs scripts/operator.test.mjs app/scripts/bootstrap-db.test.mjs
npm ci --prefix infra/tools --ignore-scripts --no-audit --no-fund
npm run render --prefix infra/tools
npm test --prefix infra/tools
for stack in bootstrap core; do
  terraform -chdir="infra/$stack" init -backend=false -input=false -lockfile=readonly
  terraform -chdir="infra/$stack" validate
  terraform -chdir="infra/$stack" test
 done
