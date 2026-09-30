#!/usr/bin/env bash
# buf lint, and buf breaking against main (D70), plus a self-test proving breaking still bites.
set -euo pipefail
cd "$(dirname "$0")/.."
buf=scripts/buf

(cd proto && "../$buf" lint)

# Compare against main's schemas: origin/main when fetched (CI fetches it), else local main.
base=$(git rev-parse -q --verify origin/main || git rev-parse -q --verify main || true)
if [[ -n $base ]] && git cat-file -e "$base:proto/buf.yaml" 2>/dev/null; then
  against=$(mktemp -d)
  trap 'rm -rf "$against"' EXIT
  git archive "$base" proto | tar -x -C "$against"
  "$buf" breaking proto --against "$against/proto"
fi

fixture=scripts/fixtures/proto-breaking
if "$buf" breaking $fixture/after --against $fixture/before >/dev/null 2>&1; then
  echo "proto: buf breaking no longer rejects its planted breaking change" >&2
  exit 1
fi
