# Domain Docs

How the engineering skills should consume this repo's domain documentation. Single-context layout.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root: the glossary.
- **`docs/spec/decisions.md`**: the decision log (D-entries). This repo uses it instead of `docs/adr/`. Read the entries that touch the area you're about to work in.
- **`docs/spec/README.md`**: index of subsystem specs. Open the spec for the subsystem in play.

## Recording decisions

Don't create `docs/adr/`. A new decision is a new D-entry in `docs/spec/decisions.md`: the decision, the rejected options, and why. To change an existing decision, add an amendment note to it and a new entry. Mark anything unverified **(verify)**.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary lists under _Avoid_.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag decision conflicts

If your output contradicts an existing D-entry, surface it explicitly rather than silently overriding:

> _Contradicts D10 (fast loop and reconciler), but worth reopening because…_
