#!/usr/bin/env python3
"""Tests for scripts/watchdog. verify.sh runs them.

GitHub is replaced by a fake serving responses recorded from #131 and #132 (scripts/fixtures/watchdog),
and the worktree is checked out in throwaway git repositories, so these run offline."""

import contextlib
import copy
import importlib.machinery
import importlib.util
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
loader = importlib.machinery.SourceFileLoader("watchdog", str(HERE / "watchdog"))
watchdog = importlib.util.module_from_spec(importlib.util.spec_from_loader("watchdog", loader))
loader.exec_module(watchdog)
work = watchdog.work
FIXTURES = HERE / "fixtures" / "watchdog"
R = f"repos/{watchdog.REPO}"
HEADS = {131: "798983f7c32c936eaf65ef9b9f5136ce5ee18b96", 132: "0f940905d170bd506f511b8845c982926dcc886d"}


def recorded(name):
    return json.loads((FIXTURES / name).read_text())


def without_watchdog(comments):
    return [c for c in comments if not c["body"].startswith("## Watchdog")]


class FakeGitHub:
    """Answers `work.gh` from recorded responses, by path without its query, and keeps posted comments."""

    def __init__(self, responses):
        self.responses = responses
        self.posted = []

    def gh(self, path, method="GET", fields=None, check=True):
        base, _, query = path.partition("?")
        if method == "POST":
            comment = {"id": len(self.posted) + 1, "user": {"login": "AndySakov"},
                       "created_at": "2026-10-14T00:00:00Z", "body": fields["body"]}
            self.posted.append(fields["body"])
            self.responses[base].append(comment)
            return comment
        page = re.search(r"(?:^|&)page=(\d+)", query)
        if page and page.group(1) != "1":
            return []
        return copy.deepcopy(self.responses[base])


def github(prs=(131, 132), comments=None):
    """Both PRs open, with their recorded checks and, unless given, their recorded comments."""
    responses = {f"{R}/pulls": [recorded(f"pull-{n}.json") for n in prs]}
    for n in prs:
        sha = HEADS[n]
        responses[f"{R}/pulls/{n}"] = recorded(f"pull-{n}.json")
        responses[f"{R}/commits/{sha}/check-runs"] = recorded(f"check-runs-{n}.json")
        responses[f"{R}/commits/{sha}/status"] = recorded(f"status-{n}.json")
        responses[f"{R}/issues/{n}/comments"] = (comments or {}).get(n, recorded(f"comments-{n}.json"))
    responses[f"{R}/issues/79"] = recorded("issue-79.json")
    responses[f"{R}/issues/124"] = recorded("issue-124.json")
    responses[f"{R}/pulls/132/files"] = recorded("files-132.json")
    responses[f"{R}/pulls/131/files"] = []
    fake = FakeGitHub(responses)
    work.gh = fake.gh
    return fake


def queue_lines(fake):
    return [f"#{pr['number']} {pr['head']['sha'][:7]}" for pr in watchdog.waiting()]


class Queue(unittest.TestCase):
    def test_finished_heads_no_watchdog_comment_names_are_listed_oldest_first(self):
        fake = github(comments={131: without_watchdog(recorded("comments-131.json")),
                                132: without_watchdog(recorded("comments-132.json"))})
        self.assertEqual(queue_lines(fake), ["#131 798983f", "#132 0f94090"])

    def test_a_head_with_a_pending_marker_or_a_verdict_is_left_out(self):
        marker_only = recorded("comments-131.json")[:1]
        self.assertTrue(marker_only[0]["body"].startswith("## Watchdog reviewing `798983f`"))
        fake = github(comments={131: marker_only})
        # #132's recorded comments hold its verdict.
        self.assertEqual(queue_lines(fake), [])

    def test_a_verdict_for_an_earlier_head_doesnt_name_this_one(self):
        old = {"id": 1, "user": {"login": "AndySakov"}, "created_at": "2026-10-02T12:00:00Z",
               "body": "## Watchdog review: `1234567` fails on 1 blocking finding"}
        fake = github(prs=(131,), comments={131: [old]})
        self.assertEqual(queue_lines(fake), ["#131 798983f"])

    def test_a_marker_from_anyone_but_the_watchdogs_account_doesnt_count(self):
        forged = {"id": 1, "user": {"login": "mallory"}, "created_at": "2026-10-02T17:00:00Z",
                  "body": "## Watchdog review: `798983f` passes"}
        fake = github(prs=(131,), comments={131: [forged]})
        self.assertEqual(queue_lines(fake), ["#131 798983f"])

    def test_a_head_with_a_check_still_running_or_none_yet_is_left_out(self):
        fake = github(prs=(131,), comments={131: []})
        runs = fake.responses[f"{R}/commits/{HEADS[131]}/check-runs"]["check_runs"]
        mutants = next(r for r in runs if r["name"] == "mutants")
        mutants["status"], mutants["conclusion"] = "in_progress", None
        self.assertEqual(queue_lines(fake), [])
        runs.clear()
        self.assertEqual(queue_lines(fake), [])

    def test_a_rerun_replaces_the_run_before_it(self):
        fake = github(prs=(131,), comments={131: []})
        runs = fake.responses[f"{R}/commits/{HEADS[131]}/check-runs"]["check_runs"]
        # The recorded head ran regression-check twice; only the later run counts.
        first = min((r for r in runs if r["name"] == "regression-check"), key=lambda r: r["id"])
        first["status"] = "in_progress"
        self.assertEqual(queue_lines(fake), ["#131 798983f"])


