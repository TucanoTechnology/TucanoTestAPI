//! Composition and execution rules.
//!
//! These are the rules that decide how a case joins a suite, how a suite or case
//! joins a run, and how a recorded result replaces an earlier one. They are pure
//! mutations of already-loaded documents so they can be reasoned about — and
//! tested — without a server or a filesystem.

use std::collections::HashMap;

use crate::models::{DefectLink, TestCase, TestCaseResult, TestConfiguration, TestRun, TestSuite};

use super::error::DomainError;

/// The version a run pins for a case that carries none.
///
/// A case written before the API versioned cases has no `version` of its own,
/// and the run still has to say which revision it snapshotted.
const UNVERSIONED_CASE: u64 = 1;

/// Adds a case to a suite, refusing a case that is already present.
///
/// The duplicate check compares both the case's own identifier and the
/// identifier the caller addressed it by, matching the legacy behaviour.
pub fn attach_case_to_suite(
    suite: &mut TestSuite,
    test_case: &TestCase,
    target_case_id: &str,
) -> Result<(), DomainError> {
    let duplicate = suite.test_cases.iter().any(|existing| {
        existing.test_case_id == test_case.test_case_id || existing.test_case_id == target_case_id
    });
    if duplicate {
        return Err(DomainError::Conflict(
            "Test case is already in suite".to_owned(),
        ));
    }
    suite.test_cases.push(test_case.clone());
    Ok(())
}

/// Removes a case from a suite; a case that is not present is a 404.
pub fn detach_case_from_suite(suite: &mut TestSuite, case_id: &str) -> Result<(), DomainError> {
    let before = suite.test_cases.len();
    suite
        .test_cases
        .retain(|existing| existing.test_case_id != case_id);
    if suite.test_cases.len() == before {
        return Err(DomainError::NotFound("Test case not in suite".to_owned()));
    }
    Ok(())
}

/// Adds a suite to a run, refusing a suite that is already present.
pub fn attach_suite_to_run(
    run: &mut TestRun,
    suite: &TestSuite,
    target_suite_id: &str,
) -> Result<(), DomainError> {
    let suites = run.test_suites.get_or_insert_with(Vec::new);
    let duplicate = suites.iter().any(|existing| {
        existing.suite_id == suite.suite_id || existing.suite_id == target_suite_id
    });
    if duplicate {
        return Err(DomainError::Conflict(
            "Test suite is already in test run".to_owned(),
        ));
    }
    suites.push(suite.clone());
    Ok(())
}

/// Adds a case to a run, refusing a case that is already present.
pub fn attach_case_to_run(
    run: &mut TestRun,
    test_case: &TestCase,
    target_case_id: &str,
) -> Result<(), DomainError> {
    let cases = run.test_cases.get_or_insert_with(Vec::new);
    let duplicate = cases.iter().any(|existing| {
        existing.test_case_id == test_case.test_case_id || existing.test_case_id == target_case_id
    });
    if duplicate {
        return Err(DomainError::Conflict(
            "Test case is already in test run".to_owned(),
        ));
    }
    cases.push(test_case.clone());
    Ok(())
}

/// Pins the version a run records for a case, keeping the first one it saw.
///
/// The first capture wins, so re-recording a result — or a later qualifying edit
/// to the live case — never revises what the run already pinned: the run is a
/// snapshot, and its `caseVersions` has to stay one too. A case with no version
/// of its own is pinned as version 1, matching how the API reads such a case.
pub fn capture_case_version(run: &mut TestRun, case_id: &str, version: Option<u64>) {
    run.case_versions
        .get_or_insert_with(HashMap::new)
        .entry(case_id.to_owned())
        .or_insert(version.unwrap_or(UNVERSIONED_CASE));
}

/// Links a top-level configuration to a run, refusing one that is already
/// referenced.
///
/// A configuration is a real resource with a home of its own, so the run stores
/// a reference rather than a copy: the reference keeps the configuration's
/// identifier and name, and drops the descriptive fields the resource owns.
pub fn attach_configuration_to_run(
    run: &mut TestRun,
    configuration: &TestConfiguration,
    target_config_id: &str,
) -> Result<(), DomainError> {
    let configurations = run.configurations.get_or_insert_with(Vec::new);
    let duplicate = configurations.iter().any(|existing| {
        existing.config_id == configuration.config_id || existing.config_id == target_config_id
    });
    if duplicate {
        return Err(DomainError::Conflict(
            "Test configuration is already linked to test run".to_owned(),
        ));
    }
    configurations.push(TestConfiguration {
        config_id: target_config_id.to_owned(),
        name: configuration.name.clone(),
        browser: None,
        os: None,
        device: None,
        resolution: None,
    });
    Ok(())
}

