#!/usr/bin/env python3
"""Tests for scripts/work's rules that decide who holds what. verify.sh runs them.

GitHub is replaced by an in-memory fake, so these run offline: the D-number ledger's race, a stalled
PR's claim lapsing, Jutin's own claims, and which PR body lines take an issue."""

import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

# Loading the script without a .py name would otherwise leave a __pycache__ in scripts/.
sys.dont_write_bytecode = True
SCRIPT = Path(__file__).resolve().parent / "work"
loader = importlib.machinery.SourceFileLoader("work", str(SCRIPT))
work = importlib.util.module_from_spec(importlib.util.spec_from_loader("work", loader))
loader.exec_module(work)


class FakeLedger:
    """The ledger issue's comments. `rival` is posted just before the next comment of ours lands,
    as a session racing for the same number would."""

    def __init__(self, bodies, rival=None):
        self.comments = [{"id": i, "body": b} for i, b in enumerate(bodies, 1)]
        self.rival = rival

    def gh(self, path, method="GET", fields=None, check=True):
        if method == "POST" and path.endswith("/comments"):
            if self.rival:
                self.comments.append({"id": len(self.comments) + 1, "body": self.rival})
                self.rival = None
            comment = {"id": len(self.comments) + 1, "body": fields["body"]}
            self.comments.append(comment)
            return comment
        return list(self.comments)


class ReservingDNumbers(unittest.TestCase):
    def reserve(self, ledger, taken_elsewhere):
        work.gh = ledger.gh
        work.gh_all = ledger.gh
        work.ledger_issue = lambda: 116
        work.taken_d_numbers = lambda holders: set(holders) | taken_elsewhere
        work.cmd_reserve_d(["a decision"])
        return [c["body"].split(" ")[0] for c in ledger.comments]

    def test_the_next_number_above_main_open_prs_and_the_ledger_is_reserved(self):
        ledger = FakeLedger(["D95 — earlier — link"])
        self.assertEqual(self.reserve(ledger, {93, 94}), ["D95", "D96"])

    def test_a_session_that_loses_the_race_takes_the_next_number(self):
        # GitHub appends a footer to comments; the number at the start is what counts.
        ledger = FakeLedger(["D97 — earlier — link"], rival="D98 — rival\n\n---\n_footer_")
        self.assertEqual(self.reserve(ledger, {93}), ["D97", "D98", "D98", "D99"])

    def test_the_earliest_comment_for_a_number_holds_it(self):
        work.gh_all = FakeLedger(["D96 — first", "D96 — second", "Correction: D96 is …"]).gh
        self.assertEqual(work.ledger_holders(116), {96: 1})


class ClaimingAStalledPr(unittest.TestCase):
    def test_a_prs_claim_lapses_after_two_idle_hours_as_next_says(self):
        self.assertIsNone(work.claim_refusal(is_pr=True, has_wip=True, claim_is_recent=False))
        self.assertIsNotNone(work.claim_refusal(is_pr=True, has_wip=True, claim_is_recent=True))

    def test_an_issues_claim_lapses_only_by_release(self):
        self.assertIsNotNone(work.claim_refusal(is_pr=False, has_wip=True, claim_is_recent=False))
        self.assertIsNone(work.claim_refusal(is_pr=False, has_wip=False, claim_is_recent=False))


class JutinsClaims(unittest.TestCase):
    def claims(self, comments):
        work.gh = lambda path, *a, **k: [{"user": {"login": u}, "body": b} for u, b in comments]
        return work.jutin_claims(64)

    def test_his_latest_taking_or_dropping_comment_decides(self):
        self.assertTrue(self.claims([("jutin0852", "Taking this")]))
        self.assertFalse(self.claims([("jutin0852", "Taking this"), ("jutin0852", "Dropping this")]))
        self.assertTrue(self.claims([("jutin0852", "Dropping this"), ("jutin0852", " taking this now")]))

    def test_only_his_own_account_can_claim_for_him(self):
        self.assertFalse(self.claims([("AndySakov", "Taking this")]))
        self.assertFalse(self.claims([("jutin0852", "I'm Taking this")]))


