//! Checks a PR's acceptance-criteria table against the issues it closes and the tests that passed
//! in its CI run (D95). The `criteria` binary does the file reading; everything here is a pure
//! function of the texts it's given.
//!
//! - An issue's criteria are the checkbox items under its `## Acceptance criteria` heading.
//! - A PR's table sits under the same heading: one row per criterion, quoting it, and a cell naming
//!   the tests that prove it (in backticks) or `manual: <evidence>`.
//! - Passed tests are the `test <name> ... ok` lines of libtest output. CI writes the frontend's
//!   Vitest and Playwright results in the same shape, with ` > ` between title segments.

use std::collections::BTreeSet;
use std::fmt;

/// The heading both an issue and a PR body put their criteria under.
const SECTION: &str = "## acceptance criteria";

/// The closing keywords GitHub recognises, longest first so `closes` isn't read as `close`.
const CLOSING_KEYWORDS: [&str; 9] = [
    "closes", "closed", "close", "fixes", "fixed", "fix", "resolves", "resolved", "resolve",
];

/// What a PR row offers as proof of its criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proof {
    /// Test names, each a full test path or its trailing segments.
    Tests(Vec<String>),
    /// Evidence a test can't give (a live measurement, a doc), for the watchdog to check.
    Manual(String),
    /// The cell names no test in backticks and doesn't start with `manual:`.
    None,
}

/// One row of a PR's acceptance-criteria table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The criterion as the row quotes it.
    pub criterion: String,
    pub proof: Proof,
}

/// An issue the PR closes, with its acceptance criteria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closed {
    pub issue: u64,
    pub criteria: Vec<String>,
}

/// One reason the check fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A criterion of a closed issue has no row.
    MissingCriterion { issue: u64, criterion: String },
    /// A row offers neither tests nor `manual:` evidence.
    NoProof { criterion: String },
    /// A named test didn't pass in this run: it doesn't exist, failed, or was ignored.
    TestNotPassed { criterion: String, test: String },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::MissingCriterion { issue, criterion } => {
                write!(f, "No row for #{issue}'s criterion \"{criterion}\"")
            }
            Problem::NoProof { criterion } => write!(
                f,
                "The row for \"{criterion}\" names no test in backticks and isn't `manual: <evidence>`"
            ),
            Problem::TestNotPassed { criterion, test } => write!(
                f,
                "`{test}` (for \"{criterion}\") didn't pass in this run: no such test, or it failed or was ignored"
            ),
        }
    }
}

/// The outcome of [`check`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// Empty when the PR passes.
    pub problems: Vec<Problem>,
    /// `(criterion as the row quotes it, evidence)` for each `manual:` row.
    pub manual: Vec<(String, String)>,
    /// Rows that quote no criterion of the closed issues: extra claims (a requirement from the
    /// issue's text), or misquotes, which also leave their criterion missing.
    pub unmatched: Vec<String>,
}

impl Report {
    /// The job summary, in Markdown: the problems, then the manual rows for the watchdog.
    pub fn summary(&self) -> String {
        let mut out = String::from("## Acceptance criteria\n\n");
        if self.problems.is_empty() {
            out.push_str(
                "Every criterion has a row, and every test the rows name passed in this run.\n",
            );
        } else {
            for problem in &self.problems {
                out.push_str(&format!("- {problem}\n"));
            }
        }
        if !self.unmatched.is_empty() {
            out.push_str(
                "\n## Rows that quote no criterion\n\nTheir tests are still checked. A misquoted criterion also shows above as having no row.\n\n",
            );
            for criterion in &self.unmatched {
                out.push_str(&format!("- {criterion}\n"));
            }
        }
        if !self.manual.is_empty() {
            out.push_str(
                "\n## Manual evidence for the watchdog\n\n| Criterion | Evidence |\n|---|---|\n",
            );
            for (criterion, evidence) in &self.manual {
                out.push_str(&format!(
                    "| {} | {} |\n",
                    escape_cell(criterion),
                    escape_cell(evidence)
                ));
            }
        }
        out
    }
}

/// The lines under the body's `## Acceptance criteria` heading, up to the next heading.
fn section(body: &str) -> Vec<&str> {
    let mut lines = body.lines().map(|l| l.trim_end_matches('\r'));
    if !lines
        .by_ref()
        .any(|l| l.trim().to_lowercase().starts_with(SECTION))
    {
        return Vec::new();
    }
    lines.take_while(|l| !is_heading(l)).collect()
}

/// A Markdown heading (`## Why`), not a line that starts with an issue number (`#98 …`).
fn is_heading(line: &str) -> bool {
    let text = line.trim_start();
    text.starts_with('#') && text.trim_start_matches('#').starts_with(' ')
}

/// The acceptance criteria of an issue: its checkbox items, ticked or not, with wrapped lines
/// joined.
pub fn issue_criteria(body: &str) -> Vec<String> {
    let mut criteria: Vec<String> = Vec::new();
    // Whether the previous line belongs to a criterion, so an indented line continues it.
    let mut open = false;
    for line in section(body) {
        let trimmed = line.trim();
        let item = ["- [ ]", "- [x]", "- [X]", "* [ ]", "* [x]", "* [X]"]
            .iter()
            .find_map(|marker| trimmed.strip_prefix(marker));
        if let Some(text) = item.filter(|_| !line.starts_with(char::is_whitespace)) {
            criteria.push(text.trim().to_string());
            open = true;
        } else if open && line.starts_with(char::is_whitespace) && !trimmed.starts_with(['-', '*'])
        {
            // A blank line is unindented once trimmed of its line ending, so it closes too.
            if let Some(last) = criteria.last_mut() {
                last.push(' ');
                last.push_str(trimmed);
            }
        } else {
            open = false;
        }
    }
    criteria
}

