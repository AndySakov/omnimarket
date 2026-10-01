#!/usr/bin/env python3
"""Tests for scripts/regression-check (D92). verify.sh runs them.

The end-to-end cases build a throwaway git repository holding one small crate with a bug on `main`,
and a branch per kind of fix PR, then run the script on each branch as CI would."""

import importlib.machinery
import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

# Loading the script without a .py name would otherwise leave a __pycache__ in scripts/.
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "regression-check"
# Outside the repository's git hook's variables (GIT_INDEX_FILE, ...): verify.sh runs these tests
# from the pre-commit hook, and the throwaway repository's git calls mustn't reach this one.
CLEAN_ENV = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
rc = importlib.machinery.SourceFileLoader("regression_check", str(SCRIPT)).load_module()


class ReadingRust(unittest.TestCase):
    def test_braces_and_test_attributes_inside_strings_and_comments_are_ignored(self):
        text = textwrap.dedent(
            """\
            // #[test] fn not_a_test() {}
            #[test]
            fn a() { let s = "}{ #[test] fn b() {}"; let c = '}'; let r = r#"}"#; }
            /* #[test] fn c() {} */
            fn helper<'a>(x: &'a str) -> &'a str { x }
            #[tokio::test(flavor = "current_thread")]
            async fn d() {}
            """
        )
        self.assertEqual(list(rc.test_fns(text)), ["a", "d"])

    def test_tests_in_nested_modules_carry_their_module_path(self):
        text = "mod outer { mod inner { #[test] fn t() {} } #[test] fn u() {} }\n#[test]\nfn v() {}\n"
        self.assertEqual(sorted(rc.test_fns(text)), ["outer::inner::t", "outer::u", "v"])

    def test_an_ignored_test_is_marked(self):
        tests = rc.test_fns('#[test]\n#[ignore = "slow"]\nfn slow() {}\n#[test]\nfn fast() {}\n')
        self.assertTrue(tests["slow"].ignored)
        self.assertFalse(tests["fast"].ignored)

    def test_only_tests_in_cfg_test_items_count_in_a_source_file(self):
        text = "#[test]\nfn outside() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn inside() {}\n}\n"
        self.assertEqual(list(rc.test_fns(text, rc.cfg_test_items(text))), ["tests::inside"])

    def test_splice_keeps_mains_code_and_takes_the_heads_test_items(self):
        base = "pub fn f() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn old() {}\n}\n"
        head = "pub fn f() -> u8 { 2 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn new() {}\n}\n"
        spliced = rc.splice(base, head)
        self.assertIn("{ 1 }", spliced)
        self.assertNotIn("{ 2 }", spliced)
        self.assertEqual(list(rc.test_fns(spliced, rc.cfg_test_items(spliced))), ["tests::new"])

    def test_targets_and_module_paths_follow_cargos_layout(self):
        d = Path("crates/x")
        self.assertEqual(rc.target_of(d, "crates/x/tests/spec.rs"), (("--test", "spec"), []))
        self.assertEqual(rc.target_of(d, "crates/x/tests/big/main.rs"), (("--test", "big"), []))
        self.assertEqual(rc.target_of(d, "crates/x/tests/common/mod.rs"), (None, []))
        self.assertEqual(rc.target_of(d, "crates/x/src/lib.rs"), (("--lib",), []))
        self.assertEqual(rc.target_of(d, "crates/x/src/a/mod.rs"), (("--lib",), ["a"]))
        self.assertEqual(rc.target_of(d, "crates/x/src/a/b.rs"), (("--lib",), ["a", "b"]))
        self.assertEqual(rc.target_of(d, "crates/x/src/main.rs"), (("--bins",), []))

    def test_a_semicolon_inside_brackets_does_not_end_a_cfg_test_item(self):
        text = "pub fn f() {}\n#[cfg(test)]\nconst X: [u8; 2] = [1, 2];\npub fn g() {}\n"
        self.assertEqual(rc.splice(text, ""), "pub fn f() {}\n\npub fn g() {}\n")

    def test_only_closing_references_to_bug_issues_make_a_fix(self):
        labels = {1: ["bug"], 2: ["backend"], 3: ["bug", "critical"]}.get
        self.assertEqual(rc.fixed_bugs("Fixes #1", labels), [1])
        self.assertEqual(rc.fixed_bugs("closes #2", labels), [])
        self.assertEqual(rc.fixed_bugs("Resolved: #3\nCloses #2", labels), [3])
        self.assertEqual(rc.fixed_bugs("See #1; related to #3", labels), [])
        self.assertEqual(rc.fixed_bugs("", labels), [])
        self.assertEqual(rc.fixed_bugs(f"Fixes {rc.REPO}#1", labels), [1])
        self.assertEqual(rc.fixed_bugs("Fixes someone/else#1", labels), [])


