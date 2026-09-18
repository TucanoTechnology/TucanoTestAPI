//! Milestone progress maths.
//!
//! Progress derives from the *stored results of the runs the milestone
//! references*, never from live source documents, so a milestone keeps reporting
//! the same figures as the runs it executed.

use std::collections::BTreeMap;

use crate::models::{Milestone, MilestoneProgress, TestRun};

/// Aggregates the case counters of every run linked to a milestone.
///
/// A run contributes one entry per case it **holds**: the cases its snapshot
/// declares — its own `testCases` and those embedded in the suites it links —
/// plus any case it recorded a result for, because an import or a direct
/// recording may name a case the snapshot never declared. Each case is counted
/// once, in exactly one bucket, so the five buckets always add up to
/// `totalCases` and `passPercentage` divides by the population it counted.
pub fn compute(milestone: &Milestone, runs: &[TestRun]) -> MilestoneProgress {
    let mut total_cases = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut blocked = 0usize;
    let mut untested = 0usize;
    let mut retest = 0usize;

    for run in runs {
        // A case id maps to the status the run recorded for it, or `None` when
        // the run holds the case without having decided it yet. Declared cases
        // go in first and results second, so a case that appears in both is
        // held once and carries its recorded status.
        let mut population: BTreeMap<&str, Option<&str>> = BTreeMap::new();
        for case in run.test_cases.as_deref().unwrap_or_default() {
            population.insert(case.test_case_id.as_str(), None);
        }
        for suite in run.test_suites.as_deref().unwrap_or_default() {
            for case in &suite.test_cases {
                population.insert(case.test_case_id.as_str(), None);
            }
        }
        for result in run.results.as_deref().unwrap_or_default() {
            population.insert(result.test_case_id.as_str(), Some(result.status.as_str()));
        }

        for status in population.values() {
            total_cases += 1;
            match *status {
                Some("Passed") => passed += 1,
                Some("Failed") => failed += 1,
                Some("Blocked") => blocked += 1,
                Some("Retest") => retest += 1,
                // `Untested` recorded explicitly, a case the run holds without a
                // result, and a stored status this API does not recognise all
                // describe the same thing here: not yet decided.
                _ => untested += 1,
            }
        }
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
    use crate::models::{TestCase, TestCaseResult, TestSuite};

    fn case(id: &str) -> TestCase {
        TestCase {
            test_case_id: id.to_owned(),
            title: "case".to_owned(),
            expected_result: "expected".to_owned(),
            ..TestCase::default()
        }
    }

    fn result(case_id: &str, status: &str) -> TestCaseResult {
        TestCaseResult {
            test_case_id: case_id.to_owned(),
            status: status.to_owned(),
            timestamp: "1".to_owned(),
            notes: None,
            duration_ms: None,
            attachments: None,
            defect_links: None,
        }
    }

    fn suite(suite_id: &str, case_ids: &[&str]) -> TestSuite {
        TestSuite {
            suite_id: suite_id.to_owned(),
            name: suite_id.to_owned(),
            test_cases: case_ids.iter().map(|id| case(id)).collect(),
            ..TestSuite::default()
        }
    }

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

    /// A run holding `case_count` declared cases named `TC-0..`, the given linked
    /// suites, and the given recorded results.
    fn run_holding(
        results: &[(&str, &str)],
        case_count: Option<usize>,
        suites: &[TestSuite],
    ) -> TestRun {
        TestRun {
            test_run_id: "R".to_owned(),
            timestamp: "1".to_owned(),
            name: None,
            projects: None,
            test_suites: if suites.is_empty() {
                None
            } else {
                Some(suites.to_vec())
            },
            test_cases: case_count.map(|count| {
                (0..count)
                    .map(|index| case(&format!("TC-{index}")))
                    .collect()
            }),
            results: Some(
                results
                    .iter()
                    .map(|(case_id, status)| result(case_id, status))
                    .collect(),
            ),
            tags: None,
            configurations: None,
            case_versions: None,
        }
    }

    fn run(results: &[(&str, &str)], case_count: Option<usize>) -> TestRun {
        run_holding(results, case_count, &[])
    }

    fn buckets(progress: &MilestoneProgress) -> usize {
        progress.passed + progress.failed + progress.blocked + progress.untested + progress.retest
    }

    #[test]
    fn a_run_contributes_every_case_it_holds() {
        let runs = [
            run(&[("TC-1", "Passed")], Some(2)),
            run(&[("TC-2", "Failed"), ("TC-3", "Blocked")], None),
        ];
        let progress = compute(&milestone("M-1", Some(vec!["R-1".to_owned()])), &runs);

        assert_eq!(progress.milestone_id, "M-1");
        // TC-0 is declared and undecided, TC-1 is declared and passed, and TC-2
        // and TC-3 are recorded without being declared: four cases, one bucket
        // each.
        assert_eq!(progress.total_cases, 4);
        assert_eq!(progress.passed, 1);
        assert_eq!(progress.failed, 1);
        assert_eq!(progress.blocked, 1);
        assert_eq!(progress.untested, 1);
        assert_eq!(progress.pass_percentage, 25.0);
    }

    #[test]
    fn a_case_declared_and_recorded_counts_once_with_its_recorded_status() {
        let runs = [run(&[("TC-0", "Failed"), ("TC-1", "Passed")], Some(2))];
        let progress = compute(&milestone("M-1", None), &runs);

        assert_eq!(progress.total_cases, 2);
        assert_eq!(progress.passed, 1);
        assert_eq!(progress.failed, 1);
        assert_eq!(progress.untested, 0);
    }

    #[test]
    fn cases_embedded_in_a_linked_suite_are_part_of_the_population() {
        let runs = [run_holding(
            &[("TC-1", "Passed")],
            Some(1),
            &[suite("SMOKE", &["TC-0", "TC-1"])],
        )];
        let progress = compute(&milestone("M-1", None), &runs);

        // The declared TC-0 and the suite's TC-0 and TC-1 dedupe to TC-0 and
        // TC-1, the latter with its recorded status.
        assert_eq!(progress.total_cases, 2);
        assert_eq!(progress.passed, 1);
        assert_eq!(progress.untested, 1);
        assert_eq!(progress.pass_percentage, 50.0);
    }

    #[test]
    fn results_beyond_the_declared_snapshot_extend_the_population() {
        let runs = [run(
            &[("TC-1", "Passed"), ("TC-2", "Failed"), ("TC-3", "Blocked")],
            Some(1),
        )];
        let progress = compute(&milestone("M-1", None), &runs);

        assert_eq!(progress.total_cases, 4);
        assert_eq!(progress.untested, 1);
        assert_eq!(buckets(&progress), progress.total_cases);
        assert_eq!(progress.pass_percentage, 25.0);
    }

    #[test]
    fn a_held_case_without_a_result_and_an_unknown_status_both_count_as_untested() {
        let runs = [run(&[("TC-1", "Untested"), ("TC-2", "Skipped")], Some(1))];
        let progress = compute(&milestone("M-1", None), &runs);

        assert_eq!(progress.total_cases, 3);
        assert_eq!(progress.untested, 3);
        assert_eq!(progress.retest, 0);
        assert_eq!(progress.pass_percentage, 0.0);
    }

    #[test]
    fn retest_keeps_its_own_bucket() {
        let runs = [run(&[("TC-1", "Retest")], None)];
        let progress = compute(&milestone("M-1", None), &runs);

        assert_eq!(progress.total_cases, 1);
        assert_eq!(progress.retest, 1);
        assert_eq!(progress.untested, 0);
    }

    #[test]
    fn every_shape_of_run_keeps_the_buckets_summing_to_the_total() {
        let shapes = [
            run(&[], None),
            run(&[("TC-1", "Passed")], None),
            run(&[("TC-0", "Passed"), ("TC-1", "Failed")], Some(2)),
            run_holding(&[("TC-9", "Weird")], Some(1), &[suite("S", &["TC-9"])]),
        ];
        for shape in shapes {
            let progress = compute(&milestone("M-1", None), &[shape]);
            assert_eq!(buckets(&progress), progress.total_cases);
            assert!((0.0..=100.0).contains(&progress.pass_percentage));
        }
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
