//! Report import: the mapping from a JUnit XML report or a JSON results array
//! to run results.
//!
//! Both readers produce [`ParsedCase`]es, so the service stores them the same
//! way and a case the run already records is left alone either way. They differ
//! in how they treat input they cannot use. A JUnit document is a tree of
//! `<testsuite>` elements, each holding `<testcase>` elements; a testcase sits
//! at whatever depth its author chose, so this walks every descendant rather
//! than one fixed level, and a testcase it cannot name is counted rather than
//! rejected so a partly unusable report still imports the part that is not. A
//! JSON body is written by a caller of this API, so anything it gets wrong —
//! malformed JSON, an unknown field, a status that cannot be mapped — fails the
//! whole request and writes nothing.
//!
//! Both parsers are read-only and never touch storage.

use roxmltree::{Document, Node};
use serde::Deserialize;
use serde_json::Value;

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

/// One entry of a JSON import body, before it meets the run it lands in.
///
/// The fields are strict in both directions: an unknown or misspelled field, a
/// missing `testCaseId` or `status`, or a wrong JSON type is rejected rather
/// than ignored, matching the models that carry `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonEntry {
    test_case_id: String,
    status: String,
    notes: Option<String>,
    timestamp: Option<String>,
}

/// Reads a JSON results body: a bare array of entries, or an object whose only
/// field `results` holds that array.
///
/// Unlike [`parse`], every problem is fatal. A body that is not valid JSON, an
/// entry that is not an object, an unknown or misspelled field, a missing or
/// empty `testCaseId`, or a `status` the importer cannot map all answer a `400`
/// and nothing is written. `notes` and `timestamp` are optional, and `null`
/// means absent.
pub fn parse_json(body: &[u8]) -> Result<Vec<ParsedCase>, DomainError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| DomainError::invalid_request("Import body must be valid JSON"))?;

    let entries = match value {
        Value::Array(entries) => entries,
        Value::Object(mut object) if object.len() == 1 => match object.remove("results") {
            Some(Value::Array(entries)) => entries,
            _ => return Err(unusable_body()),
        },
        _ => return Err(unusable_body()),
    };

    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| map_json_entry(index, entry))
        .collect()
}

/// The error for a body that is neither a results array nor the wrapper object.
fn unusable_body() -> DomainError {
    DomainError::invalid_request(
        "Import body must be an array of results, or an object whose only field is `results`",
    )
}

/// Maps one JSON entry, naming its position when the entry is unusable.
fn map_json_entry(index: usize, entry: Value) -> Result<ParsedCase, DomainError> {
    let entry: JsonEntry = serde_json::from_value(entry).map_err(|error| {
        DomainError::invalid_request(format!("Import entry {index} is invalid: {error}"))
    })?;

    if entry.test_case_id.is_empty() {
        return Err(DomainError::invalid_request(format!(
            "Import entry {index} is missing testCaseId"
        )));
    }

    let status = match entry.status.as_str() {
        "Passed" => ImportStatus::Passed,
        "Failed" => ImportStatus::Failed,
        "Blocked" => ImportStatus::Blocked,
        _ => return Err(DomainError::invalid_import_status()),
    };

    Ok(ParsedCase {
        test_case_id: entry.test_case_id,
        status,
        notes: entry.notes,
        timestamp: entry.timestamp,
    })
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

    #[test]
    fn a_json_array_maps_its_fields_and_statuses() {
        let cases = parse_json(
            br#"[{"testCaseId":"TC-1","status":"Passed","notes":"ok"},
                 {"testCaseId":"TC-2","status":"Failed"},
                 {"testCaseId":"TC-3","status":"Blocked","timestamp":"1"}]"#,
        )
        .expect("valid JSON");

        assert_eq!(cases.len(), 3);
        assert_eq!(cases[0].test_case_id, "TC-1");
        assert_eq!(cases[0].status, ImportStatus::Passed);
        assert_eq!(cases[0].notes.as_deref(), Some("ok"));
        assert_eq!(cases[0].timestamp, None);
        assert_eq!(cases[1].status, ImportStatus::Failed);
        assert_eq!(cases[1].notes, None);
        assert_eq!(cases[2].status, ImportStatus::Blocked);
        assert_eq!(cases[2].timestamp.as_deref(), Some("1"));
    }

    #[test]
    fn a_json_body_may_be_wrapped_in_a_results_field() {
        let cases = parse_json(br#"{"results":[{"testCaseId":"TC-1","status":"Passed"}]}"#)
            .expect("valid wrapper");
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].test_case_id, "TC-1");

        // An object is the wrapper only when `results` is its single field and
        // holds an array; anything else is not a results body.
        let bodies: &[&[u8]] = &[
            br#"{"cases":[]}"#,
            br#"{"results":[],"extra":1}"#,
            br#"{"results":"nope"}"#,
            br#"{"results":{}}"#,
            b"\"results\"",
            b"null",
            b"42",
        ];
        for body in bodies {
            let error = parse_json(body).expect_err("unusable body");
            assert!(
                matches!(
                    error,
                    DomainError::InvalidRequest {
                        code: "invalid_request",
                        ..
                    }
                ),
                "rejected {body:?} as {error:?}"
            );
        }
    }

    #[test]
    fn an_empty_json_body_parses_to_no_cases() {
        assert!(parse_json(b"[]").expect("valid").is_empty());
        assert!(parse_json(br#"{"results":[]}"#).expect("valid").is_empty());
    }

    #[test]
    fn json_null_optional_fields_mean_absent() {
        let cases = parse_json(
            br#"[{"testCaseId":"TC-1","status":"Passed","notes":null,"timestamp":null}]"#,
        )
        .expect("valid");
        assert_eq!(cases[0].notes, None);
        assert_eq!(cases[0].timestamp, None);
    }

    #[test]
    fn a_json_entry_problem_names_its_position() {
        let bodies: &[&[u8]] = &[
            br#"[{"status":"Passed"}]"#,
            br#"[{"testCaseId":"TC-1"}]"#,
            br#"[{"testCaseId":"TC-1","status":"Passed","unknown":1}]"#,
            br#"[{"testCaseId":"TC-1","status":"Passed","notes":5}]"#,
            br#"[{"testCaseId":"","status":"Passed"}]"#,
            br#"["not an object"]"#,
        ];
        for body in bodies {
            let error = parse_json(body).expect_err("unusable entry");
            match error {
                DomainError::InvalidRequest {
                    code: "invalid_request",
                    message,
                } => assert!(
                    message.contains("entry 0"),
                    "names the entry, got {message}"
                ),
                other => panic!("unexpected error: {other:?}"),
            }
        }
    }

    #[test]
    fn a_json_status_outside_the_three_is_rejected_as_invalid_status() {
        for status in ["Untested", "Retest", "passed", "Unknown", ""] {
            let body = format!(r#"[{{"testCaseId":"TC-1","status":"{status}"}}]"#);
            let error = parse_json(body.as_bytes()).expect_err("unmappable status");
            assert!(
                matches!(
                    error,
                    DomainError::InvalidRequest {
                        code: "invalid_status",
                        ..
                    }
                ),
                "rejected {status:?} as {error:?}"
            );
        }
    }

    #[test]
    fn malformed_json_is_an_invalid_request() {
        for body in [&b"{not json"[..], b"", b"[{\"testCaseId\":]", b"[1,2,3"] {
            let error = parse_json(body).expect_err("malformed");
            assert!(matches!(
                error,
                DomainError::InvalidRequest {
                    code: "invalid_request",
                    ..
                }
            ));
        }
    }
}
