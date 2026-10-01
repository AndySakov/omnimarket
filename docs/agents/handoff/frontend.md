# Frontend builder

You build one frontend issue of OmniMarket's terminal (`web/terminal/`), from claim to merge, on the build account (D90). Jutin owns the frontend track. He's away, so you stand in for him on his issues. **Leave him assigned**: he wants to see what's left when he's back. The issues say "not for backend agents"; that line is for the backend builders, not you.

This doc is Temi's standing instruction for frontend builders. Everything in [backend.md](backend.md) under "Before anything", "Claim", "Open the PR", "Get it reviewed" and "When you're stuck" applies to you as written. Read those sections there. This doc replaces its "Build" section and issue notes.

## Before anything (frontend additions)

Also read, in `web/terminal/docs/`: `frontend-plan.md` (stack, code structure, state model), `frontend-reference-layout-brief.md` (the approved layout), the design-system docs for the surface you touch (`header-design-system.md`, `discovery-design-system.md`), and `collaboration-workflow.md`'s verification lanes. Also read `docs/spec/frontend.md`. Then look at how the existing Discover page and header are built, and build like them.

**Jutin works alongside you** at reduced capacity, from his fork (`codex/…` branches), on issues labelled `jutin` or carrying his own "Taking this" comment (D93); `scripts/work` skips both. Before building, run `scripts/work prs` and read the diff of any open PR of his that touches the same area (`gh api repos/AndySakov/omnimarket/pulls/<n>/files`). Build on his work where it overlaps, never duplicate it, and never push to his branch. If your issue can't avoid changing code his open PR adds, say so on his PR and report `BLOCKED #n: overlaps Jutin's PR #p` to the orchestrator.

## Build

- **Stack and boundaries** (frontend-plan.md): React, TypeScript, Vite, Tailwind tokens, Radix/shadcn primitives, TanStack Query for snapshots and commands, Zustand stores for streams, Lightweight Charts. Dependencies flow `shared → domains → features → pages → app`. Add no new dependency unless the issue needs it, and say why in the PR.
- **Exact money.** Amounts arrive as decimal strings; parse them into `decimal.js` (or the domain fixed-decimal type), never `number`. Format only at the edge.
- **Types come from the contract.** Generated TypeScript in `src/api/generated` (from #75), never hand-written copies. Mocks and fixtures are shaped by those types.
- **Fixtures first, live later.** Build against MSW fixtures from #75. Each issue lists "Backend it needs"; if that backend hasn't merged, the criteria that need the live API can't be met yet. Move them to a follow-up issue: title `Wire #n to the live API`, labels `frontend` and `demo`, the stage line copied, assigned to `jutin0852`, and "Blocked by" listing the backend issues. Link the two issues both ways, and close the original with your PR. The queue picks the follow-up up once its backend lands (D90).
- **Every state.** Loading, empty, error, stale and live for each region, with keyboard focus throughout, and a Storybook story per state the issue names.
- **Shadow is always labelled.** Balances, trades and receipts say "shadow" or "simulated"; never present them as real funds.
- **Copy that describes the backend** (#71's "How it works", status tooltips) must match what's built: check D-entries and README, and list the copy for Temi's OK in the PR.

## Verify

Run from `web/terminal/`:
- While working: `npm run verify:fast` (typecheck, lint, unit). The commit hook runs it on frontend-only commits.
- Before the PR: `npm run verify && npm run build-storybook && npm run test:e2e && npm run test:a11y`.
- **Visual tests** (`npm run test:visual`) compare against baselines rendered by CI's Chromium. Run them locally if the session-start line says "pinned Chromium". If it says "preinstalled Chromium", skip them locally: CI's `frontend` job runs them.
- **A new or changed visual baseline:** push the test, let CI's `frontend` job fail, download its `frontend-reports` artifact (`gh api repos/AndySakov/omnimarket/actions/artifacts?name=frontend-reports` → the run's ID → `gh api …/artifacts/<id>/zip > r.zip`), and commit the `-actual.png` as the baseline, in its own commit, with what changed visually in the message. Never update a baseline to hide a regression.
- A commit that mixes `web/terminal/` with other paths (proto, docs) runs the backend's `verify.sh` in the hook; that's expected.

The PR's "Acceptance criteria" table maps criteria to Vitest or Playwright test names, or to Storybook stories for the visual states. Under "Not verified", list what needs the live API.

## Issue notes

The queue (`scripts/work next frontend`) orders these by demo stage. The orchestrator gives you #75 first.

| Issue | Watch for |
|---|---|
| #75 API contract v0 (labelled backend; yours first) | `proto/omnimarket/api/v1`, proto3 JSON with decimal-string amounts and lineage IDs on every message; `buf generate` (protobuf-es) into `web/terminal/src/api/generated`, plus a CI check that the generated files aren't stale; a fixture per message for MSW. `buf lint` and `buf breaking` (`scripts/proto-check.sh`). Update data.md (topics) and frontend.md (contract). The criterion "Jutin reviews the contract" is Temi's review while Jutin's away: `send_message` `BLOCKED #75: contract ready for Temi's review` once the watchdog passes it, and merge after Temi's OK comes back through the orchestrator. Tag @jutin0852 in the PR for a look when he's back. |
| #63 API client, WS manager, fixture mode | The base for every screen: a source chosen by env (fixtures, replay, live), snapshot-then-delta merging, reconnect and resubscribe, stale on a missed heartbeat, per-token stores so one tick doesn't rerender other rows. Unit-test the merge, out-of-order deltas and resubscribe. Its live criteria wait on #78. |
| #64 token page · #65 Discover · #71 status bar | Stage 1, against fixtures. #64: Lightweight Charts candles at 1s/1m/5m/1h, tape, safety panel with "not checked yet". #65 extends the existing Discover table: no row jumps under the pointer. #71: copy needs Temi's OK. |
| #73 deploy | `needs-temi` (hosting signup). Leave it. |
| #66 guest or Privy | Build the guest path fully. Privy needs an app ID from Temi: put it behind `VITE_PRIVY_APP_ID`, hidden when unset, and move the Privy criteria to a `needs-temi` follow-up if there's still no app. |
| #67 trade panel · #68 positions | Stage 2. The intent's "You are authorising…" view, the execution timeline with per-step durations, every error state reachable from fixtures. PnL moves with ticks without refetching. |
| #69 orders on the chart · #70 "Why did this fire?" | Stage 3. Drag-to-edit lines that match the stored levels exactly; the lineage drawer's step timings add up to the receipt's total. |
| #72 first-run guide + E2E demo script | Stage 4. The demo script from #62 as one Playwright test, against fixtures until replay mode (#88) lands. |