/// Removes a configuration reference from a run; one that is not linked is a
/// 404.
pub fn detach_configuration_from_run(
    run: &mut TestRun,
    config_id: &str,
) -> Result<(), DomainError> {
    let configurations = run.configurations.get_or_insert_with(Vec::new);
    let before = configurations.len();
    configurations.retain(|existing| existing.config_id != config_id);
    if configurations.len() == before {
        return Err(DomainError::NotFound(
            "Test configuration not linked to test run".to_owned(),
        ));
    }
    Ok(())
}

/// What a recording request says about one optional field of a result.
///
/// A request describes a result in the same shape every time, so a field it
/// leaves out says nothing about that field: an omitted `notes` keeps whatever
/// the stored result already carried, while an explicit JSON `null` clears it.
/// `Keep` and `Clear` are therefore different answers even though both store no
/// value in the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Patch<T> {
    Keep,
    Clear,
    Set(T),
}

impl<T> Patch<T> {
    /// The value to store for a result that does not exist yet.
    ///
    /// A patch that says `Keep` has nothing to keep when there is no result, so
    /// a brand new result stores nothing for that field.
    fn into_stored(self) -> Option<T> {
        match self {
            Patch::Set(value) => Some(value),
            Patch::Keep | Patch::Clear => None,
        }
    }
}

/// The result a run should hold for one case after a recording request.
///
/// A run's stored result carries more than a request can describe: defect links
/// and attachments are added by their own routes once the result exists, and no
/// recording request can speak for them. The update therefore names only the
/// fields the request decides, so re-recording keeps what it cannot see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultUpdate {
    pub test_case_id: String,
    pub status: String,
    pub timestamp: String,
    pub notes: Patch<String>,
    pub duration_ms: Patch<u64>,
}

/// Records a result for a case, updating any earlier result for that case.
///
/// The status and timestamp the request carries are the run's current word on
/// the case and always replace what was stored; `notes` and `durationMs` follow
/// the patch the request built, so an omitted field survives a re-record and an
/// explicit `null` clears it. A result's defect links and attachments are left
/// exactly as they were: the request cannot describe either, and dropping them
/// would lose the failures already linked to the case.
pub fn upsert_result(run: &mut TestRun, update: ResultUpdate) {
    let results = run.results.get_or_insert_with(Vec::new);
    match results
        .iter_mut()
        .find(|existing| existing.test_case_id == update.test_case_id)
    {
        Some(existing) => {
            existing.status = update.status;
            existing.timestamp = update.timestamp;
            apply_patch(&mut existing.notes, update.notes);
            apply_patch(&mut existing.duration_ms, update.duration_ms);
        }
        None => results.push(TestCaseResult {
            test_case_id: update.test_case_id,
            status: update.status,
            timestamp: update.timestamp,
            notes: update.notes.into_stored(),
            duration_ms: update.duration_ms.into_stored(),
            attachments: None,
            defect_links: None,
        }),
    }
}

fn apply_patch<T>(stored: &mut Option<T>, patch: Patch<T>) {
    match patch {
        Patch::Keep => {}
        Patch::Clear => *stored = None,
        Patch::Set(value) => *stored = Some(value),
    }
}

/// Whether a run holds `case_id`, so it may record a result for it.
///
/// A run holds a case when it declares it — under `testCases`, or through one of
/// the suites it embeds — or when it already records a result for it. The second
/// clause is what keeps a run readable: a document written before the run
/// declared a population, and a run an imported report filled with cases it
/// never listed, both stay editable rather than becoming write-once.
pub fn holds_case(run: &TestRun, case_id: &str) -> bool {
    let declared = run
        .test_cases
        .as_ref()
        .is_some_and(|cases| cases.iter().any(|case| case.test_case_id == case_id));
    let through_a_suite = run.test_suites.as_ref().is_some_and(|suites| {
        suites.iter().any(|suite| {
            suite
                .test_cases
                .iter()
                .any(|case| case.test_case_id == case_id)
        })
    });
    let already_recorded = run
        .results
        .as_ref()
        .is_some_and(|results| results.iter().any(|result| result.test_case_id == case_id));

    declared || through_a_suite || already_recorded
}

