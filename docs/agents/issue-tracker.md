# Issue tracker: GitHub

Issues and specs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --comments`, filtering comments by `jq` and also fetching labels.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repo from `git remote -v`; `gh` does this automatically when run inside a clone.

## Picking the next issue

The order is CLAUDE.md's "Current mode" (D87). `scripts/work next backend|frontend` applies it and prints why it skipped the rest; the queries below are what it does. Several sessions run at once under the one GitHub account, from Temi's Claude account and the build account (D90), so an issue counts as taken once it has an assignee, the `wip` or `jutin` label, a `Taking this` comment from Jutin's own account, or an open PR that says `Closes #<n>` or `Part of #<n>`, from any author (D93). The queue matches `Part of #<n>` anywhere in a PR body, so write it only for an issue the PR works on: "part of #63's design" in prose would take #63.

`scripts/work next frontend` can't tell which account runs it: only the build account's frontend builder calls it while Jutin is away (D90).

**In cloud sessions, use `scripts/work`.** GitHub's GraphQL API is blocked there, so `gh issue list`, `gh issue view`, `gh pr list`, `gh pr view` and `gh pr checks` fail; `gh api` (REST) works.

| Instead of | Use |
|---|---|
| `gh issue view <n> --comments`, `gh pr view <n> --comments` | `scripts/work show <n>` |
| `gh pr list` with checks | `scripts/work prs` |
| `gh pr checks <n>` | `scripts/work checks <n>` |
| the claim commands below | `scripts/work claim <n>` |
| `gh pr create` | `scripts/work open-pr <title> <body-file>` |
| `gh pr merge` | `scripts/work merge <n>` |
| counting D-entries by hand | `scripts/work reserve-d "<title>"` (the D-number ledger, D98) |
| — | `scripts/work overlaps`: other live branches and PRs touching this branch's files, and who holds them |
| `gh issue comment <n> --body …` | `gh api repos/AndySakov/omnimarket/issues/<n>/comments -f body=…` |

1. **Stalled open PRs.** Any open PR from this account, not only ones this session remembers opening: sessions restart and share the account. List them oldest first, with their checks:
   ```bash
   gh pr list --state open --author AndySakov --json number,title,createdAt,mergeable,statusCheckRollup,updatedAt \
     --jq 'sort_by(.createdAt) | .[] | {number, title, mergeable,
            failing: [.statusCheckRollup[] | select((.conclusion // .state) == "FAILURE") | (.name // .context)]}'
   ```
   A PR is stalled if a check is failing (including `watchdog/review`) or `mergeable` is `CONFLICTING`. Skip it if another session claimed it (a "Taking this" comment) or pushed to it in the last 2 hours. After that, the claim has lapsed. Claim it the same way as an issue, with a comment on the PR, then fix it on its own branch.
2. **Critical:**
   ```bash
   gh issue list --state open --label critical --json number,title,milestone,assignees
   ```
3. **Demo sprint** (backend; the `frontend` issues are Jutin's, and while he's away only the build account's frontend builder takes them, D90):
   ```bash
   gh issue list --state open --label demo --label backend --limit 100 \
     --json number,title,assignees,body \
     --jq '[.[] | select(.assignees | length == 0)
            | {number, title, stage: ((.body | capture("Demo stage (?<s>[0-9])")?) // {s: "9"} | .s)}]
           | sort_by(.stage, .number)'
   ```
4. **M1:**
   ```bash
   gh issue list --state open --milestone M1 --json number,title,assignees
   ```

For each candidate, in order:

- **Blocked?** Look at every issue under `## Blocked by` in its body, and at `issue_dependencies_summary.blocked_by` from `gh api repos/AndySakov/omnimarket/issues/<n>`. If any blocker is open, move on. At rule 3, if an open blocker is an M1 issue, follow the chain through its own `## Blocked by` to the first M1 issue that is unblocked and untaken, and take that. For example, #88 → #42 → #40 or #41.
- **Taken?** Skip it if it has an assignee, or if an open PR closes it:
  ```bash
  gh pr list --state open --search "<n> in:body" --json number,title,body
  ```
  Read the matches: only a `Closes #<n>` (or `Fixes`) line counts.
  An issue whose closing PR is stalled isn't picked here: rule 1 picks up the PR.
- **Claim it** before your first commit: `scripts/work claim <n>`, which does this:
  ```bash
  gh issue edit <n> --add-label wip
  gh issue edit <n> --add-assignee @me   # only if nobody is assigned: Jutin stays on his issues
  gh issue comment <n> --body "Taking this: <session link>"
  ```
  Then put `Closes #<n>` in the PR body. If another session's claim comment appears first, back off and pick again.

If a rule yields nothing, go to the next one. If none does, stop and tell Temi what is blocked and on what.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

When set to `yes`, PRs run through the same labels and states as issues, using the `gh pr` equivalents:

- **Read a PR**: `gh pr view <number> --comments` and `gh pr diff <number>` for the diff.
- **List external PRs for triage**: `gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments` then keep only `authorAssociation` of `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, or `NONE` (drop `OWNER`/`MEMBER`/`COLLABORATOR`).
- **Comment / label / close**: `gh pr comment`, `gh pr edit --add-label`/`--remove-label`, `gh pr close`.

GitHub shares one number space across issues and PRs, so a bare `#42` may be either: resolve with `gh pr view 42` and fall back to `gh issue view 42`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single issue with **child** issues as tickets.

- **Map**: a single issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body. `gh issue create --label wayfinder:map`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue (`gh api` on the sub-issues endpoint). Where sub-issues aren't enabled, add the child to a task list in the map body and put `Part of #<map>` at the top of the child body. Labels: `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, the ticket is assigned to the driving dev.
- **Blocking**: GitHub's **native issue dependencies**, the canonical, UI-visible representation. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where `<blocker-db-id>` is the blocker's numeric **database id** (`gh api repos/<owner>/<repo>/issues/<n> --jq .id`, _not_ the `#number` or `node_id`). GitHub reports `issue_dependencies_summary.blocked_by` (open blockers only, the live gate). Where dependencies aren't available, fall back to a `Blocked by: #<n>, #<n>` line at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Frontier query**: list the map's open children (`gh issue list --state open`, scoped to the map's sub-issues / task list), drop any with an open blocker (`issue_dependencies_summary.blocked_by > 0`, or an open issue in the `Blocked by` line) or an assignee; first in map order wins.
- **Claim**: `gh issue edit <n> --add-assignee @me`, the session's first write.
- **Resolve**: `gh issue comment <n> --body "<answer>"`, then `gh issue close <n>`, then append a context pointer (gist + link) to the map's Decisions-so-far.