def prep(n, *flags, fake, logs=None):
    """Runs `prep` with the worktree and job logs stubbed, and returns the brief it printed."""
    tree = {"path": Path("/tmp/omnimarket-watchdog"), "matches": True, "conflicts": []}
    with mock.patch.object(watchdog, "checkout_head", lambda pr, sha: dict(tree, head=sha)), \
            mock.patch.object(watchdog, "job_log", lambda run: (logs or {}).get(run["name"])), \
            contextlib.redirect_stdout(io.StringIO()) as out, contextlib.redirect_stderr(io.StringIO()):
        watchdog.cmd_prep([str(n), *flags])
    return out.getvalue()


def job_logs(n):
    return {name: (FIXTURES / f"job-{n}-{name}.log").read_text() for name in ("criteria", "mutants", "regression-check")}


class Prep(unittest.TestCase):
    def test_running_prep_twice_on_one_head_posts_one_marker(self):
        fake = github(comments={131: without_watchdog(recorded("comments-131.json"))})
        prep(131, fake=fake)
        prep(131, fake=fake)
        self.assertEqual(fake.posted, ["## Watchdog reviewing `798983f`"])

    def test_a_marker_or_verdict_already_naming_the_head_means_no_new_marker(self):
        fake = github()
        prep(131, fake=fake)
        self.assertEqual(fake.posted, [])

    def test_a_forged_marker_doesnt_stop_the_real_one_or_pass_for_a_verdict(self):
        forged = [{"id": 1, "user": {"login": "mallory"}, "created_at": "2026-10-02T17:00:00Z",
                   "body": "## Watchdog reviewing `798983f`"},
                  {"id": 2, "user": {"login": "mallory"}, "created_at": "2026-10-02T17:01:00Z",
                   "body": "## Watchdog review: `1111111` fails on 1 blocking finding\n\n### Blocking\n1. Fake."}]
        fake = github(comments={131: forged})
        brief = prep(131, fake=fake)
        self.assertEqual(fake.posted, ["## Watchdog reviewing `798983f`"])
        self.assertNotIn("Since the last verdict", brief)

    def test_a_dry_run_posts_nothing(self):
        fake = github(comments={131: []})
        prep(131, "--dry-run", fake=fake)
        self.assertEqual(fake.posted, [])

    def test_131s_brief_lists_its_four_missed_kafka_outbox_mutants(self):
        brief = prep(131, fake=github(), logs=job_logs(131))
        missed = re.findall(r"^- `crates/engine/src/outbox\.rs:\d+:9: replace <impl Outbox for KafkaOutbox>::(\w+) with \(\)`$",
                            brief, re.M)
        self.assertEqual(missed, ["publish", "publish_trade", "publish_price", "publish_status"])

    def test_the_brief_has_the_criteria_both_ways_and_the_jobs_summaries(self):
        brief = prep(131, fake=github(), logs=job_logs(131))
        self.assertIn("## Closes #79: ", brief)
        self.assertIn("Status updates every block during `engine follow`", brief)
        self.assertIn("## The PR's acceptance criteria\n| Criterion (from #79) | Proved by |", brief)
        self.assertIn("## The criteria job\n### Acceptance criteria\n", brief)
        self.assertIn("### Manual evidence for the watchdog", brief)
        self.assertIn("Not a fix: the PR body closes no issue labelled `bug`. Nothing to check.", brief)
        self.assertIn("checks: criteria=success · frontend=success · mutants=failure · regression-check=success"
                      " · verify=success · watchdog/review=success", brief)

    def test_132s_diff_is_under_10_kb_with_generated_code_and_fixtures_as_stat_lines(self):
        diff = watchdog.diff_section(recorded("files-132.json"))
        self.assertLess(len(diff.encode()), 10 * 1024)
        raw = sum(len((f.get("patch") or "").encode()) for f in recorded("files-132.json"))
        self.assertGreater(raw, 30 * 1024)
        cut = [line for line in diff.splitlines() if not line.startswith(("--- ", "+", "-", " ", "@@", "\\"))]
        self.assertEqual(cut, [
            "web/terminal/src/api/generated/omnimarket/api/v1/market_pb.ts | modified +16 -1",
            "web/terminal/src/mocks/fixtures/api/candles/15m.json | added +171 -0",
            "web/terminal/src/mocks/fixtures/api/candles/1d.json | added +171 -0",
            "web/terminal/src/mocks/fixtures/api/candles/4h.json | added +171 -0",
        ])
        self.assertIn("--- docs/spec/data.md | modified", diff)

    def test_a_rereview_shows_the_last_verdicts_blocking_findings_and_the_replies_since(self):
        verdict = {"id": 1, "user": {"login": "AndySakov"}, "created_at": "2026-10-02T15:00:00Z", "body":
                   "## Watchdog review: `1111111` fails on 1 blocking finding\n\n### Verified\n- **x.** y\n\n"
                   "### Blocking\n1. **The lag counts the wall clock.** Fix: use the recorded reading.\n\n"
                   "### Non-blocking\n2. A nit.\n\n---\n_Generated_"}
        reply = {"id": 2, "user": {"login": "AndySakov"}, "created_at": "2026-10-02T16:00:00Z",
                 "body": "Fixed in 798983f: the lag reads the recorded clock."}
        brief = prep(131, fake=github(comments={131: [verdict, reply]}), logs={})
        earlier = brief.split("## Since the last verdict (1111111 fails)\n", 1)[1]
        self.assertTrue(earlier.startswith(
            "### Its blocking findings\n1. **The lag counts the wall clock.** Fix: use the recorded reading.\n"
            "### Comments since\nAndySakov · 2026-10-02T16:00:00Z\nFixed in 798983f: the lag reads the recorded clock."))
        self.assertNotIn("A nit.", earlier.split("## Diff")[0])

    def test_a_first_review_has_no_earlier_verdict_section(self):
        brief = prep(131, fake=github(comments={131: []}), logs={})
        self.assertNotIn("Since the last verdict", brief)