/// Adds a defect link to the result a run records for `case_id`, refusing a
/// defect that is already linked.
///
/// The duplicate check compares the defect the link names rather than the link's
/// own identifier: linking the same defect twice would record the same failure
/// twice, and the API derives the identifiers anyway.
pub fn attach_defect_to_result(
    run: &mut TestRun,
    case_id: &str,
    link: DefectLink,
) -> Result<(), DomainError> {
    let result = result_mut(run, case_id)?;
    let links = result.defect_links.get_or_insert_with(Vec::new);
    if links
        .iter()
        .any(|existing| existing.defect_id == link.defect_id)
    {
        return Err(DomainError::Conflict(
            "Defect is already linked to test result".to_owned(),
        ));
    }
    links.push(link);
    Ok(())
}

/// Removes a defect link from the result a run records for `case_id`; a link
/// the result does not carry is a 404.
pub fn detach_defect_from_result(
    run: &mut TestRun,
    case_id: &str,
    link_id: &str,
) -> Result<(), DomainError> {
    let result = result_mut(run, case_id)?;
    let links = result.defect_links.get_or_insert_with(Vec::new);
    let before = links.len();
    links.retain(|existing| existing.link_id != link_id);
    if links.len() == before {
        return Err(DomainError::NotFound(
            "Defect link not found in test result".to_owned(),
        ));
    }
    Ok(())
}

