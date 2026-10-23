//! The `mutants` job's rule (#137): every mutant the PR's run missed has a row in the PR body's
//! "Missed mutants" table with a reason, the reasons are listed for the watchdog, and rows for
//! mutants the run didn't miss are reported as stale without failing.

use criteria::mutants::{
    MutantProblem, MutantReport, MutantRow, check_mutants, missed_mutants, mutant_rows,
};
use criteria::pr_rows;

/// #131's `missed.txt`, as its `mutants` job printed it.
const MISSED_131: &str = "\
crates/engine/src/outbox.rs:337:9: replace <impl Outbox for KafkaOutbox>::publish with ()
crates/engine/src/outbox.rs:343:9: replace <impl Outbox for KafkaOutbox>::publish_trade with ()
crates/engine/src/outbox.rs:349:9: replace <impl Outbox for KafkaOutbox>::publish_price with ()
crates/engine/src/outbox.rs:355:9: replace <impl Outbox for KafkaOutbox>::publish_status with ()
";

fn pr_body(table: &str) -> String {
    format!(
        "## Problem\n\nSomething.\n\n## Missed mutants\n\n\
         | Mutant | Why no test catches it |\n|---|---|\n{table}\n\n## Not verified\n\nNothing.\n\nCloses #79\n"
    )
}

fn check(missed: &str, table: &str) -> MutantReport {
    check_mutants(&missed_mutants(missed), &mutant_rows(&pr_body(table)))
}

const PUBLISH: &str =
    "crates/engine/src/outbox.rs:337:9: replace <impl Outbox for KafkaOutbox>::publish with ()";

#[test]
fn a_missed_mutant_with_no_row_fails_naming_it() {
    let report = check(&format!("{PUBLISH}\n"), "");
    assert_eq!(
        report.problems,
        vec![MutantProblem::NoRow {
            mutant: PUBLISH.to_string()
        }]
    );
    assert_eq!(
        report.summary(),
        "## Missed mutants\n\n- `crates/engine/src/outbox.rs:337:9: replace <impl Outbox for KafkaOutbox>::publish with ()` \
         was missed, and the PR body's \"Missed mutants\" table has no row for it\n"
    );
}

#[test]
fn a_missed_mutant_whose_row_gives_a_reason_passes() {
    let report = check(
        &format!("{PUBLISH}\n"),
        &format!("| `{PUBLISH}` | Needs a broker; #92's end-to-end test covers it. |"),
    );
    assert_eq!(report.problems, vec![]);
    assert_eq!(
        report.reasons,
        vec![(
            PUBLISH.to_string(),
            "Needs a broker; #92's end-to-end test covers it.".to_string()
        )]
    );
}

#[test]
fn a_row_with_an_empty_reason_fails() {
    for row in [format!("| `{PUBLISH}` |  |"), format!("| `{PUBLISH}` |")] {
        assert_eq!(
            check(&format!("{PUBLISH}\n"), &row).problems,
            vec![MutantProblem::NoReason {
                mutant: PUBLISH.to_string()
            }],
            "{row}"
        );
    }
}

#[test]
fn pr_131s_four_kafka_outbox_misses_listed_with_reasons_pass() {
    let reason = "No offline test reaches KafkaOutbox: it needs a broker. #92's end-to-end test through CI's Kafka covers it.";
    let table: String = missed_mutants(MISSED_131)
        .iter()
        .map(|m| format!("| `{m}` | {reason} |\n"))
        .collect();
    let report = check(MISSED_131, &table);
    assert_eq!(report.problems, vec![]);
    assert_eq!(report.stale, Vec::<String>::new());
    assert_eq!(
        report.summary(),
        format!(
            "## Missed mutants\n\nEvery missed mutant has a row with a reason.\n\n\
             ### Reasons for the watchdog\n\n| Mutant | Why no test catches it |\n|---|---|\n\
             | `crates/engine/src/outbox.rs:337:9: replace <impl Outbox for KafkaOutbox>::publish with ()` | {reason} |\n\
             | `crates/engine/src/outbox.rs:343:9: replace <impl Outbox for KafkaOutbox>::publish_trade with ()` | {reason} |\n\
             | `crates/engine/src/outbox.rs:349:9: replace <impl Outbox for KafkaOutbox>::publish_price with ()` | {reason} |\n\
             | `crates/engine/src/outbox.rs:355:9: replace <impl Outbox for KafkaOutbox>::publish_status with ()` | {reason} |\n"
        )
    );
}

#[test]
fn a_row_for_a_mutant_the_run_did_not_miss_is_stale_and_passes() {
    let report = check("", &format!("| {PUBLISH} | It needed a broker. |"));
    assert_eq!(report.problems, vec![]);
    assert_eq!(
        report.summary(),
        format!(
            "## Missed mutants\n\nNo mutant was missed.\n\n### Stale rows\n\n\
             This run didn't miss these, so their rows can go:\n\n- `{PUBLISH}`\n"
        )
    );
}

#[test]
fn a_pipe_in_a_mutant_is_written_as_backslash_pipe() {
    let mutant = "crates/venues/src/v3.rs:88:20: replace || with && in tick_crossed";
    let report = check(
        &format!("{mutant}\n"),
        r"| `crates/venues/src/v3.rs:88:20: replace \|\| with && in tick_crossed` | Equivalent: both sides are false at a tick boundary. |",
    );
    assert_eq!(report.problems, vec![]);
    assert!(
        report
            .summary()
            .contains(r"| `crates/venues/src/v3.rs:88:20: replace \|\| with && in tick_crossed` |"),
        "{}",
        report.summary()
    );
}

#[test]
fn backticks_and_spacing_around_a_mutant_do_not_count() {
    let report = check(
        &format!("{PUBLISH}\n"),
        "|  `crates/engine/src/outbox.rs:337:9:  replace <impl Outbox for KafkaOutbox>::publish with ()`  | Needs a broker. |",
    );
    assert_eq!(report.problems, vec![]);
}

#[test]
fn another_line_or_column_is_another_mutant() {
    let moved = PUBLISH.replace(":337:9:", ":338:9:");
    let report = check(
        &format!("{PUBLISH}\n"),
        &format!("| `{moved}` | Needs a broker. |"),
    );
    assert_eq!(
        report.problems,
        vec![MutantProblem::NoRow {
            mutant: PUBLISH.to_string()
        }]
    );
    assert_eq!(report.stale, vec![moved]);
}

#[test]
fn the_mutants_table_and_the_criteria_table_are_read_apart() {
    let body = format!(
        "## Acceptance criteria\n\n| Criterion | Proved by |\n|---|---|\n| It parses | `it_parses` |\n\n\
         ## Missed mutants\n\n| Mutant | Why no test catches it |\n|---|---|\n| `{PUBLISH}` | Needs a broker. |\n"
    );
    assert_eq!(
        mutant_rows(&body),
        vec![MutantRow {
            mutant: format!("`{PUBLISH}`"),
            reason: "Needs a broker.".to_string()
        }]
    );
    assert_eq!(pr_rows(&body).len(), 1);
    assert_eq!(pr_rows(&body)[0].criterion, "It parses");
}

#[test]
fn a_body_with_no_table_has_no_rows() {
    assert_eq!(mutant_rows("## Problem\n\nNo table here.\n"), vec![]);
    assert_eq!(missed_mutants("\n\n"), Vec::<String>::new());
}
