# CLAUDE.md

OmniMarket is a multi-chain EVM trading terminal backend (Base first, then BNB Chain and MegaETH): live pricing, routing, fast execution and trigger orders. Read [CONTEXT.md](CONTEXT.md) for the vocabulary before anything else. Use its terms in code, commits and replies.

## Current mode

**Build mode**, since 2026-09-29. Work comes from GitHub issues. Current focus: the **demo sprint** (#62), with critical work on M0 and M1 ahead of it (D87). M0 is done.

Take your next issue in this order. The first rule that yields an issue wins:

1. **Your own open PRs.** Red CI, a merge conflict or a failed `watchdog/review` on a PR you opened comes before new work (see Review gate).
2. **`critical`, in any milestone.** Red CI on `main`; a bug that stops or corrupts the live read path (following, pool state, recording or replay); or a security problem. Oldest milestone first.
3. **The demo sprint.** Open issues labelled `demo` and `backend`, lowest demo stage first (each issue's first line names its stage), then lowest number. If the one you'd take is blocked by M1 issues, follow that chain of blockers to the first M1 issue that is unblocked and untaken, and take it.
4. **M1**, only when no demo issue is left to take.

At every step, skip issues that are assigned (the `frontend` ones are Jutin's), claimed by an open PR, or blocked by an open issue. Claim yours before your first commit. If nothing qualifies, stop and tell Temi. The queries and the claim step are in [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md#picking-the-next-issue).

A bug that meets rule 2's bar gets the `critical` label, and a comment saying which part of the bar it meets. Temi can remove the label.

## Where things are

| Need | Go to |
|---|---|
| What we decided and why | [docs/spec/decisions.md](docs/spec/decisions.md) |
| Milestones, repo layout, build rules | [docs/build-plan.md](docs/build-plan.md) |
| Subsystem specs | [docs/spec/README.md](docs/spec/README.md) index |
| Claims checked against sources | [docs/spec/verification.md](docs/spec/verification.md) |

## Decisions

- Every choice that changes behaviour or architecture gets a D-entry: the decision, the rejected options, and why.
- To change an existing decision, add an amendment note to it and a new entry. Never let code and a D-entry silently disagree: if they do, one is a bug, so fix it in the same change.
- Settle an empirical question by building a throwaway prototype, not by writing another D-entry.
- Mark anything unverified **(verify)**.

## Build-mode rules

These come from the build plan, and each should be enforced by a check rather than by memory:

1. Core logic is deterministic. Time, randomness, network, RPC and signing go only through the `det` traits. A lint or CI check enforces this from the first commit.
2. Every record carries lineage IDs. Schemas define them.
3. Every input is recordable: each `det` implementation has a recording wrapper.
4. Shadow mode is the default everywhere outside the real-funds demos.
5. Boring Rust: well-known crates, plain structs and enums, explicit error types. No clever generics or custom macros in core logic. Comment non-obvious ownership and async.
6. Engine and execution cores are single-threaded state machines. I/O runs on tasks around them.
7. Money-path operations are idempotent and safe to replay (firing IDs, nonce ledger, intents).

### Boring Rust, by example

```rust
// Bad: generic soup, a hidden wall clock, an opaque error.
fn check<O: AsRef<Order>, F: PriceFeed + ?Sized>(order: O, feed: &F) -> anyhow::Result<bool> {
    let now = std::time::SystemTime::now();
    // ...
}

// Good: concrete types, time handed in from det::Clock, an error the caller can match on.
pub enum CheckError {
    StalePrice { age_ms: u64 },
}

pub fn should_fire(order: &StopLoss, price: DisplayPrice, now_ms: u64) -> Result<bool, CheckError> {
    // ...
}
```

When an agent mistake repeats, fix it in this order: the architecture or types, then a lint rule, test or CI check, then a line in this file.

## Verification

M0 creates the workspace and the commands. Until then there is nothing to run. Once they exist, the definition of done for any code change is:

- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` and `cargo test --workspace` pass
- the simulated-clock replay test replays identically
- `forge test` passes for anything under `contracts/`
- any spec or D-entry the change touches is updated in the same commit

`scripts/verify.sh` runs the full repository checks. CI runs it, and the tracked git pre-commit hook runs the scoped fast checks in `scripts/verify-fast.sh` (run `scripts/setup.sh` once per clone). A Claude Code hook refuses `--no-verify` and, if the git hook isn't installed, runs verify itself. Fix the failure, never bypass it.

Run the real thing before claiming done, and say what you could not verify.

## When you change X, update Y

| Change | Update in the same commit |
|---|---|
| `proto/` | `docs/spec/data.md` (keys, topics); `docs/spec/frontend.md` if the API contract moves |
| `crates/det/`, `crates/sim/`, `loadtest/` | `docs/spec/observability.md` (replay, flight recorder); `docs/spec/loadtest.md` |
| `crates/chain-io/`, `crates/engine/` | `docs/spec/indexer.md`; `docs/spec/architecture.md` if a component boundary moves |
| `crates/venues/`, `crates/pricing/` | `docs/spec/pricing.md` |
| `crates/routing/`, `crates/safety/` | `docs/spec/routing.md` |
| `crates/triggers/` | `docs/spec/triggers.md` |
| `crates/execution/` | `docs/spec/execution.md`; `docs/spec/security.md` if signing or key power changes |
| `contracts/` | `docs/spec/routing.md`; `docs/spec/security.md` |
| `crates/api/` | `docs/spec/frontend.md` (API contract) |
| `crates/candles/`, `crates/history/` | `docs/spec/data.md` |
| `crates/watcher/`, `crates/telemetry/`, lineage or brakes | `docs/spec/observability.md` |
| `deploy/`, CI | `docs/spec/infra.md` |
| A latency budget | `docs/spec/slas.md` |
| A new domain term | `CONTEXT.md` |
| A resolved **(verify)** | `docs/spec/verification.md` |
| Milestone scope | `docs/build-plan.md` |

## How to work here

- When a step doesn't need input, keep going. Stop and ask only when blocked, or before anything destructive, anything that touches real funds or mainnet keys, or anything outside this repo.
- Questions are read-only: answer them without editing files.
- Keep changes to the task. List unrelated problems you notice as follow-ups instead of fixing them.
- Branch per task and open a PR. The PR title states the effect of the change; the body opens with the problem, then the fix.
- When compacting, preserve the goal, the done criteria, decisions made and open questions.

## Review gate

`main` merges only when two checks pass on the PR's head commit: CI's `verify` and `watchdog/review` (D81). A separate watchdog session reviews every PR against its issue, the D-entries and the update table above, then posts `watchdog/review` with its findings as a PR comment. Branch protection enforces this for admins too.

- After opening a PR or pushing to one, wait for the checks: `gh pr checks <n> --required --watch`. While you wait, you may start the next issue in the "Current mode" order on a new branch off `main`.
- On `failure`, read the latest watchdog comment (`gh pr view <n> --comments`), fix every blocking finding on the same branch, and push. Each push needs a fresh review.
- If you think a finding is wrong, reply on the PR saying why and leave it for Temi. Don't push past it.
- Never route around the gate: no `gh pr merge --admin`, no editing branch protection, no posting `watchdog/review` yourself.
- If `watchdog/review` hasn't appeared 30 minutes after a push, tell Temi. The watchdog may be down.

## Agent skills

### Issue tracker

GitHub Issues on `AndySakov/omnimarket`, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `CONTEXT.md` plus D-entries in `docs/spec/decisions.md` (no `docs/adr/`). See `docs/agents/domain.md`.
