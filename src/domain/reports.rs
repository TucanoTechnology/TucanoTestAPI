//! Aggregation for the reporting endpoints.
//!
//! The [`TestService`](super::TestService) walks the stored tree and hands the
//! counts it found to the pure functions here, which turn them into the
//! response models. Keeping the arithmetic separate from the walk keeps it
//! testable without a filesystem.

use crate::models::{CoverageReport, SuiteCoverage, SummaryReport, TestCaseResult, TestRun};

use super::error::DomainError;

/// The cases one suite holds.
#[derive(Debug, Clone, PartialEq)]
pub struct SuiteCases {
    /// The suite's wire identifier, `<folder>.json`.
    pub suite_id: String,
    /// The suite's own name.
    pub name: String,
    /// How many cases the suite folder holds.
    pub case_count: usize,
}

/// The cases one project holds, split into those sitting directly in the
/// project folder and those inside each of its suites.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectCases {
    /// Cases held directly by the project rather than by a suite.
    pub direct: usize,
    /// One entry per suite, in the order the storage layer listed them.
    pub suites: Vec<SuiteCases>,
}

/// Which projects a report covers.
///
/// The caller resolves the scope before the walk: a trusted deployment and a
/// system administrator both ask for [`Scope::All`], a request naming one
/// project asks for [`Scope::Project`], and an ordinary caller gets the set of
/// projects it can reach as [`Scope::Projects`].
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Scope {
    /// Every project in the tree.
    #[default]
    All,
    /// Exactly the project `id`.
    Project(String),
    /// Exactly these projects. Unlike [`Scope::Project`] the identifiers are
    /// taken as given, so a caller that filtered by reachability can report on
    /// a project that has since been deleted without tripping its existence
    /// check.
    Projects(Vec<String>),
}

/// Builds a coverage report from the counts found under `project_id`, or under
/// every project when no scope was asked for.
///
/// A case may live directly in a project folder, so `total_cases` counts those
/// as well and can exceed the sum of the per-suite counts.
pub fn coverage(project_id: Option<&str>, projects: Vec<ProjectCases>) -> CoverageReport {
    let mut total_cases = 0;
    let mut suites = Vec::new();
    for project in projects {
        total_cases += project.direct;
        for suite in project.suites {
            total_cases += suite.case_count;
            suites.push(SuiteCoverage {
                suite_id: suite.suite_id,
                name: suite.name,
                case_count: suite.case_count,
            });
        }
    }

    CoverageReport {
        project_id: project_id.map(str::to_owned),
        total_cases,
        suites,
    }
}

/// The optional scope of a summary report.
///
/// Every field is optional and they combine conjunctively: a run is counted
/// only when it satisfies all of the filters that are set. With none set, every
/// run contributes. `from` and `to` bound the run's own timestamp, inclusive,
/// and may name a bare date or a full ISO-8601 timestamp.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SummaryFilters {
    pub project_id: Option<String>,
    pub milestone_id: Option<String>,
    pub configuration_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Sums the recorded results in scope into a summary report.
///
/// `total` counts every result. `passed`, `failed`, `blocked` and `untested`
/// are the named buckets; `Retest` and any unrecognised status contribute to
/// `total` (and therefore to the pass rate's denominator) but to no bucket, so
/// `passPercentage` never credits a result that was not a pass.
pub fn summary(results: &[TestCaseResult]) -> SummaryReport {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut blocked = 0usize;
    let mut untested = 0usize;
    let mut total_duration_ms = 0u64;

    for result in results {
        match result.status.as_str() {
            "Passed" => passed += 1,
            "Failed" => failed += 1,
            "Blocked" => blocked += 1,
            "Untested" => untested += 1,
            _ => {}
        }
        total_duration_ms += result.duration_ms.unwrap_or(0);
    }

    let total = results.len();
    let pass_percentage = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    SummaryReport {
        total,
        passed,
        failed,
        blocked,
        untested,
        pass_percentage,
        total_duration_ms,
    }
}