/// Finds the result a run records for a case, as a 404 when there is none.
///
/// A run with no results and a run whose results name other cases are the same
/// answer to the client — there is nothing here to link a defect to — so both
/// are reported the same way.
fn result_mut<'a>(
    run: &'a mut TestRun,
    case_id: &str,
) -> Result<&'a mut TestCaseResult, DomainError> {
    run.results
        .as_mut()
        .and_then(|results| {
            results
                .iter_mut()
                .find(|result| result.test_case_id == case_id)
        })
        .ok_or_else(|| DomainError::NotFound("Test result not found in test run".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Attachment;
    use serde_json::json;

    fn case(id: &str) -> TestCase {
        TestCase {
            test_case_id: id.to_owned(),
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
            version: None,
            last_modified: None,
        }
    }

    fn suite(id: &str, cases: Vec<TestCase>) -> TestSuite {
        TestSuite {
            suite_id: id.to_owned(),
            name: "suite".to_owned(),
            description: None,
            test_cases: cases,
            tags: None,
        }
    }

    fn configuration(id: &str, name: &str) -> TestConfiguration {
        TestConfiguration {
            config_id: id.to_owned(),
            name: name.to_owned(),
            browser: Some("chrome".to_owned()),
            os: Some("linux".to_owned()),
            device: None,
            resolution: None,
        }
    }

    fn run() -> TestRun {
        TestRun {
            test_run_id: "R-1".to_owned(),
            timestamp: "1".to_owned(),
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

    fn update(case_id: &str, status: &str) -> ResultUpdate {
        ResultUpdate {
            test_case_id: case_id.to_owned(),
            status: status.to_owned(),
            timestamp: "1".to_owned(),
            notes: Patch::Keep,
            duration_ms: Patch::Keep,
        }
    }

    #[test]
    fn a_case_joins_a_suite_once() {
        let mut target = suite("S-1", Vec::new());
        attach_case_to_suite(&mut target, &case("TC-1"), "TC-1").expect("first add");

        assert_eq!(target.test_cases.len(), 1);
        let error = attach_case_to_suite(&mut target, &case("TC-1"), "TC-1")
            .expect_err("second add must conflict");
        assert!(matches!(error, DomainError::Conflict(_)));
        assert_eq!(target.test_cases.len(), 1);
    }

    #[test]
    fn a_case_is_detected_as_present_under_either_identifier() {
        let mut target = suite("S-1", vec![case("TC-internal")]);
        assert!(attach_case_to_suite(&mut target, &case("TC-internal"), "TC-internal").is_err());

        let mut by_address = suite("S-1", vec![case("TC-internal")]);
        assert!(attach_case_to_suite(&mut by_address, &case("TC-other"), "TC-internal").is_err());
    }

    #[test]
    fn removing_an_absent_case_is_not_found() {
        let mut target = suite("S-1", Vec::new());
        let error = detach_case_from_suite(&mut target, "TC-1").expect_err("absent");
        assert!(matches!(error, DomainError::NotFound(_)));

        let mut present = suite("S-1", vec![case("TC-1"), case("TC-2")]);
        detach_case_from_suite(&mut present, "TC-1").expect("present");
        assert_eq!(present.test_cases.len(), 1);
        assert_eq!(present.test_cases[0].test_case_id, "TC-2");
    }

    #[test]
    fn suites_and_cases_join_a_run_exactly_once() {
        let mut target = run();
        attach_suite_to_run(&mut target, &suite("S-1", Vec::new()), "S-1").expect("suite");
        assert!(attach_suite_to_run(&mut target, &suite("S-1", Vec::new()), "S-1").is_err());

        attach_case_to_run(&mut target, &case("TC-1"), "TC-1").expect("case");
        assert!(attach_case_to_run(&mut target, &case("TC-1"), "TC-1").is_err());

        assert_eq!(target.test_suites.map(|suites| suites.len()), Some(1));
        assert_eq!(target.test_cases.map(|cases| cases.len()), Some(1));
    }

    #[test]
    fn a_run_pins_the_first_version_it_captures_for_a_case() {
        let mut target = run();
        capture_case_version(&mut target, "TC-1", Some(3));
        capture_case_version(&mut target, "TC-1", Some(9));

        assert_eq!(
            target
                .case_versions
                .as_ref()
                .and_then(|pinned| pinned.get("TC-1")),
            Some(&3),
            "a second capture must not revise the pinned version"
        );
    }

    #[test]
    fn a_case_without_a_version_is_pinned_as_the_first_revision() {
        let mut target = run();
        capture_case_version(&mut target, "TC-1", None);
        assert_eq!(
            target
                .case_versions
                .as_ref()
                .and_then(|pinned| pinned.get("TC-1")),
            Some(&1)
        );
    }

    #[test]
    fn each_case_is_pinned_under_its_own_identifier() {
        let mut target = run();
        capture_case_version(&mut target, "TC-1", Some(2));
        capture_case_version(&mut target, "TC-2", None);

        let pinned = target.case_versions.expect("pinned versions");
        assert_eq!(pinned.get("TC-1"), Some(&2));
        assert_eq!(pinned.get("TC-2"), Some(&1));
    }

    #[test]
    fn a_second_result_for_a_case_merges_into_the_first() {
        let mut target = run();
        upsert_result(&mut target, update("TC-1", "Untested"));
        upsert_result(&mut target, update("TC-2", "Passed"));
        let mut second = update("TC-1", "Failed");
        second.timestamp = "2".to_owned();
        upsert_result(&mut target, second);

        let results = target.results.expect("results");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].test_case_id, "TC-1");
        assert_eq!(results[0].status, "Failed");
        assert_eq!(results[0].timestamp, "2");
        assert_eq!(results[1].test_case_id, "TC-2");
    }

    #[test]
    fn a_re_recorded_result_keeps_the_fields_the_request_leaves_out() {
        let mut target = run();
        let mut first = update("TC-1", "Failed");
        first.notes = Patch::Set("flaky on CI".to_owned());
        first.duration_ms = Patch::Set(1200);
        upsert_result(&mut target, first);

        upsert_result(&mut target, update("TC-1", "Passed"));

        let result = &target.results.as_ref().expect("results")[0];
        assert_eq!(result.status, "Passed");
        assert_eq!(result.notes.as_deref(), Some("flaky on CI"));
        assert_eq!(result.duration_ms, Some(1200));
    }

    #[test]
    fn an_explicit_null_clears_a_stored_field() {
        let mut target = run();
        let mut first = update("TC-1", "Failed");
        first.notes = Patch::Set("flaky on CI".to_owned());
        first.duration_ms = Patch::Set(1200);
        upsert_result(&mut target, first);

        let mut clearing = update("TC-1", "Passed");
        clearing.notes = Patch::Clear;
        clearing.duration_ms = Patch::Clear;
        upsert_result(&mut target, clearing);

        let result = &target.results.as_ref().expect("results")[0];
        assert_eq!(result.notes, None);
        assert_eq!(result.duration_ms, None);
    }

    #[test]
    fn a_new_result_stores_what_the_request_describes() {
        let mut target = run();
        let mut described = update("TC-1", "Failed");
        described.notes = Patch::Set("boom".to_owned());
        described.duration_ms = Patch::Set(7);
        upsert_result(&mut target, described);

        let result = &target.results.as_ref().expect("results")[0];
        assert_eq!(result.test_case_id, "TC-1");
        assert_eq!(result.status, "Failed");
        assert_eq!(result.notes.as_deref(), Some("boom"));
        assert_eq!(result.duration_ms, Some(7));
        assert_eq!(result.attachments, None);
        assert_eq!(result.defect_links, None);
    }

    #[test]
    fn a_new_result_stores_nothing_for_a_field_the_request_leaves_out() {
        let mut target = run();
        let mut described = update("TC-1", "Failed");
        described.notes = Patch::Clear;
        upsert_result(&mut target, described);

        let result = &target.results.as_ref().expect("results")[0];
        assert_eq!(result.notes, None);
        assert_eq!(result.duration_ms, None);
    }

    #[test]
    fn re_recording_keeps_the_defect_links_and_attachments_it_cannot_describe() {
        let mut target = run();
        upsert_result(&mut target, update("TC-1", "Failed"));
        attach_defect_to_result(&mut target, "TC-1", link("L-1", "BUG-42")).expect("link");
        target.results.as_mut().expect("results")[0].attachments = Some(vec![Attachment {
            filename: "failure.log".to_owned(),
            original_name: "failure.log".to_owned(),
            mime_type: "text/plain".to_owned(),
            size: 12.0,
            uploaded_at: None,
        }]);

        upsert_result(&mut target, update("TC-1", "Passed"));

        let result = &target.results.as_ref().expect("results")[0];
        assert_eq!(result.status, "Passed");
        assert_eq!(
            result
                .defect_links
                .as_ref()
                .expect("links")
                .iter()
                .map(|link| link.defect_id.as_str())
                .collect::<Vec<_>>(),
            ["BUG-42"]
        );
        assert_eq!(
            result
                .attachments
                .as_ref()
                .expect("attachments")
                .iter()
                .map(|attachment| attachment.filename.as_str())
                .collect::<Vec<_>>(),
            ["failure.log"]
        );
    }

    #[test]
    fn a_run_holds_a_case_it_declares_directly() {
        let mut target = run();
        assert!(!holds_case(&target, "TC-1"));

        target.test_cases = Some(vec![case("TC-1")]);
        assert!(holds_case(&target, "TC-1"));
        assert!(!holds_case(&target, "TC-2"));
    }

    #[test]
    fn a_run_holds_a_case_one_of_its_suites_declares() {
        let mut target = run();
        target.test_suites = Some(vec![suite("S-1", vec![case("TC-1")])]);

        assert!(holds_case(&target, "TC-1"));
        assert!(!holds_case(&target, "TC-2"));
    }

    #[test]
    fn a_run_holds_a_case_it_already_records_a_result_for() {
        let mut target = run();
        upsert_result(&mut target, update("TC-1", "Passed"));

        assert!(holds_case(&target, "TC-1"));
        assert!(!holds_case(&target, "TC-2"));
    }

    #[test]
    fn composition_leaves_the_source_document_untouched() {
        let source = case("TC-1");
        let snapshot = serde_json::to_value(&source).expect("serialisable");
        let mut target = suite("S-1", Vec::new());
        attach_case_to_suite(&mut target, &source, "TC-1").expect("add");
        target.test_cases[0].title = "changed".to_owned();

        assert_eq!(
            serde_json::to_value(&source).expect("serialisable"),
            snapshot,
            "the added case must be a copy"
        );
        assert_eq!(json!(source.test_case_id), json!("TC-1"));
    }

    #[test]
    fn a_configuration_joins_a_run_as_a_reference_exactly_once() {
        let mut target = run();
        let source = configuration("chrome-linux.json", "chrome-linux");
        attach_configuration_to_run(&mut target, &source, "chrome-linux.json").expect("link");

        let linked = target.configurations.as_ref().expect("linked");
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].config_id, "chrome-linux.json");
        assert_eq!(linked[0].name, "chrome-linux");
        assert_eq!(
            linked[0].browser, None,
            "a reference embeds no full document"
        );

        let error = attach_configuration_to_run(&mut target, &source, "chrome-linux.json")
            .expect_err("second link must conflict");
        assert!(matches!(error, DomainError::Conflict(_)));
        assert_eq!(
            target
                .configurations
                .map(|configurations| configurations.len()),
            Some(1)
        );
    }

    #[test]
    fn removing_an_absent_configuration_is_not_found() {
        let mut target = run();
        let error = detach_configuration_from_run(&mut target, "chrome-linux.json")
            .expect_err("absent link");
        assert!(matches!(error, DomainError::NotFound(_)));

        attach_configuration_to_run(
            &mut target,
            &configuration("chrome-linux.json", "chrome-linux"),
            "chrome-linux.json",
        )
        .expect("link");
        detach_configuration_from_run(&mut target, "chrome-linux.json").expect("unlink");
        assert_eq!(
            target
                .configurations
                .map(|configurations| configurations.len()),
            Some(0)
        );
    }

    fn link(link_id: &str, defect_id: &str) -> DefectLink {
        DefectLink {
            link_id: link_id.to_owned(),
            defect_id: defect_id.to_owned(),
            defect_url: format!("https://tracker.example/{defect_id}"),
            tracker_type: "custom".to_owned(),
            title: None,
            status: None,
            linked_at: "1".to_owned(),
        }
    }

    #[test]
    fn a_defect_joins_a_result_exactly_once() {
        let mut target = run();
        upsert_result(&mut target, update("TC-1", "Failed"));
        attach_defect_to_result(&mut target, "TC-1", link("L-1", "BUG-42")).expect("link");

        let links = target.results.as_ref().expect("results")[0]
            .defect_links
            .as_ref()
            .expect("links");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].defect_id, "BUG-42");

        let error = attach_defect_to_result(&mut target, "TC-1", link("L-2", "BUG-42"))
            .expect_err("the same defect must conflict");
        assert!(matches!(error, DomainError::Conflict(_)));
        assert_eq!(
            target.results.as_ref().expect("results")[0]
                .defect_links
                .as_ref()
                .map(|links| links.len()),
            Some(1)
        );
    }

    #[test]
    fn linking_a_defect_needs_the_result_it_belongs_to() {
        let mut empty = run();
        let error = attach_defect_to_result(&mut empty, "TC-1", link("L-1", "BUG-42"))
            .expect_err("a run with no results has nothing to link to");
        assert!(matches!(error, DomainError::NotFound(_)));

        let mut other = run();
        upsert_result(&mut other, update("TC-2", "Passed"));
        let error = attach_defect_to_result(&mut other, "TC-1", link("L-1", "BUG-42"))
            .expect_err("the case must be the one the run recorded");
        assert!(matches!(error, DomainError::NotFound(_)));
    }

    #[test]
    fn removing_an_absent_defect_link_is_not_found() {
        let mut target = run();
        let error = detach_defect_from_result(&mut target, "TC-1", "L-1")
            .expect_err("a run with no results has nothing to unlink");
        assert!(matches!(error, DomainError::NotFound(_)));

        upsert_result(&mut target, update("TC-1", "Failed"));
        attach_defect_to_result(&mut target, "TC-1", link("L-1", "BUG-42")).expect("link");
        let error = detach_defect_from_result(&mut target, "TC-1", "L-2")
            .expect_err("the link must be the one the result carries");
        assert!(matches!(error, DomainError::NotFound(_)));

        detach_defect_from_result(&mut target, "TC-1", "L-1").expect("unlink");
        let links = target.results.as_ref().expect("results")[0]
            .defect_links
            .as_ref()
            .expect("links");
        assert!(links.is_empty(), "{links:?}");
    }

    #[test]
    fn unlinking_one_defect_leaves_the_others_in_place() {
        let mut target = run();
        upsert_result(&mut target, update("TC-1", "Failed"));
        attach_defect_to_result(&mut target, "TC-1", link("L-1", "BUG-42")).expect("first");
        attach_defect_to_result(&mut target, "TC-1", link("L-2", "BUG-43")).expect("second");

        detach_defect_from_result(&mut target, "TC-1", "L-1").expect("unlink");

        let links = target.results.as_ref().expect("results")[0]
            .defect_links
            .as_ref()
            .expect("links");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].defect_id, "BUG-43");
    }
}