def verdict(pr, sha, at, manual, login="AndySakov"):
    """A verdict comment in the shape the repository-wide comments endpoint returns, logging `manual`."""
    body = f"## Watchdog review: `{sha}` passes\n\n### Verified\n- **It.** Ran it.\n\n### Manual checks\n"
    body += "".join(f"- `{kind}`: checked it by hand\n" for kind in manual) + "\n---\n_Generated by Claude Code_"
    return {"id": pr * 1000, "user": {"login": login}, "created_at": at,
            "issue_url": f"https://api.github.com/{R}/issues/{pr}", "body": body}


class Ledger(unittest.TestCase):
    """Against the comments on #147, #150, #151 and #154 and the `watchdog-automation` issues, as recorded
    on 2026-10-23: #135 (`update-table`) closed at 10:54, #136 to #140 open with their kinds, and no
    verdict yet with a `### Manual checks` section, so each test adds verdicts that have one."""

    def rows(self, *verdicts):
        rows, unknown = watchdog.ledger(recorded("ledger-comments.json") + list(verdicts),
                                        recorded("automation-issues.json"))
        return {r["kind"]: r for r in rows}, unknown

    def test_the_recorded_verdicts_log_no_manual_check_and_nothing_needs_an_issue(self):
        rows, unknown = self.rows()
        self.assertEqual({k: len(r["reviews"]) for k, r in rows.items() if r["reviews"]}, {})
        self.assertEqual([k for k, r in rows.items() if r["needs_issue"]], [])
        self.assertEqual(unknown, [])

    def test_each_kind_maps_to_the_open_issue_whose_kinds_line_names_it(self):
        rows, _ = self.rows()
        self.assertEqual({k: r["open"] for k, r in rows.items() if r["open"]},
                         {"d-number": 136, "mutants": 137, "fixture-invariants": 138, "region-states": 139,
                          "hostile-input": 139, "interleaving": 140, "io-bounds": 140})

    def test_a_kind_logged_in_two_reviews_with_no_issue_needs_one(self):
        rows, _ = self.rows(verdict(160, "aaaaaaa", "2026-10-24T09:00:00Z", ["copy-accuracy"]),
                            verdict(161, "bbbbbbb", "2026-10-24T10:00:00Z", ["copy-accuracy", "live-claim"]))
        self.assertTrue(rows["copy-accuracy"]["needs_issue"])
        self.assertEqual([(r["pr"], r["sha"]) for r in rows["copy-accuracy"]["reviews"]], [(160, "aaaaaaa"), (161, "bbbbbbb")])
        self.assertFalse(rows["live-claim"]["needs_issue"])

    def test_a_kind_an_open_issue_automates_needs_no_other(self):
        rows, _ = self.rows(verdict(160, "aaaaaaa", "2026-10-24T09:00:00Z", ["interleaving"]),
                            verdict(161, "bbbbbbb", "2026-10-24T10:00:00Z", ["interleaving"]))
        self.assertEqual((len(rows["interleaving"]["reviews"]), rows["interleaving"]["needs_issue"]), (2, False))

    def test_only_reviews_since_the_last_issue_for_a_kind_closed_count(self):
        before = [verdict(140 + i, f"{i}aaaaaa", f"2026-10-22T0{i}:00:00Z", ["update-table"]) for i in (1, 2)]
        rows, _ = self.rows(*before)
        self.assertEqual((len(rows["update-table"]["reviews"]), rows["update-table"]["needs_issue"]), (2, False))
        after = [verdict(160 + i, f"{i}bbbbbb", f"2026-10-24T0{i}:00:00Z", ["update-table"]) for i in (1, 2)]
        rows, _ = self.rows(*before, *after)
        self.assertTrue(rows["update-table"]["needs_issue"])

    def test_a_kind_tagged_twice_in_one_verdict_is_one_review(self):
        rows, _ = self.rows(verdict(160, "aaaaaaa", "2026-10-24T09:00:00Z", ["copy-accuracy", "copy-accuracy"]))
        self.assertEqual((len(rows["copy-accuracy"]["reviews"]), rows["copy-accuracy"]["needs_issue"]), (1, False))

    def test_other_accounts_and_pending_markers_dont_count(self):
        marker = verdict(160, "aaaaaaa", "2026-10-24T09:00:00Z", ["copy-accuracy"])
        marker["body"] = marker["body"].replace("## Watchdog review: `aaaaaaa` passes", "## Watchdog reviewing `aaaaaaa`")
        rows, _ = self.rows(verdict(161, "bbbbbbb", "2026-10-24T10:00:00Z", ["copy-accuracy"], login="jutin0852"), marker)
        self.assertEqual(rows["copy-accuracy"]["reviews"], [])

    def test_other_is_read_never_filed_and_an_unknown_kind_is_reported(self):
        rows, unknown = self.rows(verdict(160, "aaaaaaa", "2026-10-24T09:00:00Z", ["other", "flaky-ci"]),
                                  verdict(161, "bbbbbbb", "2026-10-24T10:00:00Z", ["other"]))
        self.assertEqual((len(rows["other"]["reviews"]), rows["other"]["needs_issue"]), (2, False))
        self.assertEqual(unknown, [(160, "flaky-ci")])

    def test_watchdog_md_lists_the_same_kinds_as_the_script(self):
        doc = (HERE.parent / "docs" / "agents" / "handoff" / "watchdog.md").read_text()
        table = doc[doc.index("| Kind | The check by hand |"):].split("\n\n")[0]
        self.assertEqual(tuple(re.findall(r"(?m)^\| `([a-z-]+)` \|", table)), watchdog.KINDS)

    def test_ledger_prints_each_kind_with_its_reviews(self):
        fake = FakeGitHub({f"{R}/issues/comments": recorded("ledger-comments.json") + [
            verdict(160, "aaaaaaa", "2026-10-24T09:00:00Z", ["copy-accuracy", "interleaving"]),
            verdict(161, "bbbbbbb", "2026-10-24T10:00:00Z", ["copy-accuracy"])],
            f"{R}/issues": recorded("automation-issues.json")})
        out = io.StringIO()
        with mock.patch.object(work, "gh", fake.gh), contextlib.redirect_stdout(out):
            watchdog.cmd_ledger([])
        self.assertEqual(out.getvalue().splitlines(), [
            "update-table: 0 reviews",
            "d-number: 0 reviews; automated by #136",
            "mutants: 0 reviews; automated by #137",
            "fixture-invariants: 0 reviews; automated by #138",
            "region-states: 0 reviews; automated by #139",
            "hostile-input: 0 reviews; automated by #139",
            "interleaving: 1 review (#160 aaaaaaa); automated by #140",
            "io-bounds: 0 reviews; automated by #140",
            "live-claim: 0 reviews",
            "copy-accuracy: 2 reviews (#160 aaaaaaa, #161 bbbbbbb); NEEDS AN ISSUE: labelled critical, "
            "watchdog-automation and its track, its body starting `Kinds: copy-accuracy`",
            "other: 0 reviews",
        ])