/// Whether a run belongs in a summary report.
///
/// `home` is the project the run is stored in, which counts as a match for a
/// `?projectId=` filter even when the run's own `projects` snapshot omits it:
/// the folder is the ownership fact, and a run executed against a project it
/// did not record is still that project's run.
///
/// `milestone_runs` is the set of run identifiers the milestone filter
/// references, resolved by the service from the milestone document before the
/// walk, so the matching here stays free of storage. The other filters come
/// straight from the request; `from` and `to` are expected already normalised
/// to `YYYY-MM-DD`.
pub fn run_is_in_scope(
    run: &TestRun,
    home: &str,
    run_id: &str,
    filters: &SummaryFilters,
    milestone_runs: Option<&[String]>,
) -> bool {
    if let Some(project_id) = filters.project_id.as_deref()
        && project_id != home
        && !run.projects.as_ref().is_some_and(|projects| {
            projects
                .iter()
                .any(|project| project.project_id == project_id)
        })
    {
        return false;
    }

    if let Some(run_ids) = milestone_runs
        && !run_ids.iter().any(|id| id == run_id)
    {
        return false;
    }

    if let Some(config_id) = filters.configuration_id.as_deref()
        && !run.configurations.as_ref().is_some_and(|configurations| {
            configurations
                .iter()
                .any(|configuration| configuration.config_id == config_id)
        })
    {
        return false;
    }

    if filters.from.is_some() || filters.to.is_some() {
        let Some(date) = run_date(&run.timestamp) else {
            return false;
        };
        if let Some(from) = filters.from.as_deref()
            && date.as_str() < from
        {
            return false;
        }
        if let Some(to) = filters.to.as_deref()
            && date.as_str() > to
        {
            return false;
        }
    }

    true
}

/// Whether every project a run belongs to is one the caller can reach.
///
/// The home anchors a run, so one whose `projects` snapshot is empty or absent
/// is reachable exactly when its home is; the old "must name at least one
/// project" condition is gone with it. Every project the snapshot does name
/// still has to be reachable, because a run embeds those projects' structure
/// and serving it to a caller who cannot reach them would disclose it.
/// [`run_is_in_scope`] is the caller's own filter and this one is the
/// authorization filter; they compose, and the more restrictive answer wins.
pub fn run_reachable(run: &TestRun, home: &str, reachable: &[String]) -> bool {
    reachable.iter().any(|id| id == home)
        && run.projects.as_ref().is_none_or(|projects| {
            projects
                .iter()
                .all(|project| reachable.iter().any(|id| id == &project.project_id))
        })
}

/// Normalises a caller-supplied date filter to `YYYY-MM-DD`.
///
/// A bare date and a full ISO-8601 timestamp both name a day; anything else is
/// a `400`.
pub fn parse_date_filter(value: &str) -> Result<String, DomainError> {
    date_prefix(value)
        .map(str::to_owned)
        .ok_or_else(|| DomainError::invalid_request(format!("Invalid date filter: {value}")))
}

/// The calendar date a run's `timestamp` falls on, as `YYYY-MM-DD`.
///
/// A run stores its timestamp in one of two shapes the API has always written:
/// Unix seconds rendered as a string (the default) or an ISO-8601 timestamp the
/// client supplied verbatim. Both reduce to a date prefix. A value in neither
/// shape has no comparable date and is left out of any date-filtered report.
fn run_date(timestamp: &str) -> Option<String> {
    if let Ok(seconds) = timestamp.parse::<u64>() {
        return Some(super::iso8601_date(seconds));
    }
    date_prefix(timestamp).map(str::to_owned)
}

