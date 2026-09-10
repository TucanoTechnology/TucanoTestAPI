//! Composition and execution rules.
//!
//! These are the rules that decide how a case joins a suite, how a suite or case
//! joins a run, and how a recorded result replaces an earlier one. They are pure
//! mutations of already-loaded documents so they can be reasoned about — and
//! tested — without a server or a filesystem.

use crate::models::{TestCase, TestCaseResult, TestConfiguration, TestRun, TestSuite};

use super::error::DomainError;

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

/// Records a result for a case, replacing any earlier result for that case.
pub fn upsert_result(run: &mut TestRun, result: TestCaseResult) {
    let results = run.results.get_or_insert_with(Vec::new);
    match results
        .iter_mut()
        .find(|existing| existing.test_case_id == result.test_case_id)
    {
        Some(existing) => *existing = result,
        None => results.push(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    fn result(case_id: &str, status: &str) -> TestCaseResult {
        TestCaseResult {
            test_case_id: case_id.to_owned(),
            status: status.to_owned(),
            timestamp: "1".to_owned(),
            notes: None,
            attachments: None,
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
    fn a_second_result_for_a_case_replaces_the_first() {
        let mut target = run();
        upsert_result(&mut target, result("TC-1", "Untested"));
        upsert_result(&mut target, result("TC-2", "Passed"));
        upsert_result(&mut target, result("TC-1", "Failed"));

        let results = target.results.expect("results");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].test_case_id, "TC-1");
        assert_eq!(results[0].status, "Failed");
        assert_eq!(results[1].test_case_id, "TC-2");
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
}