class PrBodies(unittest.TestCase):
    def test_closing_keywords_and_part_of_take_an_issue_and_prose_does_not(self):
        self.assertEqual(work.working_refs("Closes #64"), {64})
        self.assertEqual(work.working_refs("Part of #64, and fixes #70"), {64, 70})
        self.assertEqual(work.working_refs('says "Issue 64" instead'), set())
        self.assertEqual(work.working_refs("Depart of #5"), set())

    def test_part_of_takes_an_issue_but_does_not_close_it(self):
        self.assertEqual(work.closing_refs("Part of #64"), set())


FIXTURES = Path(__file__).resolve().parent / "fixtures" / "d-numbers"


def recorded(name):
    return json.loads((FIXTURES / name).read_text())


class CheckingDNumbers(unittest.TestCase):
    """Against the ledger (#116) and #148's decisions.md patch as recorded on 2026-10-14: the ledger
    holds D97 to D103, and #148 added D103."""

    MAIN = "## D1 — first\n\n## D100 — pricing\n\n## D102 — trades\n"

    def problems(self, numbers, main=MAIN):
        added = work.added_d_numbers(main, main + "".join(f"\n## D{n} — new\n" for n in numbers))
        with mock.patch.object(work, "gh_all", lambda path: recorded("ledger-comments.json")):
            reserved = set(work.ledger_holders(116))
        others = {148: work.d_numbers_a_pr_adds(recorded("pr-148-files.json"))}
        return work.d_entry_problems(added, work.d_numbers_in(main), others, reserved, 116)

    def test_a_number_already_on_main_fails(self):
        self.assertEqual(self.problems([102]), ["D102 is already on main"])

    def test_a_number_another_open_pr_adds_fails_naming_it(self):
        self.assertEqual(self.problems([103]), ["D103 is also added by open PR #148"])

    def test_an_unreserved_number_fails(self):
        self.assertEqual(self.problems([104]), ["D104 has no reservation in the D-number ledger (#116)"])

    def test_a_reserved_number_nothing_else_has_passes_whatever_the_gaps(self):
        # D101 was reserved and never merged. Main jumps from D1 to D100, or has only D1.
        self.assertEqual(self.problems([101]), [])
        self.assertEqual(self.problems([101], main="## D1 — first\n"), [])

    def test_editing_a_heading_adds_nothing_and_a_second_copy_adds_it(self):
        main = "## D100 — pricing\n"
        self.assertEqual(work.added_d_numbers(main, "## D100 — pricing, renamed\n"), set())
        # A PR merged into main: main's D100 and the PR's own D100.
        self.assertEqual(work.added_d_numbers(main, main + "\n## D100 — mine\n"), {100})

    def test_an_open_pr_that_only_edits_a_heading_adds_nothing(self):
        files = [{"filename": "docs/spec/decisions.md", "patch": "@@ -1 +1 @@\n-## D80 — old\n+## D80 — new"}]
        self.assertEqual(work.d_numbers_a_pr_adds(files), set())
        self.assertEqual(work.d_numbers_a_pr_adds(recorded("pr-148-files.json")), {103})


# Outside the repository's git hook's variables (GIT_DIR, GIT_INDEX_FILE, ...): verify.sh runs these tests
# from the pre-commit hook, and with them every git command here would act on the repository itself.
CLEAN_ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}


def git(cwd, *args):
    return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false",
                           "-c", "core.hooksPath=/dev/null", *args], cwd=cwd, capture_output=True, text=True,
                          check=True, env=CLEAN_ENV).stdout.strip()


