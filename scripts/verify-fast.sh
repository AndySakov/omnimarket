#!/usr/bin/env bash
# The git pre-commit gate (D86). A commit whose staged files are all under web/terminal/ runs the
# frontend's fast checks; every other commit runs scripts/verify.sh, the same checks as CI's verify job.
set -euo pipefail
cd "$(dirname "$0")/.."

need() { command -v "$1" >/dev/null || { echo "verify-fast: $1 is required but not installed" >&2; exit 1; }; }

# --no-renames lists a moved file under both paths, so moving one out of crates/ still counts as a backend change.
staged=$(git diff --cached --name-only --no-renames)

# Fails closed: an empty commit, or any path outside web/terminal/ (clippy.toml, .github/, docs, a new
# top-level directory), gets the full checks.
if [[ -z $staged ]] || grep -qv '^web/terminal/' <<<"$staged"; then
  exec scripts/verify.sh
fi

need node
need npm
[[ -d web/terminal/node_modules ]] || { echo "verify-fast: web/terminal/node_modules is missing; run npm ci in web/terminal" >&2; exit 1; }
cd web/terminal
npm run verify:fast
