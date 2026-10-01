#!/usr/bin/env python3
"""Tests for scripts/work's rules that decide who holds what. verify.sh runs them.

GitHub is replaced by an in-memory fake, so these run offline: the D-number ledger's race, a stalled
PR's claim lapsing, Jutin's own claims, and which PR body lines take an issue."""

import importlib.machinery
import sys
import unittest
from pathlib import Path

# Loading the script without a .py name would otherwise leave a __pycache__ in scripts/.
sys.dont_write_bytecode = True
SCRIPT = Path(__file__).resolve().parent / "work"
work = importlib.machinery.SourceFileLoader("work", str(SCRIPT)).load_module()


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


if __name__ == "__main__":
    unittest.main()