class RecordedGitHub:
    """This PR, #150; #148 and its recorded files; and the ledger, #116, with its recorded comments."""

    def gh(self, path, method="GET", fields=None, check=True):
        assert (method, path) == ("GET", f"repos/{work.REPO}/issues?labels=d-ledger&state=open&per_page=10"), path
        return [{"number": 116}]

    def gh_all(self, path):
        return {
            f"repos/{work.REPO}/pulls?state=open": [{"number": 150, "head": {"ref": "mine"}},
                                                    {"number": 148, "head": {"ref": "theirs"}}],
            f"repos/{work.REPO}/pulls/148/files": recorded("pr-148-files.json"),
            f"repos/{work.REPO}/issues/116/comments": recorded("ledger-comments.json"),
        }[path]


class CheckingDNumbersAsCiDoes(unittest.TestCase):
    """`check-d` on a PR merged into its base, as CI checks it out: HEAD^1 is the base."""

    DECISIONS = "# Decisions\n\n## D1 — first\n\nWhy.\n\n## D100 — pricing\n\nWhy.\n"

    def setUp(self):
        environment = mock.patch.dict(os.environ, CLEAN_ENV, clear=True)
        environment.start()
        self.addCleanup(environment.stop)
        github = RecordedGitHub()
        for name in ("gh", "gh_all"):
            patch = mock.patch.object(work, name, getattr(github, name))
            patch.start()
            self.addCleanup(patch.stop)
        self.repo = Path(tempfile.mkdtemp()).resolve()
        self.addCleanup(shutil.rmtree, self.repo)
        git(self.repo, "init", "-q", "-b", "main")
        self.assertEqual(git(self.repo, "rev-parse", "--show-toplevel"), str(self.repo))
        self.commit(self.DECISIONS)
        cwd = os.getcwd()
        os.chdir(self.repo)
        self.addCleanup(os.chdir, cwd)

    def commit(self, decisions):
        path = self.repo / work.DECISIONS
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(decisions)
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", "change")

    def check(self, pr_appends, main_inserts=""):
        """The PR appends entries; main, meanwhile, inserts some after D1. Returns check-d's exit code and summary."""
        git(self.repo, "checkout", "-q", "-b", "pr")
        self.commit(self.DECISIONS + pr_appends)
        git(self.repo, "checkout", "-q", "main")
        if main_inserts:
            self.commit(self.DECISIONS.replace("## D100", main_inserts + "## D100"))
        git(self.repo, "merge", "-q", "--no-ff", "-m", "merge", "pr")
        summary = self.repo.parent / f"{self.repo.name}-summary.md"
        self.addCleanup(summary.unlink, missing_ok=True)
        with self.assertRaises(SystemExit) as exit, contextlib.redirect_stdout(io.StringIO()):
            work.cmd_check_d(["--base", "HEAD^1", "--head", "HEAD", "--pr", "150", "--summary", str(summary)])
        return exit.exception.code, summary.read_text()

    def test_a_reserved_number_nothing_else_has_passes(self):
        self.assertEqual(self.check("\n## D101 — new\n\nWhy.\n"), (0,
            "## D-numbers\n\nEach D-entry this change adds is reserved, and on neither main nor another open PR: D101.\n"))

    def test_a_number_main_took_since_the_pr_branched_fails(self):
        self.assertEqual(self.check("\n## D101 — mine\n\nWhy.\n", main_inserts="## D101 — theirs\n\nWhy.\n\n"),
                         (1, "## D-numbers\n\n- D101 is already on main\n"))

    def test_each_problem_of_each_number_is_listed(self):
        self.assertEqual(self.check("\n## D103 — new\n\nWhy.\n\n## D104 — new\n\nWhy.\n"), (1,
            "## D-numbers\n\n- D103 is also added by open PR #148\n- D104 has no reservation in the D-number ledger (#116)\n"))

    def test_a_change_adding_no_entry_passes(self):
        self.assertEqual(self.check("\nMore on D100.\n"), (0, "## D-numbers\n\nThis change adds no D-entry.\n"))


if __name__ == "__main__":
    unittest.main()
