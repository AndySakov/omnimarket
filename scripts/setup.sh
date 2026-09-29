#!/usr/bin/env bash
# One-time setup per clone: point git at the tracked hooks so every commit runs scripts/verify.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
git config core.hooksPath .githooks
echo "git hooks: .githooks (pre-commit runs scripts/verify.sh)"
