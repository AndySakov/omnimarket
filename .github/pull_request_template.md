## Problem

<what was missing or wrong, for someone who hasn't read the issue>

## Fix

<what changed and why, in a few bullets; the D-entry if any>

## Acceptance criteria

<!-- CI's `criteria` job checks this table (D95). One row per acceptance criterion of the issue this
PR closes, quoting it as the issue words it. "Proved by" names the tests in backticks: a Rust test's
name or path (`a_lagging_node_is_asked_again_with_backoff`, `follow::tests::x`), or a frontend test's
title (`navigates between terminal workspaces`, `global header visual baselines > desktop shell`;
a `test.each` case by its expanded title, not its `%s` template).
Each must pass in this PR's CI run. A file path in backticks is allowed and isn't checked. Where no
test can prove it (a live measurement, a doc, a Storybook story, a criterion moved to a follow-up
issue), write `manual: <evidence>`; the job lists those for the watchdog. -->

| Criterion (from #n) | Proved by |
|---|---|
| <criterion, quoted> | `test_name` in `crate/tests/file.rs` |
| <criterion> | manual: <the run, its command and result> |

## Not verified

<anything you couldn't run or check, and why>

Closes #n
