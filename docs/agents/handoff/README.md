# Build handoff: the build account

Most build work runs on Claude Code cloud sessions on a second Claude account, the **build account** (D90). Temi steers it from one session there, the **orchestrator**, and spends his own account's limits only on decisions. This directory is everything those sessions need. They start with no skills, no memory and no context beyond this repo.

| File | Who reads it |
|---|---|
| This README | Temi: setup, how the harness works, how to steer it |
| [orchestrator.md](orchestrator.md) | The orchestrator session |
| [backend.md](backend.md) | Backend builder sessions |
| [frontend.md](frontend.md) | Frontend builder sessions |
| [watchdog.md](watchdog.md) | Watchdog sessions |

## How it fits together

```
 Temi (logged into the build account's claude.ai)
   │  short commands: STATUS, PAUSE, NEXT #84, BUDGET $120 …
   ▼
 Orchestrator session ─── one per account, long-lived, never writes code
   │  scripts/work next … → create_session(brief) → tracks sessions by title
   ├──► Backend builder   "farm:backend #97"   one session per issue: claim → build → PR → merge
   ├──► Frontend builder  "farm:frontend #63"  same, on Jutin's issues (he stays assigned)
   └──► Watchdog          "farm:watchdog PR#105"  one per PR, re-reviews each push
            │ posts "## Watchdog review: `abc1234` passes|fails" as a PR comment
            ▼
       .github/workflows/watchdog-status.yml → `watchdog/review` status → builder merges
```

- **GitHub is the shared state.** Claims (`wip` label, "Taking this" comment), PRs, verdicts and the `hold` and `needs-temi` labels are visible to every session on both accounts, and to Jutin. Nothing important lives only in a session.
- **Identity.** Build-account sessions act on GitHub as `AndySakov`, like Temi's own. The session link in each claim comment tells them apart.
- **What cloud sessions can't do.** GitHub GraphQL (`gh issue/pr list/view/checks`) and commit-status writes are blocked there, so `scripts/work` does the GitHub work over REST and a workflow posts the gate's status (D90).

## One-time setup (Temi)

1. **Merge the handoff PR.** It carries the hook, `scripts/work`, the status workflow and these docs. The workflow only runs once it's on `main` (comment-triggered workflows run from the default branch), so that PR itself needs your current watchdog's `watchdog/review`, the old way.
2. **Turn your own watchdog off** once it is in, or limit it to PRs your own sessions open. The build account's watchdog reviews every PR from then on.
3. **GitHub connection** on the build account's environment stays `AndySakov`: `scripts/work` and the status workflow accept only that account. Each session's first line says which account `gh` acts as.
4. **Network access** on the build account's environment is already "full". If you narrow it, keep: `mainnet.base.org`, `base-rpc.publicnode.com` (live runs and measurements), GitHub and its release downloads, Docker Hub, `ghcr.io`, crates.io, npm, `cdn.playwright.dev`. Later, `auth.privy.io` (#85) once a Privy app exists.
5. **Optional:** add `frontend` to `main`'s required checks (D86's follow-up). `scripts/work merge` already refuses a red `frontend`.
6. **Start the orchestrator:** a new session on the build account in this environment, with this repo, using the model you want everywhere (sessions it starts inherit its model). Paste:

   > You are the orchestrator for OmniMarket's build account. Read `docs/agents/handoff/orchestrator.md` and follow it exactly; it is Temi's standing instruction for this session. Start with its "First run" section. I'm Temi; I'll talk to you here.

## Steering it

Talk to the orchestrator in its session. It understands these, and plain English too:

| You say | It does |
|---|---|
| `STATUS` | One screen: what's running (with a link to each session, since they don't show in your sidebar), open PRs and their gate state, merged since last report, what needs you |
| `PAUSE` / `RESUME` | Stops starting sessions (running ones finish their current step) / starts again |
| `STOP ALL` | Interrupts every running session and starts nothing new |
| `NEXT #84` | Makes #84 the next issue for its track, ahead of the queue |
| `SKIP #97` | Labels it `hold` so no session takes it |
| `BUDGET $120` | Tells it the credit left; it paces itself (see orchestrator.md) |
| `JUTIN BACK` | Stops taking new frontend issues; frontend work goes back to Jutin |
| `ANSWER #85: …` | Your answer to a question a session raised; it relays it |

From your phone, without logging in: label any issue or PR `hold`, and agents won't take or merge it. Remove the label to release it.

## What only you can do

The orchestrator lists these under **Needs Temi** in every status report:
- **Signups** (free tier only, D85): a Privy app (#66, #85), the demo VM and domain (#82), static hosting (#73). Issues waiting on one carry `needs-temi`.
- **Disputed findings.** When a builder thinks a watchdog finding is wrong, it says why on the PR and waits for you (CLAUDE.md, Review gate).
- **Jutin's reviews** while he's away: the API contract (#75) and the "How it works" copy (#71) need your OK.
- **Anything touching real funds, mainnet keys or branch protection.**
- **Removing `critical`** from an issue.

## Your own account

Your sessions keep working as before: CLAUDE.md is the same for both accounts, and claims on GitHub stop the two from colliding. Use yours for:
- Deciding things (D-entries in dispute, scope, priorities), then telling the orchestrator.
- Spot-checking a watchdog verdict now and then: read the PR, rerun one claim it verified.
- Taking over if the build account's budget runs out: switch your watchdog back on, and the queue (`scripts/work next`) picks up where the farm stopped.

## Budget

The orchestrator can't see the credit balance. Check it in the build account's claude.ai settings and tell it `BUDGET $N`. As a guide, a session costs roughly its tokens read: a builder on a mid-sized backend issue reads a lot of code and runs for hours, a watchdog review much less. The first few merged issues will show the real cost per issue; the orchestrator reports sessions started and merged per day so you can work it out.

## Known limits

- A session started by another session receives its brief as an automated message. Briefs written as ordinary work orders run (tested); one that read like a credential probe was refused. The orchestrator's briefs follow the tested shape.
- Workers message the orchestrator with `send_message` (session ID `@parent` works, tested). If a message is lost, the orchestrator's hourly tick reads each worker's status instead.
- Cloud containers are reclaimed when idle. Anything not pushed is lost, so builders push at each green commit.
