//! The `criteria` CI job's command (D95).
//!
//! - `criteria closes <pr-body.md>` prints the issues the PR body closes, one per line.
//! - `criteria check <pr-body.md> <issues-dir> <test-output>...` checks the PR's
//!   acceptance-criteria table against `<issues-dir>/<n>.md` for each closed issue and the tests
//!   that passed in the given libtest-format outputs. It prints the job summary in Markdown, and
//!   exits 1 when the check fails.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use criteria::{Closed, check, closed_issues, issue_criteria, passed_tests, pr_rows};

const USAGE: &str = "usage: criteria closes <pr-body.md>\n       criteria check <pr-body.md> <issues-dir> <test-output>...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["closes", body] => closes(body),
        ["check", body, issues, outputs @ ..] if !outputs.is_empty() => {
            run_check(body, issues, outputs)
        }
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("criteria: {message}");
            ExitCode::from(2)
        }
    }
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))
}

fn closes(body: &str) -> Result<ExitCode, String> {
    for issue in closed_issues(&read(body)?) {
        println!("{issue}");
    }
    Ok(ExitCode::SUCCESS)
}

fn run_check(body: &str, issues_dir: &str, outputs: &[&str]) -> Result<ExitCode, String> {
    let body = read(body)?;
    let mut closed = Vec::new();
    for issue in closed_issues(&body) {
        let path = Path::new(issues_dir).join(format!("{issue}.md"));
        let text = read(&path.to_string_lossy())?;
        closed.push(Closed {
            issue,
            criteria: issue_criteria(&text),
        });
    }
    let mut passed = BTreeSet::new();
    for output in outputs {
        passed.extend(passed_tests(&read(output)?));
    }
    let rows = pr_rows(&body);

    if closed.is_empty() {
        println!("This PR closes no issue, so only the tests its table names are checked.\n");
    }
    for c in closed.iter().filter(|c| c.criteria.is_empty()) {
        println!(
            "#{} has no checkboxes under an \"Acceptance criteria\" heading.\n",
            c.issue
        );
    }
    let report = check(&closed, &rows, &passed);
    print!("{}", report.summary());
    Ok(if report.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
