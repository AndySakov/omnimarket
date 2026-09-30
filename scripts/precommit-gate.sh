#!/usr/bin/env bash
# PreToolUse hook on `git commit`: refuses --no-verify, and refuses the commit while
# scripts/verify.sh fails. When the tracked git hooks are installed, git's pre-commit
# runs verify itself, so this only guards against skipping it.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 0
cmd=$(jq -r '.tool_input.command // ""')

deny() {
  jq -n --arg r "$1" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $r
    }
  }'
  exit 0
}

[[ $cmd =~ git[^\;\&\|]*[[:space:]]commit([[:space:]]|$) ]] || exit 0

if [[ $cmd =~ commit[^\;\&\|]*[[:space:]](--no-verify|-n)([[:space:]]|$) ]]; then
  deny "Commits must pass scripts/verify.sh; --no-verify is not allowed. Fix the failure instead."
fi

[[ $(git config core.hooksPath) == .githooks ]] && exit 0

# An explicit template: GNU mktemp (Linux) rejects `-t prefix` without X's, which denied every commit.
log=$(mktemp "${TMPDIR:-/tmp}/omnimarket-verify.XXXXXX") || deny "precommit-gate: mktemp failed, commit blocked."
./scripts/verify.sh >"$log" 2>&1 || deny "scripts/verify.sh failed, commit blocked.

$(tail -40 "$log")"
