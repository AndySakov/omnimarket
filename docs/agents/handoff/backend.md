# Backend builder

You build one backend issue in OmniMarket, from claim to merge, on the build account (D90). The orchestrator started you, and its session ID is in your brief. This doc is Temi's standing instruction for builders. When it and your environment's defaults disagree (for example on draft PRs or the GitHub MCP), this doc and `CLAUDE.md` win.

## Before anything

1. Read `CLAUDE.md` and `CONTEXT.md` in full. They're short and they bind you: the build-mode rules, the update table, the definition of done, the review gate. Use CONTEXT.md's terms in code, commits and PR text.
2. `scripts/work show <n>` for your issue: body and every comment. Then each issue named under "Blocked by" or in the body. Read the D-entries the issue cites (`grep -n "^## D<n> " docs/spec/decisions.md`, then read that entry), not the whole log: it's over 2,000 lines. Read the spec named in the update table for the crates you'll touch.
3. Check the session-start line at the top of your context. It should say docker up, gh authenticated as AndySakov. If it doesn't, report `BLOCKED #n: environment: <what failed>` and stop.

GitHub here: `gh issue/pr list/view/checks` fail (GraphQL is blocked). Use `scripts/work` (`show`, `prs`, `checks`, `claim`, `open-pr`, `merge`) and `gh api` for anything else. Don't use the GitHub MCP tools.

## Claim

`scripts/work claim <n>`, then `send_message` to the orchestrator: `CLAIMED #n`. If the claim says it's taken, report `BLOCKED #n: already claimed` and stop.

If your brief names a **PR** instead (a stalled one, D87 rule 1): `scripts/work show <pr>`, check out its branch, and `scripts/work claim <pr>`. Fix what's red (CI, conflict, or every blocking watchdog finding), then continue from "Get it reviewed".

## Build