/// The leading `YYYY-MM-DD` of `value`, when it carries one.
fn date_prefix(value: &str) -> Option<&str> {
    let date = value.get(..10)?;
    let bytes = date.as_bytes();
    let shaped = bytes[4] == b'-'
        && bytes[7] == b'-'
        && date[..4].bytes().all(|byte| byte.is_ascii_digit())
        && date[5..7].bytes().all(|byte| byte.is_ascii_digit())
        && date[8..10].bytes().all(|byte| byte.is_ascii_digit());
    shaped.then_some(date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Project, TestConfiguration};

    fn suite(suite_id: &str, case_count: usize) -> SuiteCases {
        SuiteCases {
            suite_id: suite_id.to_owned(),
            name: suite_id.trim_end_matches(".json").to_owned(),
            case_count,
        }
    }

    #[test]
    fn an_empty_scope_reports_no_cases_and_no_suites() {
        let report = coverage(None, Vec::new());

        assert_eq!(
            report,
            CoverageReport {
                project_id: None,
                total_cases: 0,
                suites: Vec::new(),
            }
        );
    }

    #[test]
    fn a_scoped_report_echoes_the_identifier() {
        let report = coverage(Some("checkout.json"), vec![ProjectCases::default()]);

        assert_eq!(report.project_id.as_deref(), Some("checkout.json"));
    }

    #[test]
    fn cases_held_directly_by_a_project_join_the_total() {
        let report = coverage(
            Some("checkout.json"),
            vec![ProjectCases {
                direct: 3,
                suites: vec![suite("smoke.json", 2)],
            }],
        );

        // Two cases in the suite and three directly in the project: the total
        // is the whole tree, not just the part the suites hold.
        assert_eq!(report.total_cases, 5);
        assert_eq!(
            report.suites,
            vec![SuiteCoverage {
                suite_id: "smoke.json".to_owned(),
                name: "smoke".to_owned(),
                case_count: 2,
            }]
        );
    }

    #[test]
    fn a_global_report_sums_every_project_and_keeps_their_suites() {
        let report = coverage(
            None,
            vec![
                ProjectCases {
                    direct: 1,
                    suites: vec![suite("smoke.json", 2)],
                },
                ProjectCases {
                    direct: 0,
                    suites: vec![suite("regression.json", 4), suite("nightly.json", 0)],
                },
            ],
        );

        assert!(report.project_id.is_none());
        assert_eq!(report.total_cases, 7);
        assert_eq!(
            report
                .suites
                .iter()
                .map(|entry| entry.suite_id.as_str())
                .collect::<Vec<_>>(),
            ["smoke.json", "regression.json", "nightly.json"]
        );
    }

    fn result(status: &str, duration_ms: Option<u64>) -> TestCaseResult {
        TestCaseResult {
            test_case_id: "TC-1".to_owned(),
            status: status.to_owned(),
            timestamp: "1".to_owned(),
            notes: None,
            duration_ms,
            attachments: None,
            defect_links: None,
        }
    }

    fn run(timestamp: &str) -> TestRun {
        TestRun {
            test_run_id: "R-1".to_owned(),
            timestamp: timestamp.to_owned(),
            name: None,
            projects: None,
            test_suites: None,
            test_cases: None,
            results: None,
            tags: None,
            configurations: None,
            case_versions: None,
        }
    }

    fn project(id: &str) -> Project {
        Project {
            project_id: id.to_owned(),
            name: id.to_owned(),
            ..Project::default()
        }
    }

    fn configuration(id: &str) -> TestConfiguration {
        TestConfiguration {
            config_id: id.to_owned(),
            name: id.to_owned(),
            ..TestConfiguration::default()
        }
    }

    /// A home distinct from every project the filters below name, so a test
    /// that is not about the home cannot be satisfied by it.
    const HOME: &str = "platform.json";

    #[test]
    fn an_empty_result_set_reports_zeroes() {
        assert_eq!(
            summary(&[]),
            SummaryReport {
                total: 0,
                passed: 0,
                failed: 0,
                blocked: 0,
                untested: 0,
                pass_percentage: 0.0,
                total_duration_ms: 0,
            }
        );
    }

    #[test]
    fn the_buckets_split_by_status_and_retest_counts_only_toward_the_total() {
        let report = summary(&[
            result("Passed", None),
            result("Failed", None),
            result("Blocked", None),
            result("Untested", None),
            result("Retest", None),
            result("Skipped", None),
        ]);

        assert_eq!(report.total, 6);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(report.blocked, 1);
        assert_eq!(report.untested, 1);
        // Two results are in neither a pass nor a failure: `Retest` and the
        // status the API does not recognise.
        assert_eq!(report.pass_percentage, 1.0 / 6.0 * 100.0);
    }

    #[test]
    fn the_issue_example_produces_the_documented_pass_rate() {
        let mut results = Vec::new();
        for _ in 0..95 {
            results.push(result("Passed", None));
        }
        for _ in 0..15 {
            results.push(result("Failed", None));
        }
        for _ in 0..6 {
            results.push(result("Blocked", None));
        }
        for _ in 0..4 {
            results.push(result("Untested", None));
        }

        let report = summary(&results);

        assert_eq!(report.total, 120);
        assert_eq!(report.passed, 95);
        assert_eq!(report.failed, 15);
        assert_eq!(report.blocked, 6);
        assert_eq!(report.untested, 4);
        assert_eq!(report.pass_percentage, 95.0 / 120.0 * 100.0);
    }

    #[test]
    fn durations_sum_and_a_result_without_one_counts_as_zero() {
        let report = summary(&[
            result("Passed", Some(1_500)),
            result("Failed", None),
            result("Passed", Some(2_000)),
        ]);

        assert_eq!(report.total_duration_ms, 3_500);
    }

    #[test]
    fn a_project_filter_keeps_only_runs_that_embed_it() {
        let mut filters = SummaryFilters {
            project_id: Some("checkout.json".to_owned()),
            ..SummaryFilters::default()
        };
        let linked = TestRun {
            projects: Some(vec![project("checkout.json")]),
            ..run("1")
        };
        let other = TestRun {
            projects: Some(vec![project("billing.json")]),
            ..run("1")
        };

        // A run stored in a third project is judged by what it embeds.
        assert!(run_is_in_scope(&linked, HOME, "R-1", &filters, None));
        assert!(!run_is_in_scope(&other, HOME, "R-1", &filters, None));

        // A run that embeds no project is left out too …
        assert!(!run_is_in_scope(&run("1"), HOME, "R-1", &filters, None));
        // … unless the filtered project is its home, which is now a match on
        // its own: the folder is the ownership fact the snapshot may omit.
        assert!(run_is_in_scope(
            &run("1"),
            "checkout.json",
            "R-1",
            &filters,
            None
        ));

        filters.project_id = None;
        assert!(run_is_in_scope(&run("1"), HOME, "R-1", &filters, None));
    }

    #[test]
    fn a_run_is_reachable_when_its_home_and_every_project_it_names_are() {
        let reachable = ["checkout.json".to_owned(), "billing.json".to_owned()];

        // The home alone anchors a run that names no project, which the old
        // "must name at least one project" rule left out.
        assert!(run_reachable(&run("1"), "checkout.json", &reachable));
        assert!(!run_reachable(&run("1"), HOME, &reachable));

        // A run naming projects needs every one of them, not just its home.
        let covered = TestRun {
            projects: Some(vec![project("checkout.json"), project("billing.json")]),
            ..run("1")
        };
        assert!(run_reachable(&covered, "checkout.json", &reachable));
        assert!(!run_reachable(&covered, HOME, &reachable));

        let crossing = TestRun {
            projects: Some(vec![project("checkout.json"), project(HOME)]),
            ..run("1")
        };
        assert!(
            !run_reachable(&crossing, "checkout.json", &reachable),
            "an unreachable covered project hides the run"
        );
    }

    #[test]
    fn a_milestone_filter_keeps_only_the_runs_it_references() {
        let filters = SummaryFilters {
            milestone_id: Some("v1.0.json".to_owned()),
            ..SummaryFilters::default()
        };
        let referenced = ["nightly.json".to_owned(), "weekly.json".to_owned()];

        assert!(run_is_in_scope(
            &run("1"),
            HOME,
            "nightly.json",
            &filters,
            Some(&referenced)
        ));
        assert!(!run_is_in_scope(
            &run("1"),
            HOME,
            "daily.json",
            &filters,
            Some(&referenced)
        ));
    }

    #[test]
    fn a_configuration_filter_keeps_only_linked_runs() {
        let filters = SummaryFilters {
            configuration_id: Some("chrome-linux.json".to_owned()),
            ..SummaryFilters::default()
        };
        let linked = TestRun {
            configurations: Some(vec![configuration("chrome-linux.json")]),
            ..run("1")
        };
        let other = TestRun {
            configurations: Some(vec![configuration("firefox-windows.json")]),
            ..run("1")
        };

        assert!(run_is_in_scope(&linked, HOME, "R-1", &filters, None));
        assert!(!run_is_in_scope(&other, HOME, "R-1", &filters, None));
        assert!(!run_is_in_scope(&run("1"), HOME, "R-1", &filters, None));
    }

    #[test]
    fn date_bounds_are_inclusive() {
        let filters = SummaryFilters {
            from: Some("2026-09-10".to_owned()),
            to: Some("2026-09-12".to_owned()),
            ..SummaryFilters::default()
        };

        // A Unix-seconds timestamp on the lower bound day.
        assert!(run_is_in_scope(
            &run("1788998400"),
            HOME,
            "R-1",
            &filters,
            None
        ));
        // An ISO-8601 timestamp on the upper bound day.
        assert!(run_is_in_scope(
            &run("2026-09-12T23:59:59Z"),
            HOME,
            "R-1",
            &filters,
            None
        ));
        assert!(!run_is_in_scope(
            &run("2026-09-09T23:59:59Z"),
            HOME,
            "R-1",
            &filters,
            None
        ));
        assert!(!run_is_in_scope(
            &run("2026-09-13T00:00:00Z"),
            HOME,
            "R-1",
            &filters,
            None
        ));
    }

    #[test]
    fn a_run_without_a_comparable_date_is_left_out_when_a_date_filter_is_set() {
        let filters = SummaryFilters {
            from: Some("2026-09-10".to_owned()),
            ..SummaryFilters::default()
        };

        assert!(!run_is_in_scope(
            &run("not a date"),
            HOME,
            "R-1",
            &filters,
            None
        ));
        // Without a date filter the same run is in scope.
        assert!(run_is_in_scope(
            &run("not a date"),
            HOME,
            "R-1",
            &SummaryFilters::default(),
            None
        ));
    }

    #[test]
    fn a_date_filter_accepts_a_bare_date_or_a_timestamp() {
        assert_eq!(
            parse_date_filter("2026-09-10").expect("bare date"),
            "2026-09-10"
        );
        assert_eq!(
            parse_date_filter("2026-09-10T12:00:00Z").expect("timestamp"),
            "2026-09-10"
        );

        for value in ["", "2026", "2026/09/10", "yesterday", "2026-9-1"] {
            let error = parse_date_filter(value).expect_err("unusable date");
            assert!(
                matches!(error, DomainError::InvalidRequest { code, .. } if code == "invalid_request"),
                "value {value:?} produced {error:?}"
            );
        }
    }
}
