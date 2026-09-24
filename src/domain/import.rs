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

/// The deepest element nesting a JUnit document may use.
///
/// A real report nests a suite inside the root and a testcase inside the suite —
/// well under ten levels. roxmltree bounds entity-reference depth but parses
/// element nesting by recursion, so a document nested thousands of levels deep
/// exhausts the worker stack before the parser returns and aborts the process.
const MAX_ELEMENT_DEPTH: usize = 100;

/// Whether the raw document nests elements deeper than [`MAX_ELEMENT_DEPTH`].
///
/// A cheap byte scan rather than a second XML parser: it walks the bytes once,
/// counting each start tag as one level deeper and each close tag as one level
/// shallower. What looks like markup but is not an element — a comment, a CDATA
/// section, a processing instruction, a quoted attribute value — is skipped
/// whole, so a stack trace wrapped in CDATA cannot inflate the count and a
/// comment cannot fake a close tag to hold the count down. Over-counting what
/// remains (a declaration, a stray `<` in text) only makes the bound stricter;
/// under-counting a real element is what must not happen, and cannot: in a
/// well-formed document every `<name …>` is one tag this scan counts once, and
/// every `</name>` is one it discounts once.
fn nests_too_deeply(xml: &str) -> bool {
    let bytes = xml.as_bytes();
    let mut depth: usize = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }

        if bytes[index..].starts_with(b"<!--") {
            index = skip_past(bytes, index + 4, b"-->");
        } else if bytes[index..].starts_with(b"<![CDATA[") {
            index = skip_past(bytes, index + 9, b"]]>");
        } else if bytes.get(index + 1) == Some(&b'?') {
            index = skip_past(bytes, index + 2, b"?>");
        } else if bytes.get(index + 1) == Some(&b'/') {
            // A close tag ends the element it names and holds nothing.
            depth = depth.saturating_sub(1);
            index = skip_past(bytes, index + 1, b">");
        } else {
            // A start tag or declaration, unless it closes itself with `/>`.
            let (self_closing, next) = scan_tag(bytes, index + 1);
            if !self_closing {
                depth = depth.saturating_add(1);
                if depth > MAX_ELEMENT_DEPTH {
                    return true;
                }
            }
            index = next;
        }
    }

    false
}

/// The index just past `delimiter`, or the end of the document when the
/// construct is never closed.
fn skip_past(bytes: &[u8], from: usize, delimiter: &[u8]) -> usize {
    let mut cursor = from;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(delimiter) {
            return cursor + delimiter.len();
        }
        cursor += 1;
    }
    bytes.len()
}

/// Walks the tag that starts just after `<` to the `>` that ends it, skipping
/// quoted attribute values, and returns whether it closes itself with `/>`
/// along with the index just past it.
fn scan_tag(bytes: &[u8], from: usize) -> (bool, usize) {
    let mut cursor = from;
    let mut previous: Option<u8> = None;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'>' => return (previous == Some(b'/'), cursor + 1),
            quote @ (b'"' | b'\'') => {
                previous = Some(quote);
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                cursor += 1;
            }
            byte => {
                previous = Some(byte);
                cursor += 1;
            }
        }
    }

    // An unclosed tag is counted rather than ignored, so a truncated document
    // cannot hide depth from the bound.
    (false, bytes.len())
}

/// Reads a JUnit report, mapping every testcase it can name.
///
/// Malformed XML is an `invalid_request`; a testcase without a `name` is not an
/// error, it is counted in [`ParsedReport::errors`]. A document nested deeper
/// than `MAX_ELEMENT_DEPTH` is refused as an `invalid_request` before the
/// parser recurses into it.
pub fn parse(xml: &str) -> Result<ParsedReport, DomainError> {
    if nests_too_deeply(xml) {
        return Err(DomainError::invalid_request(
            "JUnit XML nests elements too deeply",
        ));
    }

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
    fn an_over_deep_document_is_refused_before_the_parser_sees_it() {
        // The audit's reproduction: deep nesting, not a large body. Before the
        // bound existed this overflowed the worker stack and aborted.
        let document = format!("{}{}", "<a>".repeat(5_000), "</a>".repeat(5_000));

        match parse(&document).expect_err("over-deep document") {
            DomainError::InvalidRequest {
                code: "invalid_request",
                message,
            } => assert_eq!(message, "JUnit XML nests elements too deeply"),
            other => panic!("unexpected error: {other:?}"),
        }

        // An unclosed document aborts the parser the same way, so it is refused
        // as well.
        assert!(nests_too_deeply(&"<a>".repeat(5_000)));
    }

    #[test]
    fn the_depth_bound_holds_exactly_at_the_limit() {
        let at_limit = format!(
            "{}{}",
            "<a>".repeat(MAX_ELEMENT_DEPTH),
            "</a>".repeat(MAX_ELEMENT_DEPTH)
        );
        let over_limit = format!(
            "{}{}",
            "<a>".repeat(MAX_ELEMENT_DEPTH + 1),
            "</a>".repeat(MAX_ELEMENT_DEPTH + 1)
        );

        assert!(!nests_too_deeply(&at_limit));
        assert!(nests_too_deeply(&over_limit));

        // A document at the bound is still handed to the parser and read; one
        // past it is refused before the parser recurses.
        assert!(parse(&at_limit).is_ok());
        assert!(parse(&over_limit).is_err());
    }

    #[test]
    fn self_closing_siblings_do_not_consume_the_depth_bound() {
        // Two hundred sibling testcases nest one level, not two hundred, so a
        // report cannot be refused for width masquerading as depth.
        let mut document = String::from("<testsuite>");
        for _ in 0..200 {
            document.push_str(r#"<testcase name="t"/>"#);
        }
        document.push_str("</testsuite>");

        assert_eq!(parse(&document).expect("valid JUnit").cases.len(), 200);
    }

    #[test]
    fn cdata_bodies_do_not_accumulate_towards_the_depth_bound() {
        // A real report wraps a stack trace in CDATA. Skipped whole, many such
        // testcases stay at one level; miscounted, they would add up past the
        // bound and refuse a legitimate report.
        let mut document = String::from("<testsuite>");
        for index in 0..200 {
            document.push_str(&format!(
                r#"<testcase name="t{index}"><failure><![CDATA[at <boom> (x:1)]]></failure></testcase>"#
            ));
        }
        document.push_str("</testsuite>");

        assert_eq!(parse(&document).expect("valid JUnit").cases.len(), 200);
    }

    #[test]
    fn markup_that_only_looks_like_a_close_tag_does_not_discount_a_real_element() {
        // Neither a comment's or attribute value's `</a>` may lower the count:
        // miscounted, a document could nest thousands of real elements while
        // the count stayed low, and the abort would remain reachable.
        let in_a_comment = "<a><!--</a></a></a>-->".repeat(200);
        let in_an_attribute = r#"<a b="</a> </a>">"#.repeat(200);

        assert!(nests_too_deeply(&in_a_comment));
        assert!(nests_too_deeply(&in_an_attribute));
    }

    #[test]
    fn a_reasonably_deep_report_still_imports() {
        let mut document = String::from("<testsuites>");
        for _ in 0..8 {
            document.push_str("<testsuite>");
        }
        document.push_str(r#"<testcase classname="Deep" name="inner"/>"#);
        for _ in 0..8 {
            document.push_str("</testsuite>");
        }
        document.push_str("</testsuites>");

        let report = parse(&document).expect("valid JUnit");
        assert_eq!(report.cases.len(), 1);
        assert_eq!(report.cases[0].test_case_id, "Deep.inner");
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
