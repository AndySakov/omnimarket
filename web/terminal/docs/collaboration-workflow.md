# Frontend Collaboration Workflow

This is the working agreement for Jutin's frontend work in the shared
OmniMarket repository.

## Repositories

- Shared source repository: `AndySakov/omnimarket`
- Jutin's public fork: `jutin0852/omnimarket`
- Local `upstream`: shared source repository for updates
- Local `origin`: Jutin's fork for personal branches and pull requests

The frontend branch is personal work in the fork until the project collaborator
reviews and merges the pull request into the shared repository.

## Remote policy

```text
origin   → https://github.com/jutin0852/omnimarket.git
upstream → https://github.com/AndySakov/omnimarket.git
```

The local repository should fetch from both remotes, but direct pushes to
`upstream` remain disabled. This makes an accidental push to the shared source
repository harder.

## Standard task flow

Start each task from the current shared branch:

```powershell
git switch main
git pull upstream main
git switch -c codex/<short-task-name>
```

Make one focused change per branch. Keep unrelated formatting, generated files
and refactors out of the branch.

### Issue definition

Before creating a branch, the issue records the user goal, primary action,
reference screen, in-scope and explicitly out-of-scope behavior, required
loading/empty/error states, API or fixture assumptions, and acceptance criteria.
Use one issue and one branch per vertical slice, for example:

```text
codex/discovery-foundation
codex/discovery-token-rows
codex/discovery-filters
codex/token-workspace
codex/trade-review
```

### Design gate

Do not start implementation until the slice has a reference screenshot audit,
measured layout notes, component tree, design tokens, desktop/mobile wireframes,
state matrix, and fixture data shape. For Discover, the gate must lock row
height, column widths, filter rail width, stream layout, action-column behavior,
mobile transformation, token-image policy, and sparkline policy.

Before publishing, run the relevant lint, typecheck, tests, build and visual
checks. For UI changes, capture desktop and mobile screenshots for review.

After explicit approval to publish:

```powershell
git add <focused-files>
git commit -m "Build <specific frontend outcome>"
git push -u origin codex/<short-task-name>
```

Open a pull request from the fork branch to `AndySakov/omnimarket:main`.

## Pull request format

```text
Problem: What frontend gap or user problem is being addressed?
Fix: What changed and why?
Verification: Commands, screenshots and flows checked.
Scope/risks: What is mocked, deferred or needs backend review?
```

The backend collaborator reviews API assumptions, shared contract changes,
security implications and product scope. Jutin owns the frontend implementation
and responds to review with focused follow-up commits. Do not force-push or merge
the branch without agreement.

## Frontend verification lanes

The frontend has two verification speeds so feedback stays fast without allowing
a green PR to skip UI coverage.

Fast local checks run from the tracked pre-commit hook:

```text
typecheck → lint → unit tests
```

Run the same lane directly with:

```bash
cd web/terminal
npm run verify:fast
```

The full PR lane runs:

```text
typecheck → lint → unit tests → production build → Storybook build
  → Playwright functional flows → visual screenshots → accessibility checks
```

Run it locally before requesting review:

```bash
cd web/terminal
npm run verify:pr
```

Visual checks use fixed viewport sizes, deterministic fixtures, and disabled
motion. Baseline changes are intentional visual changes and should be reviewed
in their own focused commit; do not update snapshots to hide a regression.

Track each frontend slice through:

```text
Planned → Design-ready → Mock-ready → Implemented → Verified → PR-ready → Merged
```

`Verified` requires typecheck, lint, relevant tests, desktop and mobile
screenshots, accessibility review, state coverage, and documented API/mock
assumptions.

## State and API boundary

Keep state ownership explicit so high-frequency market updates do not rerender
the entire terminal:

```text
TanStack Query → token metadata, settings, snapshots
Zustand        → streaming ticks, stream status, connection state
Local state    → selected filters, menus, selected row, mobile sheets
```

Before the backend reaches M2, build the shell, design system, deterministic
fixtures, Storybook stories, and mocked journeys. At M2, switch to generated API
types, mock-server integration, WebSocket behavior, and live discovery data.
Frontend fixtures must mirror the intended API shape rather than inventing a
private contract.

## Review order

Review frontend pull requests in this order: product behavior, API assumptions,
accessibility, responsive behavior, visual fidelity, then code structure. Keep
commits focused (`Build shared terminal tokens`, `Build Discover stream fixtures`,
`Add Discover visual baselines`) and do not mix dependency upgrades, unrelated
refactors, or backend contract edits into a user-visible slice.

## Frontend/shared contract boundary

Frontend plans live in this directory. Shared product or API changes belong in
the repository-level specs and must be raised with the collaborator first.
Frontend mocks should match the intended API and generated types rather than
inventing a private shape.
