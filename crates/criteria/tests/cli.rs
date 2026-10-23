//! The `criteria` binary as the CI job runs it (D95): exit 0 passes the job, 1 fails it, 2 is a
//! usage or read error; the summary goes to stdout.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ISSUE: &str = "## Acceptance criteria\n\n- [ ] It parses\n- [ ] A D-entry records it\n";
const CARGO_OUTPUT: &str = "test parse::it_parses ... ok\n";

/// A fresh directory for one test, holding `pr-body.md`, `issues/98.md` and `tests.log`.
fn setup(name: &str, pr_table: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("issues")).unwrap();
    let body = format!(
        "## Acceptance criteria\n\n| Criterion | Proved by |\n|---|---|\n{pr_table}\n\nCloses #98\n"
    );
    fs::write(dir.join("pr-body.md"), body).unwrap();
    fs::write(dir.join("issues/98.md"), ISSUE).unwrap();
    fs::write(dir.join("tests.log"), CARGO_OUTPUT).unwrap();
    dir
}

fn run_check(dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_criteria"))
        .arg("check")
        .arg(dir.join("pr-body.md"))
        .arg(dir.join("issues"))
        .arg(dir.join("tests.log"))
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_complete_table_passes_the_job_and_lists_manual_rows() {
    let dir = setup(
        "complete",
        "| It parses | `it_parses` |\n| A D-entry records it | manual: D95 |",
    );
    let output = run_check(&dir);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(stdout(&output).contains("| A D-entry records it | D95 |"));
}

#[test]
fn a_test_that_does_not_exist_fails_the_job() {
    let dir = setup(
        "missing-test",
        "| It parses | `no_such_test` |\n| A D-entry records it | manual: D95 |",
    );
    let output = run_check(&dir);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(stdout(&output).contains("`no_such_test`"));
}

#[test]
fn an_omitted_criterion_fails_the_job() {
    let dir = setup("omitted", "| It parses | `it_parses` |");
    let output = run_check(&dir);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(stdout(&output).contains("No row for #98's criterion \"A D-entry records it\""));
}

#[test]
fn an_unreadable_row_fails_the_job() {
    let dir = setup("unreadable", "| It parses | the tests |");
    let output = run_check(&dir);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(stdout(&output).contains("names no test in backticks"));
}

#[test]
fn a_closed_issue_that_was_not_fetched_is_an_error() {
    let dir = setup("unfetched", "| It parses | `it_parses` |");
    fs::remove_file(dir.join("issues/98.md")).unwrap();
    assert_eq!(run_check(&dir).status.code(), Some(2));
}

#[test]
fn closes_prints_the_closed_issues() {
    let dir = setup("closes", "");
    let output = Command::new(env!("CARGO_BIN_EXE_criteria"))
        .arg("closes")
        .arg(dir.join("pr-body.md"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "98\n");
}

#[test]
fn check_without_test_output_is_a_usage_error() {
    let dir = setup("no-output", "| It parses | `it_parses` |");
    let output = Command::new(env!("CARGO_BIN_EXE_criteria"))
        .arg("check")
        .arg(dir.join("pr-body.md"))
        .arg(dir.join("issues"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

const MISSED: &str =
    "crates/engine/src/outbox.rs:337:9: replace <impl Outbox for KafkaOutbox>::publish with ()\n";

/// A fresh directory for one test, holding `pr-body.md` with a "Missed mutants" table, and `missed.txt`.
fn setup_mutants(name: &str, table: &str, missed: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let body = format!(
        "## Missed mutants\n\n| Mutant | Why no test catches it |\n|---|---|\n{table}\n\nCloses #79\n"
    );
    fs::write(dir.join("pr-body.md"), body).unwrap();
    fs::write(dir.join("missed.txt"), missed).unwrap();
    dir
}

fn run_mutants(dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_criteria"))
        .arg("mutants")
        .arg(dir.join("pr-body.md"))
        .arg(dir.join("missed.txt"))
        .output()
        .unwrap()
}

#[test]
fn mutants_passes_the_job_when_every_miss_has_a_reason() {
    let dir = setup_mutants(
        "mutants-reasoned",
        "| `crates/engine/src/outbox.rs:337:9: replace <impl Outbox for KafkaOutbox>::publish with ()` | Needs a broker. |",
        MISSED,
    );
    let output = run_mutants(&dir);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(stdout(&output).contains("| Needs a broker. |"));
}

#[test]
fn mutants_fails_the_job_naming_a_miss_with_no_row() {
    let dir = setup_mutants("mutants-unlisted", "", MISSED);
    let output = run_mutants(&dir);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(stdout(&output).contains(
        "- `crates/engine/src/outbox.rs:337:9: replace <impl Outbox for KafkaOutbox>::publish with ()` was missed"
    ));
}

#[test]
fn mutants_with_nothing_missed_passes_the_job() {
    let dir = setup_mutants("mutants-none", "", "");
    let output = run_mutants(&dir);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_eq!(
        stdout(&output),
        "## Missed mutants\n\nNo mutant was missed.\n"
    );
}

#[test]
fn mutants_without_a_missed_list_is_an_error() {
    let dir = setup_mutants("mutants-no-list", "", MISSED);
    fs::remove_file(dir.join("missed.txt")).unwrap();
    assert_eq!(run_mutants(&dir).status.code(), Some(2));
}