LIB = """\
/// `part` as a percentage of `whole`, at most 100.
pub fn percent(part: u32, whole: u32) -> u32 {{
    (part * 100 / whole).min(CAP)
}}

const CAP: u32 = {cap};
{extra}
#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn half_is_fifty() {{
        assert_eq!(percent(1, 2), 50);
    }}
{tests}}}
"""

EXTRA_BASE = """\
pub fn double(x: u32) -> u32 {
    x * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubles() {
        assert_eq!(super::double(2), 4);
    }
}
"""


def lib(cap, tests="", extra=""):
    return LIB.format(cap=cap, tests=tests, extra=extra)


def test_fn(name, body):
    return f"\n    #[test]\n    fn {name}() {{\n        {body}\n    }}\n"


class OnAFixPr(unittest.TestCase):
    """The script on a crate whose `main` caps percentages at 99 instead of 100."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp(prefix="regression-check-test-")
        cls.repo = Path(cls.tmp) / "repo"
        cls.target = Path(cls.tmp) / "target"
        cls.repo.mkdir()
        cls.git("init", "-q", "-b", "main")
        shutil.copy(ROOT / "rust-toolchain.toml", cls.repo)
        cls.write("Cargo.toml", '[package]\nname = "fixture"\nversion = "0.1.0"\nedition = "2021"\n')
        cls.write(".gitignore", "target/\nCargo.lock\n")
        cls.write("src/lib.rs", lib(99, extra="\npub mod extra;\n"))
        cls.write("src/extra.rs", EXTRA_BASE)
        cls.commit("main with the bug")
        fix = lib(100, extra="\npub mod extra;\n")

        # Tests the fix: 100% is 100, which main gets wrong.
        cls.branch("good", {"src/lib.rs": lib(100, test_fn("all_is_a_hundred", "assert_eq!(percent(2, 2), 100);"), "\npub mod extra;\n")})
        # A test that passes on main too: it never reaches the cap.
        cls.branch("weak", {"src/lib.rs": lib(100, test_fn("a_quarter_is_25", "assert_eq!(percent(1, 4), 25);"), "\npub mod extra;\n")})
        # The fix's test in an integration test file.
        cls.branch("integration", {"src/lib.rs": fix, "tests/spec.rs": "#[test]\nfn all_is_a_hundred() {\n    assert_eq!(fixture::percent(3, 3), 100);\n}\n"})
        # A test only of new API: it can't build on main, so it shows nothing about the bug.
        cls.branch("new-api", {"src/lib.rs": fix + "\npub fn cap() -> u32 {\n    CAP\n}\n", "tests/spec.rs": "#[test]\nfn the_cap_is_a_hundred() {\n    assert_eq!(fixture::cap(), 100);\n}\n"})
        # One file's new test doesn't build on main; another file's test still shows the bug.
        cls.branch(
            "mixed",
            {
                "src/lib.rs": lib(100, test_fn("all_is_a_hundred", "assert_eq!(percent(2, 2), 100);"), "\npub mod extra;\n"),
                "src/extra.rs": EXTRA_BASE.replace("x * 2\n}", "x * 2\n}\n\npub fn triple(x: u32) -> u32 {\n    x * 3\n}")
                .replace("    }\n}", "    }\n\n    #[test]\n    fn triples() {\n        assert_eq!(super::triple(2), 6);\n    }\n}"),
            },
        )
        # A fix that breaks a test on its own head.
        cls.branch("red", {"src/lib.rs": lib(100, test_fn("all_is_a_hundred", "assert_eq!(percent(2, 2), 99);"), "\npub mod extra;\n")})
        # No test at all.
        cls.branch("untested", {"src/lib.rs": fix})

        # main moves on after the branches: the check runs against main's latest code.
        cls.git("checkout", "-q", "main")
        cls.write("README.md", "fixture\n")
        cls.commit("main moves on")

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)

    @classmethod
    def git(cls, *args):
        env = dict(CLEAN_ENV, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t")
        subprocess.run(["git", "-c", "core.hooksPath=/dev/null", *args], cwd=cls.repo, check=True, env=env, capture_output=True)

    @classmethod
    def write(cls, path, text):
        (cls.repo / path).parent.mkdir(parents=True, exist_ok=True)
        (cls.repo / path).write_text(text)

    @classmethod
    def commit(cls, message):
        cls.git("add", "-A")
        cls.git("commit", "-q", "-m", message)

    @classmethod
    def branch(cls, name, files):
        cls.git("checkout", "-q", "-b", name, "main")
        for path, text in files.items():
            cls.write(path, text)
        cls.commit(name)

    def check(self, head):
        summary = Path(self.tmp) / f"{head}.md"
        done = subprocess.run(
            [sys.executable, str(SCRIPT), "--base", "main", "--head", head, "--summary", str(summary), "--target-dir", str(self.target)],
            cwd=self.repo,
            capture_output=True,
            text=True,
            env=CLEAN_ENV,
        )
        self.assertIn(done.returncode, (0, 1), done.stderr)
        self.assertEqual(summary.read_text().strip(), done.stdout.strip())
        return done.returncode, done.stdout

    def row(self, report, test):
        return next(line for line in report.splitlines() if f"`{test}`" in line and line.startswith("|"))

    def test_a_test_that_fails_on_main_passes_the_check(self):
        code, report = self.check("good")
        self.assertEqual(code, 0, report)
        self.assertIn("| fixture --lib | `tests::all_is_a_hundred` | fails | passes |", report)
        self.assertIn("**Passes:** failing on main's code: `tests::all_is_a_hundred`", report)
        self.assertIn("left: 99", report)  # the panic, so a reader sees why it fails

    def test_a_test_that_passes_on_main_too_fails_the_check_and_says_so(self):
        code, report = self.check("weak")
        self.assertEqual(code, 1, report)
        self.assertIn("| fixture --lib | `tests::a_quarter_is_25` | passes | passes |", report)
        self.assertIn("**Fails:** none of the PR's new or changed tests fails on main's code", report)

    def test_an_integration_test_runs_on_mains_code(self):
        code, report = self.check("integration")
        self.assertEqual(code, 0, report)
        self.assertIn("| fixture --test spec | `all_is_a_hundred` | fails | passes |", report)

    def test_a_test_that_does_not_build_on_main_does_not_count(self):
        code, report = self.check("new-api")
        self.assertEqual(code, 1, report)
        self.assertIn("| fixture --test spec | `the_cap_is_a_hundred` | doesn't build | passes |", report)
        self.assertIn("cannot find function `cap`", report)

    def test_one_files_tests_not_building_on_main_leave_the_others_running(self):
        code, report = self.check("mixed")
        self.assertEqual(code, 0, report)
        self.assertIn("| fails | passes |", self.row(report, "tests::all_is_a_hundred"))
        self.assertIn("| doesn't build | passes |", self.row(report, "extra::tests::triples"))

    def test_a_test_failing_on_the_head_fails_the_check(self):
        code, report = self.check("red")
        self.assertEqual(code, 1, report)
        self.assertIn("must pass on the PR's head", report)

    def test_a_fix_without_a_test_fails_the_check(self):
        code, report = self.check("untested")
        self.assertEqual(code, 1, report)
        self.assertIn("adds or changes no test", report)


if __name__ == "__main__":
    unittest.main()
