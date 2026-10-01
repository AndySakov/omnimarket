//! The `criteria` CI job's rules (D92): every acceptance criterion of the issue a PR closes has a
//! row in the PR's table, every test a row names passed in this CI run, and `manual:` rows are
//! listed for the watchdog.

use criteria::{
    Closed, Problem, Proof, ReadError, Row, check, closed_issues, issue_criteria, passed_tests,
    pr_rows,
};

const ISSUE: &str = "\
## Why

Something.

## Acceptance criteria

- [ ] A PR that names a test that doesn't exist fails the job
- [x] `manual:` rows are listed in the job summary
  for the watchdog to check
- [ ] A D-entry records it

## Blocked by

None.
";

fn pr_body(table: &str) -> String {
    format!(
        "## Problem\n\nSomething.\n\n## Acceptance criteria\n\n\
         | Criterion (from #98) | Proved by |\n|---|---|\n{table}\n\n## Not verified\n\nNothing.\n\nCloses #98\n"
    )
}

const CARGO_OUTPUT: &str = "\
     Running tests/check.rs (target/debug/deps/check-0123)

running 3 tests
test a_missing_test_fails ... ok
test parse::rows_split_on_pipes ... ok
test an_ignored_test ... ignored
test a_failing_test ... FAILED

test result: FAILED. 2 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out
";

fn criteria_of_98() -> Vec<Closed> {
    vec![Closed {
        issue: 98,
        criteria: issue_criteria(ISSUE),
    }]
}

fn tests(names: &[&str]) -> Proof {
    Proof::Tests(names.iter().map(|n| n.to_string()).collect())
}

#[test]
fn the_issue_criteria_are_its_checkbox_items_with_wrapped_lines_joined() {
    assert_eq!(
        issue_criteria(ISSUE),
        vec![
            "A PR that names a test that doesn't exist fails the job",
            "`manual:` rows are listed in the job summary for the watchdog to check",
            "A D-entry records it",
        ]
    );
}

#[test]
fn an_issue_without_an_acceptance_criteria_section_has_none() {
    assert!(issue_criteria("## Why\n\n- [ ] not a criterion\n").is_empty());
}

#[test]
fn checkboxes_outside_the_acceptance_criteria_section_are_not_criteria() {
    let body = "## Tasks\n\n- [ ] a task\n\n## Acceptance criteria\n\n- [ ] the one\n\n## Notes\n\n- [ ] a note\n";
    assert_eq!(issue_criteria(body), vec!["the one"]);
}

#[test]
fn closing_keywords_name_the_closed_issues() {
    let body = "Closes #98\nfixes #7, and Resolves #12. Refs #3. Closed #4 resolved #5";
    assert_eq!(closed_issues(body), vec![4, 5, 7, 12, 98]);
}

#[test]
fn a_closing_keyword_inside_a_word_closes_nothing() {
    assert!(closed_issues("encloses #9 and prefixes #10").is_empty());
}

#[test]
fn the_pr_table_rows_carry_tests_and_manual_evidence() {
    let body = pr_body(
        "| A PR that names a test that doesn't exist fails the job | `a_missing_test_fails` in `crates/criteria/tests/check.rs`, `parse::rows_split_on_pipes` |\n\
         | \"A D-entry records it.\" | manual: D92 in `docs/spec/decisions.md` |",
    );
    assert_eq!(
        pr_rows(&body),
        Ok(vec![
            Row {
                criterion: "A PR that names a test that doesn't exist fails the job".into(),
                proof: tests(&["a_missing_test_fails", "parse::rows_split_on_pipes"]),
            },
            Row {
                criterion: "\"A D-entry records it.\"".into(),
                proof: Proof::Manual("D92 in `docs/spec/decisions.md`".into()),
            },
        ])
    );
}

#[test]
fn an_escaped_pipe_stays_inside_its_cell() {
    let body = pr_body("| a \\| b | `t` |");
    assert_eq!(pr_rows(&body).unwrap()[0].criterion, "a | b");
}

#[test]
fn a_row_without_a_test_or_manual_evidence_is_an_error() {
    let body = pr_body("| A D-entry records it | see the diff |");
    assert_eq!(
        pr_rows(&body),
        Err(ReadError::NoProof {
            criterion: "A D-entry records it".into()
        })
    );
}

#[test]
fn a_pr_body_without_the_section_has_no_rows() {
    assert_eq!(pr_rows("## Problem\n\nx\n\nCloses #98\n"), Ok(vec![]));
}

#[test]
fn passed_tests_are_the_ok_lines_of_libtest_output() {
    let passed = passed_tests(CARGO_OUTPUT);
    assert_eq!(
        passed.into_iter().collect::<Vec<_>>(),
        vec!["a_missing_test_fails", "parse::rows_split_on_pipes"]
    );
}

