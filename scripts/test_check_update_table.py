#!/usr/bin/env python3
"""Tests for scripts/check-update-table. verify.sh runs them.

Each runs the script on a throwaway repository holding a copy of this repository's CLAUDE.md, so the
table under test is the real one, and a copy with a row added shows the table alone decides."""

import importlib.machinery
import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "check-update-table"
CLAUDE = (HERE.parent / "CLAUDE.md").read_text()
loader = importlib.machinery.SourceFileLoader("check_update_table", str(SCRIPT))
check = importlib.util.module_from_spec(importlib.util.spec_from_loader("check_update_table", loader))
loader.exec_module(check)
# Outside the repository's git hook's variables (GIT_DIR, GIT_INDEX_FILE, ...): verify.sh runs these tests
# from the pre-commit hook, and with them every git command here would act on the repository itself.
CLEAN_ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}


class Repo:
    """A throwaway repository: `main` with CLAUDE.md and the docs, and a branch for the change."""

    def __init__(self, table=CLAUDE):
        self.dir = Path(tempfile.mkdtemp()).resolve()
        self.git("init", "-q", "-b", "main")
        self.write({"CLAUDE.md": table, "docs/spec/pricing.md": "pricing\n", "docs/spec/data.md": "data\n",
                    "crates/pricing/src/lib.rs": "// pricing\n", "proto/a.proto": "// a\n", "scripts/tool": "#\n"})
        self.commit("main")
        assert self.git("rev-parse", "--show-toplevel") == str(self.dir), "never anything but this repository"
        self.git("checkout", "-q", "-b", "change")

    def git(self, *args):
        return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false",
                               "-c", "core.hooksPath=/dev/null", *args], cwd=self.dir, capture_output=True, text=True,
                              check=True, env=CLEAN_ENV).stdout.strip()

    def write(self, files):
        for name, text in files.items():
            path = self.dir / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-qm", message)

    def check(self, body=None, table="CLAUDE.md"):
        """The script's exit code and summary, for the branch against main."""
        args = [str(SCRIPT), "--base", "main", "--head", "HEAD", "--table", table]
        if body is not None:
            (self.dir / "body.md").write_text(body)
            args += ["--body", "body.md"]
        out = subprocess.run(args, cwd=self.dir, capture_output=True, text=True, env=CLEAN_ENV)
        return out.returncode, out.stdout

    def remove(self):
        subprocess.run(["rm", "-rf", str(self.dir)])


class Checking(unittest.TestCase):
    def setUp(self):
        self.repo = Repo()
        self.addCleanup(self.repo.remove)

    def test_a_pricing_change_without_pricing_md_fails_naming_both(self):
        self.repo.write({"crates/pricing/src/lib.rs": "// pricing, changed\n"})
        self.repo.commit("pricing")
        code, summary = self.repo.check()
        self.assertEqual(code, 1)
        self.assertIn("| `crates/pricing/src/lib.rs` | `docs/spec/pricing.md` |", summary)

    def test_changing_both_passes(self):
        self.repo.write({"crates/pricing/src/lib.rs": "// changed\n", "docs/spec/pricing.md": "pricing, changed\n"})
        self.repo.commit("pricing and its spec")
        code, summary = self.repo.check()
        self.assertEqual(code, 0, summary)
        self.assertIn("Every changed path's docs changed too, or are waived.", summary)

    def test_a_waiver_line_passes_and_is_listed_in_the_summary(self):
        self.repo.write({"crates/pricing/src/lib.rs": "// a comment\n"})
        self.repo.commit("pricing")
        body = "## Fix\n\nA comment.\n\nDocs unchanged: `docs/spec/pricing.md`: only a comment changed\n"
        code, summary = self.repo.check(body)
        self.assertEqual(code, 0, summary)
        self.assertIn("### Waived\n\n- `docs/spec/pricing.md`: only a comment changed\n", summary)

    def test_the_summary_is_appended_to_the_summary_file(self):
        self.repo.write({"crates/pricing/src/lib.rs": "// changed\n"})
        self.repo.commit("pricing")
        summary = self.repo.dir / "step-summary.md"
        summary.write_text("earlier\n")
        subprocess.run([str(SCRIPT), "--base", "main", "--summary", str(summary)], cwd=self.repo.dir,
                       capture_output=True, env=CLEAN_ENV)
        self.assertTrue(summary.read_text().startswith("earlier\n## Update table\n"))

    def test_a_waiver_for_another_doc_waives_nothing_here(self):
        self.repo.write({"crates/pricing/src/lib.rs": "// changed\n"})
        self.repo.commit("pricing")
        code, _ = self.repo.check("Docs unchanged: docs/spec/data.md: nothing here\n")
        self.assertEqual(code, 1)

    def test_a_row_added_to_the_table_changes_the_result_with_no_code_change(self):
        self.repo.write({"scripts/tool": "# changed\n"})
        self.repo.commit("a script")
        self.assertEqual(self.repo.check()[0], 0)
        row = "| `crates/api/` | `docs/spec/frontend.md` (API contract) |"
        self.assertIn(row, CLAUDE)
        self.repo.write({"ROWS.md": CLAUDE.replace(row, row + "\n| `scripts/` | `docs/agents/process.md` |")})
        code, summary = self.repo.check(table="ROWS.md")
        self.assertEqual(code, 1)
        self.assertIn("| `scripts/tool` | `docs/agents/process.md` |", summary)

    def test_conditional_docs_and_rows_that_arent_paths_are_left_to_the_watchdog(self):
        self.repo.write({"proto/a.proto": "// changed\n", "docs/spec/data.md": "data, changed\n"})
        self.repo.commit("proto")
        code, summary = self.repo.check()
        self.assertEqual(code, 0, summary)
        self.assertIn("- `docs/spec/frontend.md` if the API contract moves (the change touches `proto/a.proto`)", summary)
        self.assertIn("A latency budget → `docs/spec/slas.md`", summary)
        self.assertIn("Milestone scope → `docs/build-plan.md`", summary)

    def test_a_file_moved_out_of_a_row_counts_too(self):
        self.repo.git("mv", "crates/pricing/src/lib.rs", "lib.rs")
        self.repo.commit("move")
        code, summary = self.repo.check()
        self.assertEqual(code, 1)
        self.assertIn("| `crates/pricing/src/lib.rs` | `docs/spec/pricing.md` |", summary)


class ReadingTheTable(unittest.TestCase):
    def rows(self):
        return {row.change: row for row in check.read_table(CLAUDE)}

    def test_each_row_has_its_paths_required_docs_conditions_and_words(self):
        rows = self.rows()
        det = rows["`crates/det/`, `crates/sim/`, `loadtest/`"]
        self.assertEqual((det.paths, det.required, det.conditional, det.words),
                         (["crates/det/", "crates/sim/", "loadtest/"],
                          ["docs/spec/observability.md", "docs/spec/loadtest.md"], [], ""))
        engine = rows["`crates/chain-io/`, `crates/engine/`"]
        self.assertEqual((engine.required, engine.conditional),
                         (["docs/spec/indexer.md"], [("docs/spec/architecture.md", "if a component boundary moves")]))
        watcher = rows["`crates/watcher/`, `crates/telemetry/`, lineage or brakes"]
        self.assertEqual((watcher.paths, watcher.words), (["crates/watcher/", "crates/telemetry/"], "lineage or brakes"))
        self.assertEqual(rows["`deploy/`, CI"].words, "CI")
        self.assertEqual((rows["A latency budget"].paths, rows["A latency budget"].words), ([], "A latency budget"))


if __name__ == "__main__":
    unittest.main()
