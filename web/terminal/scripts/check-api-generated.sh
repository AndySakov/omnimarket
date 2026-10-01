#!/usr/bin/env bash
# Fails when src/api/generated doesn't match what `buf generate` makes from proto/omnimarket/api
# (#75): regenerates in place, then asks git whether anything changed.
set -euo pipefail
cd "$(dirname "$0")/.."
npm run --silent api:generate
if [[ -n $(git status --porcelain -- src/api/generated) ]]; then
  git status --short -- src/api/generated >&2
  echo "api:check: src/api/generated is stale; run 'npm run api:generate' and commit the result" >&2
  exit 1
fi
