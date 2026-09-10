//! Milestone progress maths.
//!
//! Progress derives from the *stored results of the runs the milestone
//! references*, never from live source documents, so a milestone keeps reporting
//! the same figures as the runs it executed.

use crate::models::{Milestone, MilestoneProgress, TestRun};

/// Aggregates the result counters of every run linked to a milestone.
///
/// A run contributes its declared case count when it has one; when none of the
/// linked runs declare cases the counters themselves become the total, which is
/// how the legacy handler reported progress for partial runs.
pub fn compute(milestone: &Milestone, runs: &[TestRun]) -> MilestoneProgress {
    let mut total_cases = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut blocked = 0usize;
    let mut untested = 0usize;
    let mut retest = 0usize;

    for run in runs {
        if let Some(cases) = run.test_cases.as_ref() {
            total_cases += cases.len();
        }
        if let Some(results) = run.results.as_ref() {
            for result in results {
                match result.status.as_str() {
                    "Passed" => passed += 1,
                    "Failed" => failed += 1,
                    "Blocked" => blocked += 1,
                    "Untested" => untested += 1,
                    "Retest" => retest += 1,
                    _ => {}
                }
            }
        }
    }

    if total_cases == 0 {
        total_cases = passed + failed + blocked + untested + retest;
    }

    let pass_percentage = if total_cases > 0 {
        (passed as f64 / total_cases as f64) * 100.0
    } else {
        0.0
    };

    MilestoneProgress {
        milestone_id: milestone.milestone_id.clone(),
        total_cases,
        passed,
        failed,
        blocked,
        untested,
        retest,
        pass_percentage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{TestCase, TestCaseResult};

    fn milestone(id: &str, run_ids: Option<Vec<String>>) -> Milestone {
        Milestone {
            milestone_id: id.to_owned(),
            name: "v1.0".to_owned(),
            description: None,
            start_date: None,
            target_date: None,
            status: None,
            test_suite_ids: None,
            test_run_ids: run_ids,
        }
    }

    fn run(results: &[(&str, &str)], case_count: Option<usize>) -> TestRun {
        TestRun {
            test_run_id: "R".to_owned(),
            timestamp: "1".to_owned(),
            name: None,
            projects: None,
            test_suites: None,
            test_cases: case_count.map(|count| {
                (0..count)
                    .map(|index| TestCase {
                        test_case_id: format!("TC-{index}"),
                        title: "case".to_owned(),
                        description: None,
                        preconditions: None,
                        steps: None,
                        expected_result: "expected".to_owned(),
                        priority: None,
                        severity: None,
                        test_type: None,
                        exploratory: None,
                        attachments: None,
                        tags: None,
                    })
                    .collect()
            }),
            results: Some(
                results
                    .iter()
                    .map(|(case_id, status)| TestCaseResult {
                        test_case_id: (*case_id).to_owned(),
                        status: (*status).to_owned(),
                        timestamp: "1".to_owned(),
                        notes: None,
                        attachments: None,
                    })
                    .collect(),
            ),
            tags: None,
            configurations: None,
        }
    }

    #[test]
    fn counters_aggregate_across_every_linked_run() {
        let runs = [
            run(&[("TC-1", "Passed")], Some(2)),
            run(&[("TC-2", "Failed"), ("TC-3", "Blocked")], None),
        ];
        let progress = compute(&milestone("M-1", Some(vec!["R-1".to_owned()])), &runs);

        assert_eq!(progress.milestone_id, "M-1");
        assert_eq!(progress.total_cases, 2);
        assert_eq!(progress.passed, 1);
        assert_eq!(progress.failed, 1);
        assert_eq!(progress.blocked, 1);
        assert_eq!(progress.pass_percentage, 50.0);
    }

    #[test]
    fn without_declared_cases_the_counters_become_the_total() {
        let runs = [run(
            &[("TC-1", "Passed"), ("TC-2", "Failed"), ("TC-3", "Blocked")],
            None,
        )];
        let progress = compute(&milestone("M-1", None), &runs);

        assert_eq!(progress.total_cases, 3);
        assert_eq!(progress.blocked, 1);
        assert_eq!(progress.pass_percentage, 33.33333333333333);
    }

    #[test]
    fn untested_and_retest_results_are_counted() {
        let runs = [run(
            &[
                ("TC-1", "Untested"),
                ("TC-2", "Retest"),
                ("TC-3", "Skipped"),
            ],
            None,
        )];
        let progress = compute(&milestone("M-1", None), &runs);

        assert_eq!(progress.untested, 1);
        assert_eq!(progress.retest, 1);
        assert_eq!(progress.total_cases, 2);
    }

    #[test]
    fn no_runs_reports_zero_everywhere() {
        let progress = compute(&milestone("M-1", None), &[]);

        assert_eq!(progress.total_cases, 0);
        assert_eq!(progress.passed, 0);
        assert_eq!(progress.pass_percentage, 0.0);
    }

    #[test]
    fn the_progress_identifier_comes_from_the_document_not_the_call_site() {
        let progress = compute(&milestone("M-from-document", None), &[]);
        assert_eq!(progress.milestone_id, "M-from-document");
    }
}
