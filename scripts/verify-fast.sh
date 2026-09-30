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

frontend_changed=0
backend_changed=0

if printf '%s\n' "$changed_files" | grep -q '^web/terminal/'; then
  frontend_changed=1
fi

if printf '%s\n' "$changed_files" | grep -Eq '^(Cargo\.toml|Cargo\.lock|rust-toolchain\.toml|crates/|src/|contracts/|proto/|scripts/|deploy/)'; then
  backend_changed=1
fi

if [[ $backend_changed == 1 ]]; then
  scripts/verify.sh
  exit 0
fi

if [[ $frontend_changed == 1 ]]; then
  need node
  need npm
  if [[ ! -d web/terminal/node_modules ]]; then
    echo "verify-fast: web/terminal/node_modules is missing; run npm ci in web/terminal" >&2
    exit 1
  fi
  (cd web/terminal && npm run verify:fast)
  exit 0
fi

echo "verify-fast: no frontend or backend files changed"