class Patches(unittest.TestCase):
    def test_a_changed_line_shows_what_changed_with_60_characters_either_side(self):
        before = "x" * 100 + "old" + "y" * 100
        after = "x" * 100 + "new words" + "y" * 100
        self.assertEqual(watchdog.elide(before, after),
                         ("…" + "x" * 60 + "old" + "y" * 60 + "…", "…" + "x" * 60 + "new words" + "y" * 60 + "…"))
        self.assertEqual(watchdog.elide("short", "shirt"), ("short", "shirt"))

    def test_unchanged_lines_go_and_lines_only_added_or_removed_stay_whole(self):
        patch = "@@ -1,4 +1,4 @@ fn x\n context\n-gone entirely\n-a b c\n+a B c\n+new line\n context"
        self.assertEqual(watchdog.compact(patch).split("\n"),
                         ["@@ -1,4 +1,4 @@ fn x", "-gone entirely", "+a B c", "-a b c", "+new line"])

    def test_generated_code_lockfiles_fixtures_and_images_are_one_stat_line(self):
        cut = [name for name in ("web/terminal/src/api/generated/x/y.ts", "crates/proto/src/a_pb.ts", "Cargo.lock",
                                 "web/terminal/package-lock.json", "crates/api/tests/fixtures/x.json",
                                 "web/terminal/tests/visual/a.spec.ts-snapshots/home.png", "web/terminal/package.json",
                                 "crates/api/src/candles.rs") if watchdog.condensed(name)]
        self.assertEqual(cut, ["web/terminal/src/api/generated/x/y.ts", "crates/proto/src/a_pb.ts", "Cargo.lock",
                               "web/terminal/package-lock.json", "crates/api/tests/fixtures/x.json",
                               "web/terminal/tests/visual/a.spec.ts-snapshots/home.png"])