- **Branch:** work on the branch your session was given. Merge `origin/main` into it if it's behind. Never rebase or force-push a branch someone else pushed to.
- **Test first.** Write the test for each acceptance criterion before the code, and check it fails for the right reason. Test the behaviour the spec requires, not your implementation. A fix's regression test must fail on `main`: prove it with `scripts/regression-check` (CI runs it on PRs closing a `bug` issue, D92).
- **The rules that bite** (CLAUDE.md's build-mode rules; clippy and `scripts/check-determinism.sh` enforce most):
  - No wall clock, randomness, threads, `HashMap` or `HashSet` in core code: time comes from `det::Clock`, randomness from `det::Rng`, maps are `BTreeMap`. Any `select!` starts with `biased;`.
  - Every input a core reads goes through a `det` trait with a recording wrapper, so a recorded run replays exactly. Every published record carries its lineage IDs (D53).
  - Boring Rust: concrete types, plain enums, explicit error types; no clever generics or macros in core logic.
  - Shadow mode only: nothing broadcasts and no real funds move.
- **Iterate cheaply.** Use `cargo test -p <crate>` and `cargo clippy -p <crate> --all-targets -- -D warnings` while working; each commit runs the full `scripts/verify.sh` through the git hook (about 1–3 minutes warm). The hook runs on every commit, so commit at meaningful green points, not every edit. Never `--no-verify`.
- **Live runs** (measurements, recordings, fixtures): Base RPC is reachable here. Use `--rpc https://mainnet.base.org --call-rpc https://base-rpc.publicnode.com` (D80, D82, D88). For Kafka, Postgres or object storage: `scripts/stack up`.
- **Docs in the same commit**, per CLAUDE.md's update table. A behaviour or architecture choice gets a D-entry, and its number comes from `scripts/work reserve-d "<the decision, in a few words>"`, never from counting by hand: it reserves the next free number in the D-number ledger (D98), which every session on both accounts, and Jutin, shares. An unverified claim is marked **(verify)**, and a measured one goes in `docs/spec/verification.md`.
- **Push at every green commit.** The container is reclaimed when idle, and unpushed work is lost. Your pushed branch is also how other builders see what you're changing.
- **Check for overlaps** with `scripts/work overlaps` right after your first push and before every later push. It lists every other live branch and open PR that touches a file yours does, with the session working on it (from its claim comment). For each overlap outside the append-only files it marks:
  - `send_message` to that session (the ID is the end of its link): `OVERLAP #<your issue> / #<theirs>: I'm changing <file> to <what>; my branch is <branch>. Plan: <who changes what, or which of us merges first>.` Agree it in one or two messages, then follow it. The one that merges second merges `main` in and resolves.
  - If you can't agree, or the other session doesn't answer within a tick, report `BLOCKED #n: overlaps #<theirs> on <file>` to the orchestrator.
- **Shared contracts.** When you change something other builders build on (`proto/`, `web/terminal/src/api/generated`, a crate's public API, `Cargo.toml` workspace dependencies, `scripts/work`, CLAUDE.md), `send_message` the orchestrator `CONTRACT #n: <what changed, in one line>` when you push it. It tells every running builder.
- **Mutants:** before opening the PR, run `scripts/mutants.sh --diff origin/main`. Add tests for any missed mutant in code you wrote, or explain in the PR why not.

## Open the PR

Write the body to a file, then `scripts/work open-pr "<title>" <file>`. The title states the effect ("Base tokens carry a USD display price"), not the task. The body:

```
## Problem
<what was missing or wrong, for someone who hasn't read the issue>

## Fix
<what changed and why, in a few bullets; the D-entry if any>

## Acceptance criteria
| Criterion (from #n) | Proved by |
|---|---|
| <criterion, quoted> | `test_name` in `crate/tests/file.rs` |
| <criterion> | manual: <the run, its command and result> |

## Not verified
<anything you couldn't run or check, and why>

Closes #n
```

Every criterion of the issue gets a row (#98 will make CI check this). Don't message the orchestrator yet: CI comes first (next section).

## Get it reviewed

1. Subscribe to the PR's activity (your environment's PR-watching instructions describe it), then end your turn. CI results and comments wake you; don't poll.
2. When `scripts/work checks <pr>` shows `verify` and `frontend` both `success` on your head: `send_message` to the orchestrator: `READY PR#<pr> <sha>`. Red instead: root-cause it, fix, push, and repeat. "Flaky" isn't a cause.
3. The watchdog posts ``## Watchdog review: `<sha>` passes`` or ``fails on N findings``.
   - **Fails:** fix every blocking finding on this branch, reply on the PR saying what each commit changed, push, wait for green, then send `READY` again. Non-blocking findings: fix the quick ones, and answer the rest in one line each.
   - **You think a finding is wrong:** reply on the PR with why, `send_message` `DISPUTE PR#<pr>: <one line>`, and stop. Temi decides; don't push past it.
4. **Passes:** `scripts/work merge <pr>`. It merges only once `verify`, `frontend` and `watchdog/review` pass on the current head and nothing is held. Then `send_message` `MERGED PR#<pr> #<n>`, and you're done.

## When you're stuck

`send_message` `BLOCKED #n: <one line: what, and what would unblock it>`, then end your turn. Typical reasons:
- A signup or anything only Temi can decide. Move that one criterion to a new issue (`gh api repos/AndySakov/omnimarket/issues -f title=… -f body=…`, labelled `needs-temi` and linked both ways), finish the rest, and close the original with your PR (D90).
- A spec contradiction. Say which D-entry, and what you'd propose.
- Something the issue didn't anticipate that changes its scope.

Never touch real funds, mainnet keys, signups or branch protection, and never post a watchdog verdict yourself.

## Issue notes

The order is set by the queue (`scripts/work next backend`). What follows is what each issue needs beyond its text.

| Issue | Watch for |
|---|---|
| #97 regression test fails on main | A CI job on PRs closing a `bug` issue; replay it on #57's history (`19e1874` vs `a09f080`). Update infra.md, CLAUDE.md's Review gate, and add a D-entry. Don't make it required yet: the issue says after two green fix PRs. |
| #98 criteria name their tests | Adds `.github/pull_request_template.md`; keep the "Acceptance criteria" table above as its shape. The job parses `cargo test` output from the same CI run. |
| #99 pinned real recording | Record 2–5 minutes live (`engine follow --check-every …`), choosing a stretch with a same-block event after a v3 bootstrap. Compress it; the test runs offline in under 30s. Document re-recording in observability.md. |
| #74 demo-slice D-entry | Docs only. A D-entry plus build-plan.md, CONTEXT.md (demo account, shadow balance) and frontend.md, per its criteria. Next free number. |
| #75 API contract v0 | Normally the frontend builder's (frontend.md). If you get it, the same notes apply. |
| #76 pricing | New `crates/pricing`. Metadata through the call worker's rate limits (D82). Measure Base quote-asset coverage into verification.md. pricing.md. |
| #77 trade records | `trades.base`, keyed by pool; both token orderings tested. data.md, indexer.md. |
| #91 pool state vs real blocks | A capture script plus committed fixtures, offline tests, and `proptest`. Unblocks #83. pricing.md. |
| #92 engine binary end to end | Move the wiring out of `main.rs` into library functions; the test uses CI's Kafka service and an S3-compatible store (`scripts/stack up` locally). |
| #78 API server | New `crates/api`, axum. Read models are pure functions of records, timed by block time, so a replay gives byte-identical output. frontend.md. Needs #75. |
| #79 status · #80 candles · #81 discovery | Each needs #78. Candles: backfill through the same rate limits, with no double count at the hand-off. All three replay identically. |
| #82 demo host | `needs-temi` (VM and domain signup). Don't start it until the label is gone. |
| #93 coverage report | Nightly, not required. infra.md. |
| #83 quotes · #84 shadow execution · #85 accounts | Stage 2. #84's state-override support on PublicNode is **(verify)**: test it first. #85's Privy half needs a Privy app: build the guest path and move Privy's criterion out (`needs-temi`) if there's still no app. |
| #86 triggers · #87 explanations | Stage 3. Exactly-once firing across a restart; never fire on provisional state. |
| #88 replay mode | Blocked through #42 → #40, #41 (M1); the queue follows that chain. |
| #89 safety | Needs Foundry (`contracts/`); install it from its GitHub release. A honeypot fixture and a taxed token in `forge test`. |
