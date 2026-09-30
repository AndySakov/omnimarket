# OmniMarket Frontend Workspace

This directory contains Jutin's frontend-owned planning and implementation
artifacts for the full OmniMarket terminal.

The shared repository specifications under `docs/spec/` remain the product and
API contract. Frontend planning documents provide implementation detail and do
not silently change that contract.

## Start here

- [Frontend implementation plan](frontend-plan.md)
- [Reference and layout brief](frontend-reference-layout-brief.md)
- [Collaboration workflow](collaboration-workflow.md)

## Current delivery checkpoint

Phase 0 is implemented and locally verified for the global terminal shell and
Discover surface. The foundation branch is awaiting collaborator review; the
next feature branch is `codex/token-workspace` for the mocked Token Workspace.

## Verification

From `web/terminal`, use `npm run verify:fast` for the local commit lane and
`npm run verify:pr` for the complete PR lane, including Storybook, Playwright
flows, visual baselines, and accessibility checks.