# Outside the repository's git hook's variables (GIT_DIR, GIT_INDEX_FILE, ...): verify.sh runs these tests
# from the pre-commit hook, and with them every git command here would act on the repository itself.
CLEAN_ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}


def run(cwd, *args):
    return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false",
                           "-c", "core.hooksPath=/dev/null", *args], cwd=cwd, capture_output=True, text=True,
                          check=True, env=CLEAN_ENV).stdout.strip()


class Worktree(unittest.TestCase):
    """A PR head on a fake origin, and a caller's checkout of it with uncommitted work."""

    def setUp(self):
        # `prep`'s own git commands inherit the environment, so they get the clean one too.
        environment = mock.patch.dict(os.environ, CLEAN_ENV, clear=True)
        environment.start()
        self.addCleanup(environment.stop)
        # Resolved: on macOS the temporary directory is reached through a symlink.
        self.tmp = Path(tempfile.mkdtemp()).resolve()
        origin = self.tmp / "origin.git"
        run(self.tmp, "init", "-q", "--bare", "-b", "main", str(origin))
        seed = self.tmp / "seed"
        run(self.tmp, "clone", "-q", str(origin), str(seed))
        # Never anything but the throwaway repository.
        self.assertEqual(run(seed, "rev-parse", "--show-toplevel"), str(seed))
        (seed / "a.txt").write_text("a\n")
        run(seed, "add", ".")
        run(seed, "commit", "-qm", "a")
        run(seed, "push", "-q", "origin", "HEAD:main")
        self.seed = seed
        self.caller = self.tmp / "omnimarket"
        run(self.tmp, "clone", "-q", str(origin), str(self.caller))
        # No identity, as on CI's runner: prep's merge must not need the caller's.
        run(self.caller, "config", "user.name", "")
        (self.caller / "a.txt").write_text("my uncommitted work\n")
        (self.caller / "untracked.txt").write_text("mine\n")
        self.cwd = os.getcwd()
        os.chdir(self.caller)

    def tearDown(self):
        os.chdir(self.cwd)
        subprocess.run(["rm", "-rf", str(self.tmp)])

    def push_pr(self, n, files):
        run(self.seed, "checkout", "-q", "--detach", "origin/main")
        for name, text in files.items():
            path = self.seed / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
            if name.startswith(".githooks/"):
                path.chmod(0o755)
        run(self.seed, "add", ".")
        run(self.seed, "commit", "-qm", f"pr {n}")
        run(self.seed, "push", "-q", "origin", f"HEAD:refs/pull/{n}/head")
        return run(self.seed, "rev-parse", "HEAD")

    def advance_main(self, files):
        run(self.seed, "fetch", "-q", "origin")
        run(self.seed, "checkout", "-q", "--detach", "origin/main")
        for name, text in files.items():
            (self.seed / name).write_text(text)
        run(self.seed, "add", ".")
        run(self.seed, "commit", "-qm", "main moves")
        run(self.seed, "push", "-q", "origin", "HEAD:main")

    def caller_state(self):
        return (run(self.caller, "status", "--porcelain=v1", "--untracked-files=all"),
                run(self.caller, "rev-parse", "HEAD"))

    def test_prep_checks_the_head_out_next_to_the_checkout_and_never_touches_it(self):
        head = self.push_pr(7, {"b.txt": "b\n"})
        self.advance_main({"c.txt": "c\n"})
        before = self.caller_state()
        tree = watchdog.checkout_head(7, head)
        self.assertEqual(self.caller_state(), before)
        self.assertEqual(tree["path"], self.tmp / "omnimarket-watchdog")
        self.assertEqual((tree["head"], tree["matches"], tree["conflicts"]), (head, True, []))
        # The head, with main merged in.
        self.assertEqual(sorted(p.name for p in tree["path"].iterdir() if p.name != ".git"), ["a.txt", "b.txt", "c.txt"])
        self.assertEqual((tree["path"] / "a.txt").read_text(), "a\n")

    def test_a_conflict_with_main_is_reported_and_the_next_prep_starts_clean(self):
        head = self.push_pr(7, {"a.txt": "the pr's a\n"})
        self.advance_main({"a.txt": "main's a\n"})
        before = self.caller_state()
        self.assertEqual(watchdog.checkout_head(7, head)["conflicts"], ["a.txt"])
        other = self.push_pr(8, {"d.txt": "d\n"})
        tree = watchdog.checkout_head(8, other)
        self.assertEqual((tree["head"], tree["conflicts"]), (other, []))
        self.assertEqual(self.caller_state(), before)

    def test_a_git_hooks_variables_dont_point_prep_at_the_callers_checkout(self):
        head = self.push_pr(7, {"b.txt": "b\n"})
        before = self.caller_state()
        hooked = {"GIT_DIR": str(self.caller / ".git"), "GIT_INDEX_FILE": str(self.caller / ".git" / "index")}
        with mock.patch.dict(os.environ, hooked):
            tree = watchdog.checkout_head(7, head)
        self.assertEqual(self.caller_state(), before)
        self.assertEqual(tree["head"], head)

    def test_the_heads_own_git_hooks_never_run(self):
        # The repository's hooks path is relative, as scripts/setup.sh sets it, so the worktree's would
        # be the PR's.
        run(self.caller, "config", "core.hooksPath", ".githooks")
        ran = self.tmp / "the-prs-hook-ran"
        hook = f"#!/bin/sh\ntouch {ran}\n"
        head = self.push_pr(7, {".githooks/post-checkout": hook, ".githooks/post-merge": hook})
        self.advance_main({"c.txt": "c\n"})
        tree = watchdog.checkout_head(7, head)
        self.assertEqual(tree["head"], head)
        self.assertFalse(ran.exists())

    def test_a_head_that_moved_since_the_api_answered_is_flagged(self):
        head = self.push_pr(7, {"b.txt": "b\n"})
        tree = watchdog.checkout_head(7, "0" * 40)
        self.assertEqual((tree["head"], tree["matches"]), (head, False))


if __name__ == "__main__":
    unittest.main()
