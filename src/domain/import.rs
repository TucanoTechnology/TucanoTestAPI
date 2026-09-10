//! JUnit XML import: the mapping from a JUnit report to run results.
//!
//! A JUnit document is a tree of `<testsuite>` elements, each holding
//! `<testcase>` elements; a testcase sits at whatever depth its author chose, so
//! this walks every descendant rather than one fixed level. The parser is
//! read-only and matches by local name, so a namespaced document imports
//! exactly like a plain one.
//!
//! Every testcase it can name becomes a [`ParsedCase`]. One it cannot name — no
//! `name` attribute — is counted as an error rather than rejected, because a
//! report that is partly unusable should still import the part that is not.

use roxmltree::{Document, Node};

use super::error::DomainError;

/// The status a testcase maps to. Only three of the run's five statuses can come
/// from a report: JUnit distinguishes success, failure and skip, not the
/// `Untested` and `Retest` the API also stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    Passed,
    Failed,
    Blocked,
}

impl ImportStatus {
    /// The run status this maps to, as the API spells it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Blocked => "Blocked",
        }
    }
}

/// One testcase a report described, before it meets the run it lands in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCase {
    pub test_case_id: String,
    pub status: ImportStatus,
    pub notes: Option<String>,
    pub timestamp: Option<String>,
}

/// What a report contained: the cases it described and how many it could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReport {
    pub cases: Vec<ParsedCase>,
    pub errors: usize,
}

/// Reads a JUnit report, mapping every testcase it can name.
///
/// Malformed XML is an `invalid_request`; a testcase without a `name` is not an
/// error, it is counted in [`ParsedReport::errors`].
pub fn parse(xml: &str) -> Result<ParsedReport, DomainError> {
    let document =
        Document::parse(xml).map_err(|_| DomainError::invalid_request("Malformed JUnit XML"))?;

    let mut cases = Vec::new();
    let mut errors = 0;

    for testcase in document
        .descendants()
        .filter(|node| node.has_tag_name("testcase"))
    {
        match map_case(testcase) {
            Some(case) => cases.push(case),
            None => errors += 1,
        }
    }

    Ok(ParsedReport { cases, errors })
}

/// Maps one `<testcase>` to the result it records, or `None` when it cannot be
/// named.
fn map_case(testcase: Node<'_, '_>) -> Option<ParsedCase> {
    let name = testcase.attribute("name").filter(|name| !name.is_empty())?;
    let classname = testcase
        .attribute("classname")
        .filter(|classname| !classname.is_empty());

    let test_case_id = match classname {
        Some(classname) => format!("{classname}.{name}"),
        None => name.to_owned(),
    };

    Some(ParsedCase {
        test_case_id,
        status: status_of(testcase),
        notes: failure_message(testcase),
        timestamp: suite_timestamp(testcase),
    })
}

/// A testcase that failed or errored maps to `Failed`, one that was skipped to
/// `Blocked`, and everything else to `Passed`.
fn status_of(testcase: Node<'_, '_>) -> ImportStatus {
    if testcase
        .children()
        .any(|child| child.has_tag_name("failure") || child.has_tag_name("error"))
    {
        ImportStatus::Failed
    } else if testcase
        .children()
        .any(|child| child.has_tag_name("skipped"))
    {
        ImportStatus::Blocked
    } else {
        ImportStatus::Passed
    }
}

/// The `message` of the first failure or error, recorded as the result's notes.
fn failure_message(testcase: Node<'_, '_>) -> Option<String> {
    testcase
        .children()
        .find(|child| child.has_tag_name("failure") || child.has_tag_name("error"))
        .and_then(|child| child.attribute("message"))
        .map(str::to_owned)
}

/// The enclosing suite's `timestamp`, when the report carries one; the caller
/// substitutes the current time when it does not.
fn suite_timestamp(testcase: Node<'_, '_>) -> Option<String> {
    testcase
        .ancestors()
        .find(|node| node.has_tag_name("testsuite"))
        .and_then(|suite| suite.attribute("timestamp"))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_three_junit_outcomes_to_run_statuses() {
        let report = parse(
            r#"<testsuite classname="Checkout">
                 <testcase classname="Checkout" name="pays"/>
                 <testcase classname="Checkout" name="declines">
                   <failure message="card declined">stack</failure>
                 </testcase>
                 <testcase classname="Checkout" name="times out">
                   <error message="timeout">trace</error>
                 </testcase>
                 <testcase classname="Checkout" name="is skipped">
                   <skipped/>
                 </testcase>
               </testsuite>"#,
        )
        .expect("valid JUnit");

        assert_eq!(report.errors, 0);
        let statuses: Vec<(String, ImportStatus)> = report
            .cases
            .iter()
            .map(|case| (case.test_case_id.clone(), case.status))
            .collect();
        assert_eq!(
            statuses,
            vec![
                ("Checkout.pays".to_owned(), ImportStatus::Passed),
                ("Checkout.declines".to_owned(), ImportStatus::Failed),
                ("Checkout.times out".to_owned(), ImportStatus::Failed),
                ("Checkout.is skipped".to_owned(), ImportStatus::Blocked),
            ]
        );
    }

    #[test]
    fn records_the_failure_message_as_notes() {
        let report = parse(
            r#"<testsuite>
                 <testcase classname="Suite" name="breaks">
                   <failure message="expected 1 got 2"/>
                 </testcase>
                 <testcase classname="Suite" name="passes"/>
               </testsuite>"#,
        )
        .expect("valid JUnit");

        assert_eq!(report.cases[0].notes.as_deref(), Some("expected 1 got 2"));
        assert_eq!(report.cases[1].notes, None);
    }

    #[test]
    fn reads_the_enclosing_suite_timestamp() {
        let report = parse(
            r#"<testsuites>
                 <testsuite timestamp="2026-09-10T12:00:00Z">
                   <testcase classname="Suite" name="nested"/>
                 </testsuite>
               </testsuites>"#,
        )
        .expect("valid JUnit");

        assert_eq!(
            report.cases[0].timestamp.as_deref(),
            Some("2026-09-10T12:00:00Z")
        );
    }

    #[test]
    fn a_testcase_without_a_classname_is_named_by_its_name_alone() {
        let report =
            parse(r#"<testsuite><testcase name="bare"/></testsuite>"#).expect("valid JUnit");

        assert_eq!(report.cases[0].test_case_id, "bare");
    }

    #[test]
    fn a_testcase_that_cannot_be_named_is_an_error_not_a_case() {
        let report = parse(
            r#"<testsuite>
                 <testcase classname="Suite"/>
                 <testcase classname="Suite" name="named"/>
               </testsuite>"#,
        )
        .expect("valid JUnit");

        assert_eq!(report.errors, 1);
        assert_eq!(report.cases.len(), 1);
        assert_eq!(report.cases[0].test_case_id, "Suite.named");
    }

    #[test]
    fn finds_testcases_at_any_depth() {
        let report = parse(
            r#"<testsuites>
                 <testsuite>
                   <testsuite>
                     <testcase classname="Deep" name="inner"/>
                   </testsuite>
                 </testsuite>
               </testsuites>"#,
        )
        .expect("valid JUnit");

        assert_eq!(report.cases.len(), 1);
        assert_eq!(report.cases[0].test_case_id, "Deep.inner");
    }

    #[test]
    fn malformed_xml_is_an_invalid_request() {
        let error = parse("<testsuite><testcase></testsuite>").expect_err("malformed");
        assert!(matches!(
            error,
            DomainError::InvalidRequest {
                code: "invalid_request",
                ..
            }
        ));
    }
}