#[test]
fn a_full_table_of_passing_tests_passes() {
    let rows = vec![
        Row {
            criterion: "a pr that names a test that doesn't exist fails the job".into(),
            proof: tests(&["a_missing_test_fails"]),
        },
        Row {
            criterion: "“`manual:` rows are listed in the job summary for the watchdog to check.”"
                .into(),
            proof: tests(&["rows_split_on_pipes"]),
        },
        Row {
            criterion: "A  D-entry records it".into(),
            proof: Proof::Manual("D92".into()),
        },
    ];
    let report = check(&criteria_of_98(), &rows, &passed_tests(CARGO_OUTPUT));
    assert_eq!(report.problems, vec![]);
    assert_eq!(
        report.manual,
        vec![("A  D-entry records it".to_string(), "D92".to_string())]
    );
}

#[test]
fn a_named_test_that_does_not_exist_fails() {
    let rows = vec![Row {
        criterion: "A D-entry records it".into(),
        proof: tests(&["no_such_test"]),
    }];
    let report = check(&criteria_of_98(), &rows, &passed_tests(CARGO_OUTPUT));
    assert!(report.problems.contains(&Problem::TestNotPassed {
        criterion: "A D-entry records it".into(),
        test: "no_such_test".into(),
    }));
}

#[test]
fn a_named_test_that_failed_or_was_ignored_fails() {
    for test in ["a_failing_test", "an_ignored_test"] {
        let rows = vec![Row {
            criterion: "A D-entry records it".into(),
            proof: tests(&[test]),
        }];
        let report = check(&criteria_of_98(), &rows, &passed_tests(CARGO_OUTPUT));
        assert!(
            report.problems.contains(&Problem::TestNotPassed {
                criterion: "A D-entry records it".into(),
                test: test.into(),
            }),
            "{test}: {:?}",
            report.problems
        );
    }
}

#[test]
fn a_test_name_matches_only_a_whole_path_segment() {
    // `pipes` is the end of `parse::rows_split_on_pipes` but not a test of its own.
    let rows = vec![Row {
        criterion: "A D-entry records it".into(),
        proof: tests(&["pipes"]),
    }];
    let report = check(&criteria_of_98(), &rows, &passed_tests(CARGO_OUTPUT));
    assert!(!report.problems.is_empty());
}

#[test]
fn an_omitted_criterion_fails() {
    let rows = vec![Row {
        criterion: "A D-entry records it".into(),
        proof: Proof::Manual("D92".into()),
    }];
    let report = check(&criteria_of_98(), &rows, &passed_tests(CARGO_OUTPUT));
    assert_eq!(
        report.problems,
        vec![
            Problem::MissingCriterion {
                issue: 98,
                criterion: "A PR that names a test that doesn't exist fails the job".into(),
            },
            Problem::MissingCriterion {
                issue: 98,
                criterion: "`manual:` rows are listed in the job summary for the watchdog to check"
                    .into(),
            },
        ]
    );
}

#[test]
fn a_row_that_quotes_no_criterion_is_reported() {
    let rows = vec![Row {
        criterion: "Something the issue never asked".into(),
        proof: tests(&["a_missing_test_fails"]),
    }];
    let report = check(&criteria_of_98(), &rows, &passed_tests(CARGO_OUTPUT));
    assert!(report.problems.contains(&Problem::UnknownCriterion {
        criterion: "Something the issue never asked".into(),
    }));
}

#[test]
fn without_a_closed_issue_only_the_named_tests_are_checked() {
    let rows = vec![Row {
        criterion: "Anything".into(),
        proof: tests(&["a_missing_test_fails"]),
    }];
    let report = check(&[], &rows, &passed_tests(CARGO_OUTPUT));
    assert_eq!(report.problems, vec![]);
}

#[test]
fn the_summary_lists_manual_rows_and_problems() {
    let rows = vec![Row {
        criterion: "A D-entry records it".into(),
        proof: Proof::Manual("D92 in decisions.md".into()),
    }];
    let report = check(&criteria_of_98(), &rows, &passed_tests(CARGO_OUTPUT));
    let summary = report.summary();
    assert!(
        summary.contains("## Manual evidence for the watchdog"),
        "{summary}"
    );
    assert!(
        summary.contains("| A D-entry records it | D92 in decisions.md |"),
        "{summary}"
    );
    assert!(summary.contains("No row for #98's criterion"), "{summary}");
}

#[test]
fn frontend_test_names_match_their_last_title_segments() {
    let output = "test tests/visual/header.spec.ts > global header visual baselines > desktop shell ... ok\n";
    let passed = passed_tests(output);
    for name in [
        "desktop shell",
        "global header visual baselines > desktop shell",
    ] {
        let rows = vec![Row {
            criterion: "A D-entry records it".into(),
            proof: tests(&[name]),
        }];
        assert_eq!(check(&[], &rows, &passed).problems, vec![], "{name}");
    }
    let rows = vec![Row {
        criterion: "A D-entry records it".into(),
        proof: tests(&["shell"]),
    }];
    assert!(!check(&[], &rows, &passed).problems.is_empty());
}
