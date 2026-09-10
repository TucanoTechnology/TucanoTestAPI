//! The application, expressed against the storage boundary.
//!
//! [`TestService`] is the only thing the HTTP layer talks to. It owns no state
//! beyond the repository it was built with, so replicas sharing the same storage
//! behave identically, and every rule it applies lives in a sibling module that
//! can be unit tested on its own.

use std::io;

use serde_json::Value;

use crate::models::{Milestone, MilestoneProgress, TestCase, TestCaseResult, TestRun, TestSuite};
use crate::storage::{Repository, Resource, unique_suffix};

use super::duplicate::{self, DuplicateSpec};
use super::error::{self, DomainError};
use super::{
    Created, ListQuery, MAX_ATTACHMENT_BYTES, StoredAttachment, composition,
    current_timestamp_string, progress, required_string, resources, validation,
};

/// Result statuses a test run accepts.
const VALID_STATUSES: [&str; 5] = ["Passed", "Failed", "Blocked", "Untested", "Retest"];

/// CRUD, composition, duplication and reporting over a [`Repository`].
pub struct TestService<R> {
    repository: R,
}

impl<R: Repository> TestService<R> {
    /// Wraps a repository in the domain rules.
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    // --- generic CRUD --------------------------------------------------

    /// Lists a resource, applying the optional substring and tag filters.
    pub fn list(&self, resource: Resource, query: &ListQuery) -> Result<Vec<String>, DomainError> {
        let mut items = self.repository.list(resource)?;

        if let Some(filter) = query.filter.as_ref() {
            let needle = filter.to_lowercase();
            items.retain(|item| item.to_lowercase().contains(&needle));
        }

        if let Some(tags_param) = query.tags.as_ref() {
            let requested: Vec<String> = tags_param
                .split(',')
                .map(|tag| tag.trim().to_lowercase())
                .collect();
            items.retain(|item| {
                let Ok(value) = self.repository.read(resource, item) else {
                    return false;
                };
                let Some(tags) = value.get("tags").and_then(Value::as_array) else {
                    return false;
                };
                let item_tags: Vec<String> = tags
                    .iter()
                    .filter_map(|tag| tag.as_str().map(str::to_lowercase))
                    .collect();
                requested.iter().any(|tag| item_tags.contains(tag))
            });
        }

        Ok(items)
    }

    /// Reads a single document.
    pub fn get(&self, resource: Resource, id: &str) -> Result<Value, DomainError> {
        self.repository
            .read(resource, id)
            .map_err(error::read_error)
    }

    /// Validates, names and stores a new document.
    pub fn create(&self, resource: Resource, value: &Value) -> Result<Created, DomainError> {
        validation::validate_payload(resource, value)?;
        let id = resources::derive_create_id(resource, value)?;
        if self.repository.exists(resource, &id)? {
            return Err(DomainError::Conflict("Resource already exists".to_owned()));
        }
        self.repository.write(resource, &id, value)?;
        Ok(Created { id })
    }

    /// Validates and replaces an existing document.
    pub fn update(&self, resource: Resource, id: &str, value: &Value) -> Result<(), DomainError> {
        validation::validate_payload(resource, value)?;
        match self.repository.exists(resource, id) {
            Ok(true) => {}
            Ok(false) => return Err(DomainError::NotFound("Resource not found".to_owned())),
            Err(error) => return Err(error::delete_error(error)),
        }
        self.repository.write(resource, id, value)?;
        Ok(())
    }

    /// Removes a document.
    pub fn delete(&self, resource: Resource, id: &str) -> Result<(), DomainError> {
        self.repository
            .delete(resource, id)
            .map_err(error::delete_error)
    }

    // --- duplication ---------------------------------------------------

    /// Copies a document, applying the request-body overrides, and returns the
    /// identifier of the copy.
    pub fn duplicate(
        &self,
        spec: &DuplicateSpec,
        id: &str,
        body: &Value,
    ) -> Result<String, DomainError> {
        let mut document = self
            .repository
            .read(spec.resource, id)
            .map_err(|error| error::load_error(error, spec.not_found_message))?;
        let new_id = duplicate::apply_overrides(spec, id, body, &mut document);

        match self.repository.write(spec.resource, &new_id, &document) {
            Ok(()) => Ok(new_id),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(
                DomainError::Conflict(spec.already_exists_message.to_owned()),
            ),
            Err(error) => Err(error.into()),
        }
    }

