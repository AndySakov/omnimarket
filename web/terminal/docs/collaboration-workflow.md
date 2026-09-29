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

## Frontend/shared contract boundary

Frontend plans live in this directory. Shared product or API changes belong in
the repository-level specs and must be raised with the collaborator first.
Frontend mocks should match the intended API and generated types rather than
inventing a private shape.
