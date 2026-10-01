#!/usr/bin/env bash
# SessionStart hook for Claude Code on the web (D90). Readies a fresh cloud container so the commit
# gate and the tests can run: the Docker daemon, the tracked git hooks, the pinned buf, the terminal's
# npm deps, and a cargo build warmed in the background. Local sessions skip all of it.
set -uo pipefail

[[ ${CLAUDE_CODE_REMOTE:-} == true ]] || exit 0
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" || exit 0

status=()

# The local stack (scripts/stack) and the Kafka integration test need dockerd, which the container
# doesn't start on its own.
if ! docker info >/dev/null 2>&1; then
  setsid nohup dockerd >/tmp/dockerd.log 2>&1 < /dev/null &
  for _ in $(seq 30); do docker info >/dev/null 2>&1 && break; sleep 1; done
fi
docker info >/dev/null 2>&1 && status+=("docker: up") || status+=("docker: NOT running (see /tmp/dockerd.log)")

git config core.hooksPath .githooks && status+=("git hooks: .githooks")

scripts/buf --version >/dev/null 2>&1 && status+=("buf: ready") || status+=("buf: download failed")

# npm ci only when node_modules is missing or older than the lockfile, so a cached container is reused.
lock=web/terminal/package-lock.json
if [[ ! -d web/terminal/node_modules || $lock -nt web/terminal/node_modules ]]; then
  (cd web/terminal && npm ci --no-audit --no-fund >/tmp/npm-ci.log 2>&1) \
    && touch web/terminal/node_modules && status+=("npm: installed") \
    || status+=("npm: ci failed (see /tmp/npm-ci.log)")
else
  status+=("npm: cached")
fi

# Playwright's pinned Chromium, for the terminal's e2e and a11y tests. Where it can't be downloaded,
# fall back to the container's preinstalled Chromium (playwright.config.ts reads the variable).
if ! (cd web/terminal && PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD= npx playwright install chromium >/tmp/playwright-install.log 2>&1); then
  fallback=$(ls -d /opt/pw-browsers/chromium-*/chrome-linux/chrome 2>/dev/null | tail -1)
  if [[ -n $fallback && -n ${CLAUDE_ENV_FILE:-} ]]; then
    echo "export PLAYWRIGHT_CHROMIUM_PATH=$fallback" >>"$CLAUDE_ENV_FILE"
    status+=("playwright: preinstalled Chromium (visual tests run in CI only)")
  else
    status+=("playwright: no Chromium (see /tmp/playwright-install.log)")
  fi
else
  status+=("playwright: pinned Chromium")
fi

# Crates download now; compiling takes minutes cold, so it runs detached. A cargo command run
# meanwhile waits on cargo's build lock, then reuses what's built.
cargo fetch --quiet >/dev/null 2>&1 && status+=("crates: fetched") || status+=("crates: fetch failed")
mkdir -p target
setsid nohup bash -c 'cargo test --workspace --no-run && cargo clippy --workspace --all-targets' \
  >target/warm-build.log 2>&1 < /dev/null &
status+=("cargo: warming in background (target/warm-build.log)")

# gh reaches GitHub through the environment's claude.ai GitHub connection, which must act as AndySakov:
# scripts/work and the watchdog-status workflow accept only that account (D90).
if gh_user=$(gh api user --jq .login 2>/dev/null); then
  [[ $gh_user == AndySakov ]] && status+=("gh: authenticated as AndySakov") \
    || status+=("gh: authenticated as $gh_user, NOT AndySakov; reconnect GitHub as AndySakov")
else
  status+=("gh: NOT authenticated; connect the environment's GitHub as AndySakov")
fi

printf 'session-start: %s\n' "$(IFS=';'; echo "${status[*]}" | sed 's/;/; /g')"
exit 0
