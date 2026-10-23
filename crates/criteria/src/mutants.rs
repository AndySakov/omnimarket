//! Checks a PR's "Missed mutants" table against the mutants its CI run missed (#137). Each missed
//! mutant needs a row giving the reason no test catches it; the reasons go to the job summary for
//! the watchdog to judge. A row for a mutant the run didn't miss is stale: reported, not failed.
//!
//! - A mutant is a line of cargo-mutants' `missed.txt`: `file:line:col: description`.
//! - The table sits under a `## Missed mutants` heading: the mutant in its first cell (backticks
//!   optional, `\|` for a pipe), the reason in its second.

use std::collections::BTreeSet;
use std::fmt;

use super::{escape_cell, table};

const SECTION: &str = "missed mutants";

/// One row of a PR's "Missed mutants" table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutantRow {
    /// The mutant as cargo-mutants prints it.
    pub mutant: String,
    /// Why no test catches it. Empty when the row gives none.
    pub reason: String,
}

/// One reason the check fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutantProblem {
    /// A missed mutant has no row.
    NoRow { mutant: String },
    /// A missed mutant's row gives no reason.
    NoReason { mutant: String },
}

impl fmt::Display for MutantProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MutantProblem::NoRow { mutant } => write!(
                f,
                "`{mutant}` was missed, and the PR body's \"Missed mutants\" table has no row for it"
            ),
            MutantProblem::NoReason { mutant } => {
                write!(f, "The row for `{mutant}` gives no reason")
            }
        }
    }
}

/// The outcome of [`check_mutants`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MutantReport {
    /// Empty when the PR passes.
    pub problems: Vec<MutantProblem>,
    /// `(mutant, reason)` for each missed mutant whose row gives a reason.
    pub reasons: Vec<(String, String)>,
    /// Rows for mutants this run didn't miss: caught now, or their code is gone.
    pub stale: Vec<String>,
}

impl MutantReport {
    /// The job summary, in Markdown: the problems, the reasons for the watchdog, then stale rows.
    pub fn summary(&self) -> String {
        let mut out = String::from("## Missed mutants\n\n");
        if !self.problems.is_empty() {
            for problem in &self.problems {
                out.push_str(&format!("- {problem}\n"));
            }
        } else if self.reasons.is_empty() {
            out.push_str("No mutant was missed.\n");
        } else {
            out.push_str("Every missed mutant has a row with a reason.\n");
        }
        if !self.reasons.is_empty() {
            out.push_str(
                "\n### Reasons for the watchdog\n\n| Mutant | Why no test catches it |\n|---|---|\n",
            );
            for (mutant, reason) in &self.reasons {
                out.push_str(&format!(
                    "| `{}` | {} |\n",
                    escape_cell(mutant),
                    escape_cell(reason)
                ));
            }
        }
        if !self.stale.is_empty() {
            out.push_str(
                "\n### Stale rows\n\nThis run didn't miss these, so their rows can go:\n\n",
            );
            for mutant in &self.stale {
                out.push_str(&format!("- `{mutant}`\n"));
            }
        }
        out
    }
}

/// The rows of a PR body's "Missed mutants" table.
pub fn mutant_rows(body: &str) -> Vec<MutantRow> {
    table(body, SECTION)
        .into_iter()
        .filter(|cells| !cells.is_empty())
        .map(|cells| MutantRow {
            mutant: cells[0].clone(),
            reason: cells
                .get(1)
                .map(|c| c.trim().to_string())
                .unwrap_or_default(),
        })
        .collect()
}

/// The mutants a run missed: the lines of cargo-mutants' `missed.txt`.
pub fn missed_mutants(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// A mutant compared loosely: surrounding backticks and runs of whitespace don't count.
fn key(mutant: &str) -> String {
    mutant
        .trim()
        .trim_matches('`')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Checks that every missed mutant has a row with a reason.
pub fn check_mutants(missed: &[String], rows: &[MutantRow]) -> MutantReport {
    let mut report = MutantReport::default();
    for mutant in missed {
        match rows.iter().find(|r| key(&r.mutant) == key(mutant)) {
            None => report.problems.push(MutantProblem::NoRow {
                mutant: mutant.clone(),
            }),
            Some(row) if row.reason.is_empty() => report.problems.push(MutantProblem::NoReason {
                mutant: mutant.clone(),
            }),
            Some(row) => report.reasons.push((mutant.clone(), row.reason.clone())),
        }
    }
    let missed: BTreeSet<String> = missed.iter().map(|m| key(m)).collect();
    report.stale = rows
        .iter()
        .filter(|r| !missed.contains(&key(&r.mutant)))
        .map(|r| key(&r.mutant))
        .collect();
    report
}
