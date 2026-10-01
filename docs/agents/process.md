# Process

How the work on OmniMarket is coordinated: who takes what, how it's reviewed and merged, and how parallel agent sessions stay out of each other's way. The decision log (`docs/spec/decisions.md`) records OmniMarket's behaviour and architecture; this file holds the rest. Change a section here in place, with the date and the reason, rather than adding a decision.

The first five sections were D-entries before this file existed, and keep their numbers so references to them still resolve.

<a id="d81"></a>

## D81 — PRs merge only after a watchdog review

*(Amended by D90: the watchdog posts its verdict as a PR comment, and the `watchdog-status` workflow turns it into the `watchdog/review` status; the watchdog runs on the build account; builders merge once `verify`, `frontend` and `watchdog/review` pass.)*

**Date:** 2026-09-30 · **Status:** Decided (process; from auditing M0 and M1)

**Decision:** A separate watchdog agent session reviews every PR before it merges. It checks the change against its issue, the D-entries and CLAUDE.md's update table, runs the tests, and posts a `watchdog/review` commit status on the PR's head commit (`pending`, then `success` or `failure`) with its findings as a PR comment. Branch protection on `main` requires `verify` and `watchdog/review`, and applies to admins. Each push needs a fresh review. The protocol is in CLAUDE.md.

**Found while auditing:** PRs #32 to #44 had no reviews. The builder merged each one 1 to 8 minutes after opening it, so CI was the only gate. #44 landed without the D-entry and `pricing.md` update CLAUDE.md requires (#48), and a follower stall on reorged-out blocks went unnoticed (#46).

**Rejected:**
- *Required approving reviews.* Every agent acts as the one GitHub account, and GitHub doesn't let an account approve its own PR.
- *A soft gate (the builder waits a while, then merges).* Relies on the builder following a rule it already skipped: CLAUDE.md asked for `/meta-review` before merging.
- *Review after merge.* Defects reach `main` first.
- *A paid CI review bot.* Free resources only for now.

**Why:** The builder moves faster than anyone can read its PRs. A gate that blocks the merge is the only review that reliably happens.

**Consequence:**
- When the watchdog is down, nothing merges. Temi can lift the gate by turning off admin enforcement on `main`.
- GitHub can't tell who posted a status, so the builder's token could post `watchdog/review` itself; only CLAUDE.md forbids it. Binding the required check to a GitHub App that only the watchdog holds closes this gap. **(Follow-up: needs Temi to create the App.)**

---

<a id="d86"></a>

## D86 — The commit gate splits by path, and fails closed

