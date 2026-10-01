# AGENTS.md

Instructions for coding agents other than Claude Code (Codex reads this file). The project's rules live in [CLAUDE.md](CLAUDE.md); they apply to every agent and person working here, whatever the file is called. Read it first, then [CONTEXT.md](CONTEXT.md) for the vocabulary.

## Frontend work alongside the build account

Jutin owns the frontend track (`web/terminal/`). While he works at reduced capacity, agent sessions on a second Claude account, the **build account** ("the farm", D90), build some frontend issues too. Both of you push to this repo, so the farm must know what Jutin is doing. His working agreement, including how he works with the farm, is [web/terminal/docs/collaboration-workflow.md](web/terminal/docs/collaboration-workflow.md). The short version:

1. **Before starting an issue,** check that the farm isn't on it: no `wip` label, and no open PR that says `Closes #<n>` or `Part of #<n>`. Then tell Temi which issue you're taking. He (or the farm's orchestrator) labels it `jutin`, and agents never take an issue labelled `jutin`. Comment `Taking this` on the issue too.
2. **Every PR body names its issue** with `Closes #<n>`, or `Part of #<n>` for a slice that leaves the issue open. That line is how the farm sees your work. A PR without it is invisible to the queue, and the farm may build the same thing.
3. **Build on what's merged, not around it.** Use the generated API types in `web/terminal/src/api/generated` (from #75's contract) and the shared client from #63 once it lands. Never hand-write copies of contract types. If a farm PR is open on code you need, read it first (`gh api repos/AndySakov/omnimarket/pulls/<n>/files`) and raise overlaps on the PR instead of duplicating.
4. **Never push to a farm branch** (`claude/…`). The farm never pushes to yours.
5. **Review gate.** Your PRs need CI's `verify` (and `frontend`) plus `watchdog/review`, like every PR (D81). The farm's watchdog reviews every open PR whose CI is green, yours included, and posts its verdict as a PR comment. Fix blocking findings on your branch and push; each push gets a fresh review. If you think a finding is wrong, reply on the PR and leave it for Temi.

## Commands

From `web/terminal/`: `npm run verify:fast` (typecheck, lint, unit), and `npm run verify:pr` before asking for review. The repo's pre-commit hook runs the fast checks on frontend-only commits (`scripts/setup.sh` installs it).