/// The issues a PR body closes with GitHub's closing keywords (`Closes #98`), in order.
pub fn closed_issues(body: &str) -> Vec<u64> {
    let lower = body.to_lowercase();
    let mut issues = BTreeSet::new();
    for (start, _) in lower.char_indices() {
        let starts_word = lower[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if !starts_word {
            continue;
        }
        let rest = &lower[start..];
        let Some(after) = CLOSING_KEYWORDS
            .iter()
            .find_map(|keyword| rest.strip_prefix(keyword))
        else {
            continue;
        };
        let after = after.strip_prefix(':').unwrap_or(after);
        let trimmed = after.trim_start();
        if trimmed.len() == after.len() {
            continue;
        }
        let Some(number) = trimmed.strip_prefix('#') else {
            continue;
        };
        let digits: String = number.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(issue) = digits.parse() {
            issues.insert(issue);
        }
    }
    issues.into_iter().collect()
}

/// The rows of a PR body's acceptance-criteria table. The first table row is the header.
pub fn pr_rows(body: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let table = section(body)
        .into_iter()
        .map(str::trim)
        .filter(|l| l.starts_with('|'))
        .skip(1);
    for line in table {
        let cells = split_cells(line);
        let is_separator = cells
            .iter()
            .all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')));
        if is_separator || cells.len() < 2 {
            continue;
        }
        rows.push(Row {
            criterion: cells[0].clone(),
            proof: read_proof(&cells[1]),
        });
    }
    rows
}

/// A table line's cells, trimmed. `\|` is a pipe inside a cell.
fn split_cells(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    // The text before the leading pipe isn't a cell; text after the last pipe is one only when
    // the line doesn't end with a pipe.
    let tail = cell.trim();
    if !tail.is_empty() {
        cells.push(tail.to_string());
    }
    cells.into_iter().skip(1).collect()
}

/// A proof cell: `manual: <evidence>`, or the test names in its backticks. A backticked span
/// that looks like a file path (`crates/x/tests/y.rs`) says where a test is, not which.
fn read_proof(cell: &str) -> Proof {
    let trimmed = cell.trim();
    if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("manual:") {
        let evidence = trimmed[7..].trim();
        if evidence.is_empty() {
            return Proof::None;
        }
        return Proof::Manual(evidence.to_string());
    }
    let tests: Vec<String> = trimmed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .filter(|span| !span.is_empty() && !is_path(span))
        .map(str::to_string)
        .collect();
    if tests.is_empty() {
        Proof::None
    } else {
        Proof::Tests(tests)
    }
}

fn is_path(span: &str) -> bool {
    !span.contains(char::is_whitespace)
        && (span.contains('/')
            || [
                ".rs", ".ts", ".tsx", ".sol", ".proto", ".md", ".sh", ".json", ".yml", ".toml",
            ]
            .iter()
            .any(|ext| span.ends_with(ext)))
}

/// The names of the tests that passed: the `test <name> ... ok` lines of libtest output.
pub fn passed_tests(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| {
            line.trim_end()
                .strip_prefix("test ")?
                .strip_suffix(" ... ok")
                .map(str::to_string)
        })
        .collect()
}

/// Whether `name` is a passed test's full name or its trailing path (`::`) or title (` > `)
/// segments.
fn has_passed(name: &str, passed: &BTreeSet<String>) -> bool {
    passed.contains(name)
        || passed.iter().any(|full| {
            full.strip_suffix(name)
                .is_some_and(|head| head.ends_with("::") || head.ends_with(" > "))
        })
}

/// A criterion compared loosely: case, runs of whitespace, surrounding quotes and a final full
/// stop don't count.
fn normalise(criterion: &str) -> String {
    // Quotes, then a full stop, then quotes again: `"text."` and `"text".` both come out as `text`.
    let quotes = ['"', '“', '”'];
    let text = criterion
        .trim()
        .trim_matches(quotes)
        .trim_end_matches('.')
        .trim_matches(quotes)
        .trim();
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Checks the rows against the closed issues' criteria and the tests that passed.
pub fn check(closed: &[Closed], rows: &[Row], passed: &BTreeSet<String>) -> Report {
    let mut report = Report::default();
    let quoted: BTreeSet<String> = rows.iter().map(|r| normalise(&r.criterion)).collect();
    let mut known = BTreeSet::new();
    for issue in closed {
        for criterion in &issue.criteria {
            let key = normalise(criterion);
            if !quoted.contains(&key) {
                report.problems.push(Problem::MissingCriterion {
                    issue: issue.issue,
                    criterion: criterion.clone(),
                });
            }
            known.insert(key);
        }
    }
    for row in rows {
        if !closed.is_empty() && !known.contains(&normalise(&row.criterion)) {
            report.unmatched.push(row.criterion.clone());
        }
        match &row.proof {
            Proof::Tests(tests) => {
                for test in tests.iter().filter(|t| !has_passed(t, passed)) {
                    report.problems.push(Problem::TestNotPassed {
                        criterion: row.criterion.clone(),
                        test: test.clone(),
                    });
                }
            }
            Proof::Manual(evidence) => report
                .manual
                .push((row.criterion.clone(), evidence.clone())),
            Proof::None => report.problems.push(Problem::NoProof {
                criterion: row.criterion.clone(),
            }),
        }
    }
    report
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}