**Date:** 2026-09-30 · **Status:** Decided (process; from the terminal foundation, #56)

**Decision:** The git pre-commit hook runs `scripts/verify-fast.sh`. A commit whose staged files are all under `web/terminal/` runs the frontend's fast checks (`npm run verify:fast`: typecheck, lint, unit tests). Every other commit, an empty one included, runs `scripts/verify.sh` as before. `verify.sh` stays backend-only (Rust, contracts, proto) and needs no Node. CI runs the frontend's full checks (`npm run verify:pr`) in its own `frontend` job, beside `verify`.

**Found while reviewing:** #56's first version added the frontend to `verify.sh`, so every backend commit and CI's `verify` job needed Node and `web/terminal/node_modules`. Its fast hook ran the backend checks only when a staged path matched a fixed list, so a commit touching only `clippy.toml` (D73) or `.github/` ran nothing and passed.

**Rejected:**
- *The frontend inside `verify.sh`.* Every backend clone needs `npm ci` before it can commit, and CI installs Node for `verify` as well as for `frontend`.
- *Backend checks only for a list of backend paths.* Fails open: a path nobody listed skips the gate.
- *Full `verify.sh` on every commit, frontend commits included.* Minutes of cargo on each frontend commit, for checks that can't see the frontend.

**Why:** Each side's gate needs only its own toolchain, and anything not provably frontend-only gets the full backend checks, so the local gate fails closed.

**Consequence:**
- A commit that mixes frontend and other files runs only `verify.sh` locally; CI's `frontend` job checks its frontend half.
- `frontend` isn't a required check on `main` yet, so a red `frontend` job doesn't block a merge. **(Follow-up: Temi adds it to branch protection.)**
- The Claude Code hook still runs the full `verify.sh` when the git hook isn't installed: it runs before the files are necessarily staged.

---

<a id="d87"></a>

## D87 — Work order: critical work first, then the demo sprint, then M1

*(Amended by D90: `scripts/work next` applies this order over REST; the build account's frontend builder takes Jutin's frontend issues while he's away, claimed by the `wip` label instead of reassignment.)*

*(Amended by D96: the sprint's scope and shortcuts are recorded there.)*

**Date:** 2026-09-30 · **Status:** Decided (process; Temi's priority call)

**Decision:** Agents take work in a fixed order, recorded in CLAUDE.md's "Current mode":
1. Stalled open PRs (red CI, a conflict or a failed `watchdog/review`), whoever opened them, oldest first. A claim on one lapses after 2 hours without a push.
2. Issues labelled `critical`, in any milestone.
3. Backend issues in the demo sprint (#62), lowest demo stage first. M1 issues in the blocking chain of the next demo issue count as demo work.
4. The rest of M1, only when no demo issue is left to take.

`critical` means red CI on `main`, a bug that stops or corrupts the live read path (following, pool state, recording or replay), or a security problem. An agent that applies the label says which part of the bar the issue meets. An issue is claimed by an assignee and a claim comment before the first commit, and it counts as taken once it has an assignee or an open PR that closes it. That lets several sessions pick work at the same time without a coordinator.

**Rejected:**
- *The demo sprint only, until it ships.* A stalled follower or a red `main` would wait behind features that depend on them. The demo shows the M0 and M1 engine, so a critical bug there breaks the demo too.
- *A fixed share of sessions per track (e.g. one in three on M1).* Sessions don't see what the others picked, so nothing could enforce the share.
- *Milestone order: finish M1, then M2.* It delays anything showable by weeks, and M1's open issues (#40, #41) don't affect the demo until replay mode (#88).

**Why:** The demo is the priority, and the engine it runs on must stay correct. A written order lets every session choose the same way. Rule 1 covers every stalled PR, not only a session's own: sessions restart and share one GitHub account, so none can know which PRs it opened. Scoping it to "your own" left #57, the fix for the one critical bug (#46), unattended while #46 counted as taken.

**Consequence:**
- Non-critical M1 work waits until the demo sprint has no backend issue left to take. When the sprint reaches replay mode (#88), its chain pulls #42 forward, and through #42, #40 and #41. #46 is `critical`, since the follower stalls forever.
- The sprint's scope and shortcuts are recorded separately (#74).
- When the sprint ends, this entry gets an amendment note and CLAUDE.md's current mode returns to milestone order.

---

<a id="d90"></a>

## D90 — Build work runs on a second account's cloud sessions, under an orchestrator

*(Amended by D93: Jutin builds the frontend issues labelled `jutin` alongside the farm; an open PR saying `Part of #n` also takes an issue; the watchdog reviews every open PR, whoever opened it.)*

**Date:** 2026-10-01 · **Status:** Decided by Temi (process)

**Decision:** Most build work moves to Claude Code cloud sessions on a second Claude account (the **build account**), whose GitHub connection acts as `AndySakov`, like Temi's own sessions. Temi steers it through one long-lived **orchestrator** session on that account, which starts and tracks the other sessions. The protocol is in `docs/agents/handoff/`.
- **Roles.** One backend builder and one frontend builder at a time, one session per issue, and one watchdog session per PR (D81's separate reviewer). The orchestrator picks work with `scripts/work next`, which applies D87's order, starts sessions, dispatches reviews and reports to Temi. It writes no code.
- **The gate.** The watchdog posts its verdict as a PR comment whose first line names the head commit and passes or fails. The `watchdog-status` workflow turns that line into the `watchdog/review` commit status, linked to the comment, and only for the PR's current head. Branch protection is unchanged.
- **Merging.** The builder merges with a merge commit (`scripts/work merge`), which refuses unless `verify`, `frontend` and `watchdog/review` all pass on the head, there's no conflict and nothing is labelled `hold`. `frontend` gates merges this way, though branch protection doesn't require it yet (D86).
- **Claims.** `scripts/work claim` adds the `wip` label, assigns the claimer if nobody is assigned, and comments with the session link. While Jutin is away, the frontend builder takes his frontend issues and leaves him assigned, so he sees what's left when he's back; `wip` is the claim there. An acceptance criterion that names Jutin's review is met by Temi's review while he's away, and Jutin is tagged for a look.
- **Partial work.** An acceptance criterion that can't be met from the repo (a signup only Temi can do; a frontend slice waiting on its backend) moves to a new issue linked both ways, labelled `needs-temi` or blocked by the backend issue, and the original closes with the rest.
- **Temi's brakes.** The `hold` label on an issue or PR stops agents taking or merging it. `needs-temi` marks work only Temi can unblock.
- **Cloud sessions are readied by a SessionStart hook** (`.claude/hooks/session-start.sh`): dockerd, the git hooks, buf, npm deps, Playwright's Chromium (or the preinstalled one), and a cargo build warmed in the background.

**Found while setting up (2026-10-01):**
- In cloud sessions GitHub's GraphQL API is refused ("GraphQL is not available from Claude Code sessions"), so `gh issue list`, `gh pr view` and `gh pr checks` fail. REST through `gh api` works.
- Commit-status and check-run writes are refused by the session proxy ("Write access to this GitHub API path is not permitted"), whatever the network level, token or GitHub account connected (a PAT stored as an environment credential changed nothing). Comments, labels, assignees, opening and merging PRs, and workflow dispatch are allowed.
- A cold `scripts/verify.sh` takes 3.5 minutes on a cloud container, and Docker's daemon isn't running at start.
- A session started by another session receives its brief as an automated message. It followed an ordinary work brief, but refused one that read like a credential probe.

**Rejected:**
- *A PAT for `AndySakov` in the build account's environment.* The proxy ignored it, and still blocks status writes and GraphQL for every account.
- *A watchdog that dispatches a workflow with the verdict as inputs.* Two steps that can disagree; a status from a comment can't exist without the findings it links to.
- *Keeping the watchdog on Temi's account.* Temi's usage limits would cap how many PRs merge a day.
- *Builders picking their own issues.* Two builders could race for one issue; one dispatcher can't.
- *Reassigning Jutin's issues.* Temi wants Jutin to see what's left when he's back.

**Why:** The build account has the budget and Temi doesn't, so the token-heavy work (building, reviewing) runs there and Temi spends his limits only on steering and decisions. The gate and the work order stay as they were; only their mechanics change to fit what cloud sessions can do.

**Consequence:**
- The gate has the same trust gap D81 records: any session with write access could post a fake verdict comment, as it could post a status before. CLAUDE.md forbids it, and the status now links to the comment that set it.
- Both Claude accounts act on GitHub as `AndySakov`, so GitHub alone can't tell their work apart; the claim comment's session link can.
- When the build account's budget runs out, the orchestrator stops starting sessions, and Temi's watchdog (or Temi) reviews again.
- When Jutin is back, frontend issues return to him one by one, as he takes them.

---

<a id="d93"></a>

## D93 — Jutin and the build account share the frontend track, issue by issue

**Date:** 2026-10-01 · **Status:** Decided by Temi (process; amends D90)

**Decision:** Jutin is back at reduced capacity, working from his fork with Codex. He and the build account's frontend builder share the frontend track:
- **Jutin claims an issue himself** by commenting `Taking this` from his GitHub account (`Dropping this` releases it). Agent sessions act as `AndySakov`, so only he can make that claim. Temi or the orchestrator can also label an issue `jutin` (`JUTIN TAKES #n`). `scripts/work` never hands either to an agent session, and `JUTIN BACK` still returns the whole track to him.
- **Any open PR that says `Closes #n` or `Part of #n` takes issue `#n`, whoever opened it.** That's how the queue sees work from Jutin's fork.
- **The watchdog reviews every open PR whose CI is green,** Jutin's included, so his PRs can pass the same gate (D81).
- **One data layer.** Frontend work builds on the generated contract types (#75) and the shared client (#63); a slice that overlaps another person's open PR builds on it and raises the overlap there.
- **`AGENTS.md`** at the root points Codex at CLAUDE.md and these rules, and `web/terminal/docs/collaboration-workflow.md` carries them for Jutin.

**Found when Jutin returned (2026-10-01):** his PR #105 built #64 but said "Issue 64" rather than `Closes #64`, so the queue would have handed #64 to the farm once #63 merged. It also added a hand-written stream client overlapping #63's. Nothing would have reviewed it: the orchestrator watchdogged only its own sessions' PRs. Codex doesn't read CLAUDE.md, and the repo had no `AGENTS.md`.

**Rejected:**
- *`hold` for Jutin's issues.* `hold` is Temi's brake, and `scripts/work merge` refuses PRs that close a held issue; Jutin's own claim needs a label of its own.
- *Reassigning issues away from Jutin.* He stays assigned to every frontend issue (D90), so the assignee can't say who's building one.
- *Only a label, applied by Temi.* Every claim would wait on Temi. Jutin's comment is his own, can't come from an agent session, and his agent can post it unprompted.

**Why:** The farm picks frontend work by rule, so Jutin's work has to be visible to that rule, and his PRs need the same review path to merge.

**Consequence:**
- Run `scripts/work labels` once to create `jutin`; issues held for Jutin before this (#64) move from `hold` to `jutin`.
- A PR body without `Closes #n` or `Part of #n` is invisible to the queue, from anyone.

---

## Build-account limits

The orchestrator runs up to three backend and three frontend builders at once, with at most four open PRs per track and four watchdogs reviewing at once (Temi, 2026-10-01; D90 started at one builder per track, two open PRs and two watchdogs). Each new builder's brief names its siblings. Spend rises with the number of busy builders, so `BUDGET $N` reports matter more; the budget thresholds in `handoff/orchestrator.md` are unchanged.

---

## Coordination between workers

Up to six builders and Jutin work at once. On 2026-10-01 two PRs opened four seconds apart both took D96 by counting `main` and open PRs, and two others edited `scripts/work` without knowing. So:

- **D-numbers come from a ledger.** One issue, labelled `d-ledger`, holds a comment per reserved number. `scripts/work reserve-d "<title>"` takes the next number above everything on `main`, in open PRs, in the ledger and in the local checkout, posts it, and re-reads the ledger: the earliest comment for a number holds it, and a session that lost the race takes the next one. Nobody counts by hand. A number reserved and never used is a gap; gaps are fine.
- **Overlaps are found before they conflict.** Builders push at every green commit, and `scripts/work overlaps` compares a branch's files with every other live branch and open PR, naming the session holding each (claim comments record the branch beside the session link). Builders run it after their first push and before every later push.
- **Builders talk to each other.** An overlap is settled by `send_message` between the two sessions: who changes what, and who merges first. The one that merges second merges `main` in. A change to a shared contract (`proto/`, generated API types, public crate APIs, workspace dependencies, `scripts/work`, CLAUDE.md) goes to the orchestrator as `CONTRACT #n: …`, and it relays the line to every running builder.

Rejected: the orchestrator assigning numbers (a round trip per entry, and Jutin's agent can't reach it); renumbering at merge (a review cycle each time); a reserved-numbers file in the repo (reserving would need a merge through the gate).
