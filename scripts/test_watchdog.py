#!/usr/bin/env python3
"""Tests for scripts/watchdog. verify.sh runs them.

GitHub is replaced by a fake serving responses recorded from #131 and #132 (scripts/fixtures/watchdog),
and the worktree is checked out in throwaway git repositories, so these run offline."""

import contextlib
import copy
import importlib.machinery
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
watchdog = importlib.machinery.SourceFileLoader("watchdog", str(HERE / "watchdog")).load_module()
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