    // --- composition ---------------------------------------------------

    /// Adds the case named in `body` to a suite.
    pub fn add_case_to_suite(&self, suite_id: &str, body: &Value) -> Result<(), DomainError> {
        let test_case_id = required_string(body, "testCaseId")
            .ok_or_else(|| DomainError::invalid_request("Required field testCaseId is missing"))?;

        let mut suite = self.load_suite(suite_id)?;
        let test_case = self.load_case(&test_case_id)?;
        composition::attach_case_to_suite(&mut suite, &test_case, &test_case_id)?;
        self.save_suite(suite_id, &suite)
    }

    /// Removes a case from a suite.
    pub fn remove_case_from_suite(&self, suite_id: &str, case_id: &str) -> Result<(), DomainError> {
        let mut suite = self.load_suite(suite_id)?;
        composition::detach_case_from_suite(&mut suite, case_id)?;
        self.save_suite(suite_id, &suite)
    }

    /// Adds the suite named in `body` to a run.
    pub fn add_suite_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let suite_id = required_string(body, "suiteId")
            .ok_or_else(|| DomainError::invalid_request("Required field suiteId is missing"))?;

        let mut run = self.load_run(run_id)?;
        let suite = self.load_suite(&suite_id)?;
        composition::attach_suite_to_run(&mut run, &suite, &suite_id)?;
        self.save_run(run_id, &run)
    }

    /// Adds the case named in `body` to a run.
    pub fn add_case_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let test_case_id = required_string(body, "testCaseId")
            .ok_or_else(|| DomainError::invalid_request("Required field testCaseId is missing"))?;

        let mut run = self.load_run(run_id)?;
        let test_case = self.load_case(&test_case_id)?;
        composition::attach_case_to_run(&mut run, &test_case, &test_case_id)?;
        self.save_run(run_id, &run)
    }

    /// Records, or replaces, the result of a case within a run.
    pub fn record_run_result(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let test_case_id = required_string(body, "testCaseId")
            .ok_or_else(|| DomainError::invalid_request("Required field testCaseId is missing"))?;
        let status = required_string(body, "status")
            .ok_or_else(|| DomainError::invalid_request("Required field status is missing"))?;
        if !VALID_STATUSES.contains(&status.as_str()) {
            return Err(DomainError::invalid_status());
        }

        let mut run = self.load_run(run_id)?;
        let result = TestCaseResult {
            test_case_id,
            status,
            timestamp: required_string(body, "timestamp").unwrap_or_else(current_timestamp_string),
            notes: required_string(body, "notes"),
            attachments: None,
        };
        composition::upsert_result(&mut run, result);
        self.save_run(run_id, &run)
    }

    // --- reporting -----------------------------------------------------

    /// Reports a milestone's progress from the runs it references.
    pub fn milestone_progress(&self, id: &str) -> Result<MilestoneProgress, DomainError> {
        let value = self
            .repository
            .read(Resource::Milestones, id)
            .map_err(error::milestone_error)?;
        let milestone: Milestone = serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored milestone JSON is invalid".to_owned()))?;

        let mut runs = Vec::new();
        for run_id in milestone.test_run_ids.as_deref().unwrap_or_default() {
            let Ok(run_value) = self.repository.read(Resource::Runs, run_id) else {
                continue;
            };
            let Ok(run) = serde_json::from_value::<TestRun>(run_value) else {
                continue;
            };
            runs.push(run);
        }

        Ok(progress::compute(&milestone, &runs))
    }

    // --- attachments ---------------------------------------------------

    /// Confirms a test case exists before an attachment is accepted.
    pub fn require_test_case(&self, id: &str) -> Result<(), DomainError> {
        if self.repository.exists(Resource::Cases, id)? {
            Ok(())
        } else {
            Err(DomainError::NotFound("Test case not found".to_owned()))
        }
    }

    /// Stores an uploaded file against a test case.
    pub fn store_attachment(
        &self,
        id: &str,
        original_name: &str,
        contents: &[u8],
    ) -> Result<StoredAttachment, DomainError> {
        if contents.len() > MAX_ATTACHMENT_BYTES {
            return Err(DomainError::PayloadTooLarge);
        }

        let filename = format!("{}-{}", unique_suffix(), original_name);
        self.repository.save_attachment(id, &filename, contents)?;
        Ok(StoredAttachment {
            filename,
            original_name: original_name.to_owned(),
            size: contents.len(),
        })
    }

    /// Reads a stored attachment.
    pub fn read_attachment(&self, id: &str, filename: &str) -> Result<Vec<u8>, DomainError> {
        self.repository
            .read_attachment(id, filename)
            .map_err(error::attachment_error)
    }

    /// Deletes a stored attachment.
    pub fn delete_attachment(&self, id: &str, filename: &str) -> Result<(), DomainError> {
        self.repository
            .delete_attachment(id, filename)
            .map_err(error::attachment_error)
    }

    // --- internals -----------------------------------------------------

    fn load_suite(&self, id: &str) -> Result<TestSuite, DomainError> {
        let value = self
            .repository
            .read(Resource::Suites, id)
            .map_err(|error| error::load_error(error, "Test suite not found"))?;
        serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored suite JSON is invalid".to_owned()))
    }

    fn save_suite(&self, id: &str, suite: &TestSuite) -> Result<(), DomainError> {
        let value = serde_json::to_value(suite)
            .map_err(|_| DomainError::Internal("Failed to serialize suite".to_owned()))?;
        self.repository
            .write(Resource::Suites, id, &value)
            .map_err(DomainError::from)
    }

    fn load_run(&self, id: &str) -> Result<TestRun, DomainError> {
        let value = self
            .repository
            .read(Resource::Runs, id)
            .map_err(|error| error::load_error(error, "Test run not found"))?;
        serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored run JSON is invalid".to_owned()))
    }

    fn save_run(&self, id: &str, run: &TestRun) -> Result<(), DomainError> {
        let value = serde_json::to_value(run)
            .map_err(|_| DomainError::Internal("Failed to serialize test run".to_owned()))?;
        self.repository
            .write(Resource::Runs, id, &value)
            .map_err(DomainError::from)
    }

    fn load_case(&self, id: &str) -> Result<TestCase, DomainError> {
        let value = self
            .repository
            .read(Resource::Cases, id)
            .map_err(|error| error::load_error(error, "Test case not found"))?;
        serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored case JSON is invalid".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::FileRepository;
    use serde_json::json;
    use tempfile::TempDir;

    fn service() -> (TestService<FileRepository>, TempDir) {
        let directory = TempDir::new().expect("temporary directory");
        let repository = FileRepository::new(directory.path()).expect("repository");
        (TestService::new(repository), directory)
    }

    fn list(service: &TestService<FileRepository>, resource: Resource) -> Vec<String> {
        service
            .list(resource, &ListQuery::default())
            .expect("list should succeed")
    }

    #[test]
    fn documents_round_trip_through_the_service() {
        let (service, _directory) = service();

        let created = service
            .create(Resource::Projects, &json!({ "name": "checkout" }))
            .expect("create");
        assert_eq!(created.id, "checkout.json");
        assert_eq!(list(&service, Resource::Projects), vec!["checkout.json"]);

        let stored = service
            .get(Resource::Projects, "checkout.json")
            .expect("get");
        assert_eq!(stored["name"], "checkout");

        service
            .update(
                Resource::Projects,
                "checkout.json",
                &json!({ "name": "updated" }),
            )
            .expect("update");
        assert_eq!(
            service
                .get(Resource::Projects, "checkout.json")
                .expect("get")["name"],
            "updated"
        );

        service
            .delete(Resource::Projects, "checkout.json")
            .expect("delete");
        assert!(service.get(Resource::Projects, "checkout.json").is_err());
        assert!(list(&service, Resource::Projects).is_empty());
    }

    #[test]
    fn creating_a_duplicate_is_a_conflict() {
        let (service, _directory) = service();
        service
            .create(Resource::Projects, &json!({ "name": "checkout" }))
            .expect("create");

        let error = service
            .create(Resource::Projects, &json!({ "name": "checkout" }))
            .expect_err("duplicate");
        assert!(matches!(error, DomainError::Conflict(_)));
    }

    #[test]
    fn an_unknown_field_is_rejected_before_anything_is_persisted() {
        let (service, _directory) = service();

        let error = service
            .create(Resource::Projects, &json!({ "name": "alpha", "sneaky": 1 }))
            .expect_err("unknown field");
        assert!(matches!(
            error,
            DomainError::InvalidRequest {
                code: "invalid_request",
                ..
            }
        ));
        assert!(
            list(&service, Resource::Projects).is_empty(),
            "a rejected payload must not be stored"
        );
    }

    #[test]
    fn updating_a_missing_document_is_not_found() {
        let (service, _directory) = service();
        let error = service
            .update(
                Resource::Projects,
                "missing.json",
                &json!({ "name": "missing" }),
            )
            .expect_err("missing");
        assert!(matches!(error, DomainError::NotFound(_)));
    }

    #[test]
    fn listing_filters_by_substring_and_by_tag() {
        let (service, _directory) = service();
        service
            .create(Resource::Projects, &json!({ "name": "alpha" }))
            .expect("alpha");
        service
            .create(
                Resource::Projects,
                &json!({ "name": "beta", "tags": ["Smoke"] }),
            )
            .expect("beta");

        let filtered = service
            .list(
                Resource::Projects,
                &ListQuery {
                    filter: Some("ALPH".to_owned()),
                    tags: None,
                },
            )
            .expect("filter");
        assert_eq!(filtered, vec!["alpha.json"]);

        let tagged = service
            .list(
                Resource::Projects,
                &ListQuery {
                    filter: None,
                    tags: Some(" smoke ".to_owned()),
                },
            )
            .expect("tags");
        assert_eq!(tagged, vec!["beta.json"]);

        let untagged = service
            .list(
                Resource::Projects,
                &ListQuery {
                    filter: None,
                    tags: Some("does-not-exist".to_owned()),
                },
            )
            .expect("tags");
        assert!(untagged.is_empty());
    }

    #[test]
    fn duplicating_a_project_stores_a_copy_under_a_new_identifier() {
        let (service, _directory) = service();
        service
            .create(
                Resource::Projects,
                &json!({ "projectId": "P-1", "name": "checkout" }),
            )
            .expect("create");

        let new_id = service
            .duplicate(
                &duplicate::PROJECT,
                "checkout.json",
                &json!({ "newId": "P-2.json" }),
            )
            .expect("duplicate");
        assert_eq!(new_id, "P-2.json");

        let copy = service.get(Resource::Projects, "P-2.json").expect("copy");
        assert_eq!(copy["projectId"], "P-2.json");
        assert_eq!(copy["name"], "checkout");
        assert!(
            service.get(Resource::Projects, "checkout.json").is_ok(),
            "the source must remain"
        );
    }

    #[test]
    fn duplicating_a_missing_document_reports_the_named_entity() {
        let (service, _directory) = service();
        let error = service
            .duplicate(&duplicate::RUN, "missing.json", &json!({}))
            .expect_err("missing");
        assert!(matches!(error, DomainError::NotFound(message) if message == "Test run not found"));
    }

    #[test]
    fn cases_join_suites_through_the_service() {
        let (service, _directory) = service();
        service
            .create(
                Resource::Suites,
                &json!({ "suiteId": "S-001.json", "name": "S-001", "testCases": [] }),
            )
            .expect("suite");
        service
            .create(
                Resource::Cases,
                &json!({ "testCaseId": "TC-001.json", "title": "T", "expectedResult": "E" }),
            )
            .expect("case");

        service
            .add_case_to_suite("S-001.json", &json!({ "testCaseId": "TC-001.json" }))
            .expect("add");
        let suite = service.get(Resource::Suites, "S-001.json").expect("suite");
        assert_eq!(suite["testCases"].as_array().map(Vec::len), Some(1));
        assert_eq!(suite["testCases"][0]["testCaseId"], "TC-001.json");

        let duplicate = service
            .add_case_to_suite("S-001.json", &json!({ "testCaseId": "TC-001.json" }))
            .expect_err("duplicate");
        assert!(matches!(duplicate, DomainError::Conflict(_)));

        let missing = service
            .add_case_to_suite("S-001.json", &json!({}))
            .expect_err("missing field");
        assert!(matches!(missing, DomainError::InvalidRequest { .. }));

        service
            .remove_case_from_suite("S-001.json", "TC-001.json")
            .expect("remove");
        let suite = service.get(Resource::Suites, "S-001.json").expect("suite");
        assert_eq!(suite["testCases"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn run_results_are_recorded_and_replaced() {
        let (service, _directory) = service();
        service
            .create(
                Resource::Runs,
                &json!({ "testRunId": "R-1", "name": "nightly", "timestamp": "1" }),
            )
            .expect("run");

        service
            .record_run_result(
                "nightly.json",
                &json!({ "testCaseId": "TC-1", "status": "Passed" }),
            )
            .expect("record");
        service
            .record_run_result(
                "nightly.json",
                &json!({ "testCaseId": "TC-1", "status": "Failed", "notes": "flaky" }),
            )
            .expect("replace");

        let run = service.get(Resource::Runs, "nightly.json").expect("run");
        let results = run["results"].as_array().expect("results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["status"], "Failed");
        assert_eq!(results[0]["notes"], "flaky");

        let invalid = service
            .record_run_result(
                "nightly.json",
                &json!({ "testCaseId": "TC-1", "status": "Nope" }),
            )
            .expect_err("bad status");
        assert!(matches!(
            invalid,
            DomainError::InvalidRequest {
                code: "invalid_status",
                ..
            }
        ));
    }

    #[test]
    fn milestone_progress_reads_the_referenced_runs() {
        let (service, _directory) = service();
        service
            .create(
                Resource::Runs,
                &json!({
                    "testRunId": "R-1",
                    "name": "nightly",
                    "timestamp": "1",
                    "results": [
                        { "testCaseId": "TC-1", "status": "Passed", "timestamp": "1" },
                        { "testCaseId": "TC-2", "status": "Failed", "timestamp": "1" },
                        { "testCaseId": "TC-3", "status": "Blocked", "timestamp": "1" }
                    ]
                }),
            )
            .expect("run");
        service
            .create(
                Resource::Milestones,
                &json!({ "milestoneId": "M-1", "name": "v1.0", "testRunIds": ["nightly.json"] }),
            )
            .expect("milestone");

        let progress = service.milestone_progress("M-1.json").expect("progress");
        assert_eq!(progress.milestone_id, "M-1");
        assert_eq!(progress.total_cases, 3);
        assert_eq!(progress.passed, 1);
        assert_eq!(progress.pass_percentage, 33.33333333333333);
    }

    #[test]
    fn progress_for_a_missing_milestone_is_not_found() {
        let (service, _directory) = service();
        let error = service
            .milestone_progress("missing.json")
            .expect_err("missing");
        assert!(
            matches!(error, DomainError::NotFound(message) if message == "Milestone not found")
        );
    }

    #[test]
    fn attachments_are_stored_read_and_deleted() {
        let (service, _directory) = service();
        service
            .create(
                Resource::Cases,
                &json!({ "testCaseId": "TC-1", "title": "T", "expectedResult": "E" }),
            )
            .expect("case");

        let stored = service
            .store_attachment("TC-1", "notes.txt", b"evidence")
            .expect("store");
        assert_eq!(stored.original_name, "notes.txt");
        assert_eq!(stored.size, 8);
        assert!(stored.filename.ends_with("-notes.txt"));

        assert_eq!(
            service
                .read_attachment("TC-1", &stored.filename)
                .expect("read"),
            b"evidence"
        );

        service
            .delete_attachment("TC-1", &stored.filename)
            .expect("delete");
        assert!(service.read_attachment("TC-1", &stored.filename).is_err());
    }

    #[test]
    fn requiring_a_missing_test_case_is_not_found() {
        let (service, _directory) = service();
        assert!(matches!(
            service.require_test_case("TC-1").expect_err("missing"),
            DomainError::NotFound(_)
        ));
    }
}
