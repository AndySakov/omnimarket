#!/usr/bin/env bash
# PreToolUse hook: refuses `git commit` while scripts/verify.sh fails.
# Silence allows the commit; a JSON deny blocks it and shows the failure.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 0
cat >/dev/null

log=$(mktemp -t omnimarket-verify)
if ./scripts/verify.sh >"$log" 2>&1; then
  exit 0
fi

jq -n --arg r "$(tail -40 "$log")" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: ("scripts/verify.sh failed, commit blocked.\n\n" + $r)
  }
}'
