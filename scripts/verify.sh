#!/usr/bin/env bash
# The definition of done in CLAUDE.md, as one command. CI and the commit gate both run this.
# Each check runs once the part of the repo it covers exists.
set -euo pipefail
cd "$(dirname "$0")/.."

ran=0
need() { command -v "$1" >/dev/null || { echo "verify: $1 is required but not installed" >&2; exit 1; }; }

if [[ -f Cargo.toml ]]; then
  need cargo
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  scripts/check-determinism.sh
  ran=1
fi

if [[ -f contracts/foundry.toml ]]; then
  need forge
  (cd contracts && forge test)
  ran=1
fi

if [[ -f proto/buf.yaml ]]; then
  scripts/proto-check.sh
  ran=1
fi

[[ $ran == 1 ]] || echo "verify: nothing to check yet"
