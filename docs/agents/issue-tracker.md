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

The order is CLAUDE.md's "Current mode" (D87). These are the queries behind each rule. Several sessions run at once under the one GitHub account, so an issue counts as taken once it has an assignee or an open PR that closes it.

1. **Your own open PRs** are the ones this session opened; you know them. Red CI, a conflict or a failed `watchdog/review` comes first.
2. **Critical:**
   ```bash
   gh issue list --state open --label critical --json number,title,milestone,assignees
   ```
3. **Demo sprint** (backend only; the `frontend` issues are assigned to Jutin):
   ```bash
   gh issue list --state open --label demo --label backend --limit 100 \
     --json number,title,assignees,body \
     --jq '[.[] | select(.assignees | length == 0)
            | {number, title, stage: (.body | capture("Demo stage (?<s>[0-9])").s)}]
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
- **Claim it** before your first commit, in this order:
  ```bash
  gh issue edit <n> --add-assignee @me
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
