#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

need() {
  command -v "$1" >/dev/null || {
    echo "verify-fast: $1 is required but not installed" >&2
    exit 1
  }
}

changed_files="$(git diff --cached --name-only)"
if [[ -z "$changed_files" ]]; then
  echo "verify-fast: no staged files to check"
  exit 0
fi

if printf '%s\n' "$changed_files" | grep -qv '^web/terminal/'; then
  scripts/verify.sh
  exit 0
fi

need node
need npm
if [[ ! -d web/terminal/node_modules ]]; then
  echo "verify-fast: web/terminal/node_modules is missing; run npm ci in web/terminal" >&2
  exit 1
fi
(cd web/terminal && npm run verify:fast)
