//! Aggregation for the reporting endpoints.
//!
//! The [`TestService`](super::TestService) walks the stored tree and hands the
//! counts it found to the pure functions here, which turn them into the
//! response models. Keeping the arithmetic separate from the walk keeps it
//! testable without a filesystem.

use crate::models::{CoverageReport, SuiteCoverage};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
