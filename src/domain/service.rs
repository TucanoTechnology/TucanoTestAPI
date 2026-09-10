//! The application, expressed against the storage boundary.
//!
//! [`TestService`] is the only thing the HTTP layer talks to. It owns no state
//! beyond the repository it was built with, so replicas sharing the same storage
//! behave identically, and every rule it applies lives in a sibling module that
//! can be unit tested on its own.
//!
//! Storage holds a tree; this layer turns it into documents. A project or suite
//! marker stores empty child collections and a read assembles the real children
//! from their folders, so membership has exactly one home: the folder. An
//! identifier that several parents own is therefore a conflict rather than an
//! arbitrary pick — the caller has to say which parent it meant.

use std::collections::HashSet;
use std::io;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::models::{
    ImportCounts, ImportSummary, Milestone, MilestoneProgress, TestCase, TestCaseResult,
    TestConfiguration, TestRun, TestSuite,
};
use crate::storage::{Parent, Placement, Repository, Resource, unique_suffix};

use super::duplicate::{self, DuplicateSpec};
use super::error::{self, DomainError};
use super::import::{self, ImportStatus};
use super::{
    Created, ListQuery, MAX_ATTACHMENT_BYTES, StoredAttachment, composition,
    current_timestamp_string, mime_type, progress, required_string, resources, validation,
};

/// Result statuses a test run accepts.
const VALID_STATUSES: [&str; 5] = ["Passed", "Failed", "Blocked", "Untested", "Retest"];

/// How a composition request was satisfied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Composed {
    /// A new entity was created in the target parent.
    Created(Created),
    /// An existing entity was copied or moved into the target parent.
    Placed { id: String, mode: Placement },
}

impl Composed {
    /// Identifier of the entity the request addressed.
    pub fn id(&self) -> &str {
        match self {
            Self::Created(created) => &created.id,
            Self::Placed { id, .. } => id,
        }
    }

    /// Response message, phrased for `noun` (for example `Test suite`).
    pub fn message(&self, noun: &str) -> String {
        match self {
            Self::Created(_) => format!("{noun} created"),
            Self::Placed {
                mode: Placement::Copy,
                ..
            } => format!("{noun} copied"),
            Self::Placed {
                mode: Placement::Move,
                ..
            } => format!("{noun} moved"),
        }
    }
}

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
                let Ok(value) = self.first_document(resource, item) else {
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

        // Only runs carry configuration references, and an unnamed
        // configuration yields an empty listing rather than an error, matching
        // how the substring and tag filters already behave.
        if resource == Resource::Runs
            && let Some(config_id) = query.configuration.as_ref()
        {
            items.retain(|item| {
                let Ok(value) = self.first_document(resource, item) else {
                    return false;
                };
                let Some(configurations) = value.get("configurations").and_then(Value::as_array)
                else {
                    return false;
                };
                configurations.iter().any(|configuration| {
                    configuration.get("configId").and_then(Value::as_str)
                        == Some(config_id.as_str())
                })
            });
        }

        Ok(items)
    }

    /// Reads a single document, assembling the children a parent owns.
    pub fn get(&self, resource: Resource, id: &str) -> Result<Value, DomainError> {
        self.assembled(resource, id, "Resource not found")
    }

    /// Identifiers of the children `parent` owns.
    pub fn list_children(
        &self,
        parent: &Parent,
        child: Resource,
    ) -> Result<Vec<String>, DomainError> {
        self.require_parent(parent)?;
        self.repository
            .list_children(parent, child)
            .map_err(error::read_error)
    }

    /// Validates, names and stores a new document in a collection that has no
    /// parent of its own — projects and the flat resources.
    pub fn create(&self, resource: Resource, value: &Value) -> Result<Created, DomainError> {
        match resource {
            Resource::Suites => Err(DomainError::invalid_request(
                "Test suites are created inside a project: POST /projects/{id}/test_suites",
            )),
            Resource::Cases => Err(DomainError::invalid_request(
                "Test cases are created inside a project or a suite: POST /projects/{id}/test_cases or POST /test_suites/{id}/test_cases",
            )),
            _ => self.create_at(resource, None, value),
        }
    }

    /// Validates, names and stores a new document inside `parent`.
    pub fn create_in(
        &self,
        resource: Resource,
        parent: &Parent,
        value: &Value,
    ) -> Result<Created, DomainError> {
        self.require_parent(parent)?;
        self.create_at(resource, Some(parent), value)
    }

    /// Validates a partial body and merges it into an existing document.
    ///
    /// A `PUT` overlays the fields the body carries onto the stored document, so
    /// the fields it leaves out keep their stored values and a partial body can
    /// never store a document the API cannot read back.
    pub fn update(&self, resource: Resource, id: &str, value: &Value) -> Result<(), DomainError> {
        validation::validate_payload(resource, value)?;
        let parent = self.owner_for_write(resource, id, "Resource not found")?;
        let stored = self
            .repository
            .read_at(resource, parent.as_ref(), id)
            .map_err(error::read_error)?;
        let merged = merged_document(&stored, value)?;
        self.write_marker(resource, parent.as_ref(), id, &merged)
    }

    /// Removes a document, its folder, and everything it owns.
    pub fn delete(&self, resource: Resource, id: &str) -> Result<(), DomainError> {
        let parent = self.owner_for_write(resource, id, "Resource not found")?;
        self.repository
            .delete_at(resource, parent.as_ref(), id)
            .map_err(error::delete_error)
    }

    /// Removes one occurrence of a child from a parent the caller named.
    pub fn delete_in(
        &self,
        resource: Resource,
        parent: &Parent,
        id: &str,
    ) -> Result<(), DomainError> {
        self.require_parent(parent)?;
        self.repository
            .delete_at(resource, Some(parent), id)
            .map_err(error::delete_error)
    }

    // --- composition ---------------------------------------------------

    /// Creates a new entity in `target`, or places an existing one there.
    ///
    /// The body's grammar decides which: a body carrying the fields a creation
    /// needs creates, a body naming only an existing identifier places. A body
    /// that mixes the two, or that asks for an unknown `mode`, is rejected.
    pub fn compose(
        &self,
        resource: Resource,
        target: &Parent,
        body: &Value,
    ) -> Result<Composed, DomainError> {
        if creates(resource, body)? {
            return self
                .create_in(resource, target, body)
                .map(Composed::Created);
        }
        self.place_into(resource, target, body)
    }

    /// The parent a suite's own cases belong to, resolved from a bare suite id.
    pub fn suite_parent(&self, suite_id: &str) -> Result<Parent, DomainError> {
        let home = self.parent_of(Resource::Suites, suite_id, "Test suite not found")?;
        Ok(Parent::Suite {
            project: home.project().to_owned(),
            suite: suite_id.to_owned(),
        })
    }

    /// Adds the case named in `body` to a suite: a creation when the body
    /// carries a title and an expected result, otherwise a copy or a move.
    pub fn add_case_to_suite(&self, suite_id: &str, body: &Value) -> Result<(), DomainError> {
        let parent = self.suite_parent(suite_id)?;
        self.compose(Resource::Cases, &parent, body)?;
        Ok(())
    }

    /// Removes one occurrence of a case from a suite.
    pub fn remove_case_from_suite(&self, suite_id: &str, case_id: &str) -> Result<(), DomainError> {
        let parent = self.suite_parent(suite_id)?;
        self.delete_in(Resource::Cases, &parent, case_id)
    }

    /// Adds the suite named in `body` to a run, embedding a snapshot copy. Runs
    /// are never a home for a suite: the source keeps its project.
    pub fn add_suite_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let suite_id = required_string(body, "suiteId")
            .ok_or_else(|| DomainError::invalid_request("Required field suiteId is missing"))?;

        let mut run = self.load::<TestRun>(Resource::Runs, run_id, "Test run not found")?;
        let suite = self.load_entity::<TestSuite>(Resource::Suites, &suite_id)?;
        composition::attach_suite_to_run(&mut run, &suite, &suite_id)?;
        self.save(Resource::Runs, run_id, &run)
    }

    /// Adds the case named in `body` to a run, embedding a snapshot copy.
    pub fn add_case_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let test_case_id = required_string(body, "testCaseId")
            .ok_or_else(|| DomainError::invalid_request("Required field testCaseId is missing"))?;

        let mut run = self.load::<TestRun>(Resource::Runs, run_id, "Test run not found")?;
        let test_case = self.load_entity::<TestCase>(Resource::Cases, &test_case_id)?;
        composition::attach_case_to_run(&mut run, &test_case, &test_case_id)?;
        self.save(Resource::Runs, run_id, &run)
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

        let mut run = self.load::<TestRun>(Resource::Runs, run_id, "Test run not found")?;
        let result = TestCaseResult {
            test_case_id,
            status,
            timestamp: required_string(body, "timestamp").unwrap_or_else(current_timestamp_string),
            notes: required_string(body, "notes"),
            attachments: None,
        };
        composition::upsert_result(&mut run, result);
        self.save(Resource::Runs, run_id, &run)
    }

    /// Imports a JUnit report's testcases into a run's results.
    ///
    /// Cases the run already records are counted as duplicates and left
    /// untouched, so re-importing a report is safe and never overwrites a result
    /// someone recorded by hand. Cases that cannot be named are counted as errors
    /// rather than failing the whole import.
    pub fn import_junit_results(
        &self,
        run_id: &str,
        xml: &str,
    ) -> Result<ImportSummary, DomainError> {
        let report = import::parse(xml)?;

        let mut run = self.load::<TestRun>(Resource::Runs, run_id, "Test run not found")?;
        let mut seen: HashSet<String> = run
            .results
            .as_ref()
            .map(|results| {
                results
                    .iter()
                    .map(|result| result.test_case_id.clone())
                    .collect()
            })
            .unwrap_or_default();

        let mut summary = ImportCounts {
            passed: 0,
            failed: 0,
            blocked: 0,
        };
        let mut imported = 0;
        let mut duplicates = 0;

        for case in report.cases {
            if !seen.insert(case.test_case_id.clone()) {
                duplicates += 1;
                continue;
            }

            match case.status {
                ImportStatus::Passed => summary.passed += 1,
                ImportStatus::Failed => summary.failed += 1,
                ImportStatus::Blocked => summary.blocked += 1,
            }

            composition::upsert_result(
                &mut run,
                TestCaseResult {
                    test_case_id: case.test_case_id,
                    status: case.status.as_str().to_owned(),
                    timestamp: case.timestamp.unwrap_or_else(current_timestamp_string),
                    notes: case.notes,
                    attachments: None,
                },
            );
            imported += 1;
        }

        let errors = report.errors;
        let outcome = ImportSummary {
            imported,
            skipped: duplicates + errors,
            errors,
            duplicates,
            summary,
        };
        self.save(Resource::Runs, run_id, &run)?;
        Ok(outcome)
    }

    /// Links the top-level configuration named in `body` to a run by reference,
    /// refusing one the run already references.
    ///
    /// A configuration owns its storage, so the run keeps a reference to it
    /// rather than a copy; the referenced configuration is verified to exist.
    pub fn link_configuration_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let config_id = required_string(body, "configId")
            .ok_or_else(|| DomainError::invalid_request("Required field configId is missing"))?;

        let mut run = self.load::<TestRun>(Resource::Runs, run_id, "Test run not found")?;
        let configuration =
            self.load_entity::<TestConfiguration>(Resource::Configurations, &config_id)?;
        composition::attach_configuration_to_run(&mut run, &configuration, &config_id)?;
        self.save(Resource::Runs, run_id, &run)
    }

    /// Removes a configuration reference from a run.
    pub fn unlink_configuration_from_run(
        &self,
        run_id: &str,
        config_id: &str,
    ) -> Result<(), DomainError> {
        let mut run = self.load::<TestRun>(Resource::Runs, run_id, "Test run not found")?;
        composition::detach_configuration_from_run(&mut run, config_id)?;
        self.save(Resource::Runs, run_id, &run)
    }

    // --- duplication ---------------------------------------------------

    /// Copies a document, applying the request-body overrides, and returns the
    /// identifier of the copy.
    ///
    /// A copied suite or case lands in the parent the source belongs to, so the
    /// copy stays where the original is; only projects and the flat resources
    /// live at the top level.
    pub fn duplicate(
        &self,
        spec: &DuplicateSpec,
        id: &str,
        body: &Value,
    ) -> Result<String, DomainError> {
        let parent = self.owner_for_write(spec.resource, id, spec.not_found_message)?;
        let mut document = self
            .repository
            .read_at(spec.resource, parent.as_ref(), id)
            .map_err(|error| error::load_error(error, spec.not_found_message))?;
        let new_id = duplicate::apply_overrides(spec, id, body, &mut document);

        if self
            .repository
            .exists_at(spec.resource, parent.as_ref(), &new_id)?
        {
            return Err(DomainError::Conflict(
                spec.already_exists_message.to_owned(),
            ));
        }

        self.write_marker(spec.resource, parent.as_ref(), &new_id, &document)?;
        Ok(new_id)
    }

    // --- reporting -----------------------------------------------------

    /// Reports a milestone's progress from the runs it references.
    pub fn milestone_progress(&self, id: &str) -> Result<MilestoneProgress, DomainError> {
        let value = self
            .repository
            .read_at(Resource::Milestones, None, id)
            .map_err(error::milestone_error)?;
        let milestone: Milestone = serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored milestone JSON is invalid".to_owned()))?;

        let mut runs = Vec::new();
        for run_id in milestone.test_run_ids.as_deref().unwrap_or_default() {
            let Ok(run_value) = self.repository.read_at(Resource::Runs, None, run_id) else {
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

    /// Resolves the single test case named by `id`, so an attachment request
    /// reaches the occurrence the caller meant.
    pub fn require_test_case(&self, id: &str) -> Result<Parent, DomainError> {
        self.parent_of(Resource::Cases, id, "Test case not found")
    }

    /// Stores an uploaded file against a test case folder.
    pub fn store_attachment(
        &self,
        parent: &Parent,
        id: &str,
        original_name: &str,
        contents: &[u8],
    ) -> Result<StoredAttachment, DomainError> {
        if contents.len() > MAX_ATTACHMENT_BYTES {
            return Err(DomainError::PayloadTooLarge);
        }

        let filename = format!("{}-{}", unique_suffix(), original_name);
        let entry = json!({
            "filename": filename,
            "originalName": original_name,
            "mimeType": mime_type(&filename),
            "size": contents.len(),
        });
        self.repository
            .save_attachment(parent, id, &filename, &entry, contents)
            .map_err(attachment_write_error)?;

        Ok(StoredAttachment {
            filename,
            original_name: original_name.to_owned(),
            size: contents.len(),
        })
    }

    /// Reads a stored attachment.
    pub fn read_attachment(
        &self,
        parent: &Parent,
        id: &str,
        filename: &str,
    ) -> Result<Vec<u8>, DomainError> {
        self.repository
            .read_attachment(parent, id, filename)
            .map_err(error::attachment_error)
    }

    /// Deletes a stored attachment.
    pub fn delete_attachment(
        &self,
        parent: &Parent,
        id: &str,
        filename: &str,
    ) -> Result<(), DomainError> {
        self.repository
            .delete_attachment(parent, id, filename)
            .map_err(error::attachment_error)
    }

    /// Stores an uploaded file against one structured step of a test case.
    pub fn store_step_attachment(
        &self,
        parent: &Parent,
        id: &str,
        step_index: usize,
        original_name: &str,
        contents: &[u8],
    ) -> Result<StoredAttachment, DomainError> {
        if contents.len() > MAX_ATTACHMENT_BYTES {
            return Err(DomainError::PayloadTooLarge);
        }
        self.structured_step(parent, id, step_index)?;

        let filename = format!("{}-{}", unique_suffix(), original_name);
        let entry = json!({
            "filename": filename,
            "originalName": original_name,
            "mimeType": mime_type(&filename),
            "size": contents.len(),
        });
        self.repository
            .save_step_attachment(parent, id, step_index, &filename, &entry, contents)
            .map_err(attachment_write_error)?;

        Ok(StoredAttachment {
            filename,
            original_name: original_name.to_owned(),
            size: contents.len(),
        })
    }

    /// Lists the attachment metadata recorded for one structured step.
    pub fn list_step_attachments(
        &self,
        parent: &Parent,
        id: &str,
        step_index: usize,
    ) -> Result<Vec<Value>, DomainError> {
        let step = self.structured_step(parent, id, step_index)?;
        Ok(step
            .get("attachments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Deletes a stored attachment of one structured step.
    pub fn delete_step_attachment(
        &self,
        parent: &Parent,
        id: &str,
        step_index: usize,
        filename: &str,
    ) -> Result<(), DomainError> {
        self.repository
            .delete_step_attachment(parent, id, step_index, filename)
            .map_err(error::attachment_error)
    }

    /// Resolves one structured step of a test case, so a step attachment only
    /// ever addresses a step that exists and can carry metadata. A step that is
    /// absent, out of range, or a plain string is an invalid request.
    fn structured_step(
        &self,
        parent: &Parent,
        id: &str,
        step_index: usize,
    ) -> Result<Value, DomainError> {
        let document =
            self.read_document(Resource::Cases, Some(parent), id, "Test case not found")?;
        let step = document
            .get("steps")
            .and_then(Value::as_array)
            .and_then(|steps| steps.get(step_index))
            .ok_or_else(|| {
                DomainError::invalid_request(format!("Step index {step_index} is out of range"))
            })?;
        match step {
            Value::Object(_) => Ok(step.clone()),
            _ => Err(DomainError::invalid_request(format!(
                "Step {step_index} is a plain string step, not a structured step"
            ))),
        }
    }

    // --- internals -----------------------------------------------------

    /// Creates a document at the addressed location.
    fn create_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        value: &Value,
    ) -> Result<Created, DomainError> {
        validation::validate_payload(resource, value)?;
        let id = resources::derive_create_id(resource, value)?;
        if self.repository.exists_at(resource, parent, &id)? {
            return Err(DomainError::Conflict("Resource already exists".to_owned()));
        }
        self.write_marker(resource, parent, &id, value)?;
        Ok(Created { id })
    }

    /// Persists a document, keeping a parent marker's child collections empty:
    /// membership lives in the folders, never in the parent document.
    fn write_marker(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> Result<(), DomainError> {
        let mut document = value.clone();
        normalise_marker(resource, id, &mut document);
        self.repository
            .write_at(resource, parent, id, &document)
            .map_err(DomainError::from)
    }

    /// Copies or moves an existing entity into `target`.
    fn place_into(
        &self,
        resource: Resource,
        target: &Parent,
        body: &Value,
    ) -> Result<Composed, DomainError> {
        let id_field = match resource {
            Resource::Suites => "suiteId",
            Resource::Cases => "testCaseId",
            _ => {
                return Err(DomainError::invalid_request(
                    "Only test suites and test cases can be placed",
                ));
            }
        };
        let id = required_string(body, id_field).ok_or_else(|| {
            DomainError::invalid_request(format!("Required field {id_field} is missing"))
        })?;
        let mode = placement_mode(body);
        let source = self.parent_of(resource, &id, entity_missing_message(resource))?;

        if resource == Resource::Suites && !matches!(target, Parent::Project(_)) {
            return Err(DomainError::invalid_request(
                "A test suite can only be placed into a project",
            ));
        }
        self.require_parent(target)?;
        self.repository
            .place(resource, &source, &id, target, mode)
            .map_err(placement_error)?;

        Ok(Composed::Placed { id, mode })
    }

    /// Reads a document the way a `GET` would, assembling its children.
    fn assembled(&self, resource: Resource, id: &str, missing: &str) -> Result<Value, DomainError> {
        match resource {
            Resource::Projects => {
                let parent = Parent::Project(id.to_owned());
                let mut document = self.read_document(resource, None, id, missing)?;
                let suites = self.child_suites(&parent)?;
                let cases = self.child_documents(&parent, Resource::Cases)?;
                if let Some(object) = document.as_object_mut() {
                    object.insert("testSuites".to_owned(), Value::Array(suites));
                    if cases.is_empty() {
                        object.remove("testCases");
                    } else {
                        object.insert("testCases".to_owned(), Value::Array(cases));
                    }
                }
                Ok(document)
            }
            Resource::Suites => {
                // `locate` reports the suite's home, which is the project that
                // holds it; the suite's own children hang off the suite folder.
                let project = self.parent_of(Resource::Suites, id, missing)?;
                self.assemble_suite(&project, id, missing)
            }
            Resource::Cases => {
                let parent = self.parent_of(Resource::Cases, id, missing)?;
                self.read_document(resource, Some(&parent), id, missing)
            }
            _ => self.read_document(resource, None, id, missing),
        }
    }

    /// Assembles every suite a project owns, each with the cases it holds.
    fn child_suites(&self, project: &Parent) -> Result<Vec<Value>, DomainError> {
        let identifiers = self
            .repository
            .list_children(project, Resource::Suites)
            .map_err(error::read_error)?;
        identifiers
            .iter()
            .map(|id| self.assemble_suite(project, id, "Resource not found"))
            .collect()
    }

    /// Reads a suite marker and replaces its empty child array with the cases
    /// stored in the suite folder.
    fn assemble_suite(
        &self,
        project: &Parent,
        id: &str,
        missing: &str,
    ) -> Result<Value, DomainError> {
        let suite = Parent::Suite {
            project: project.project().to_owned(),
            suite: id.to_owned(),
        };
        let mut document = self.read_document(Resource::Suites, Some(project), id, missing)?;
        let cases = self.child_documents(&suite, Resource::Cases)?;
        if let Some(object) = document.as_object_mut() {
            object.insert("testCases".to_owned(), Value::Array(cases));
        }
        Ok(document)
    }

    /// Assembles every child a parent owns, ordered by folder name.
    fn child_documents(&self, parent: &Parent, child: Resource) -> Result<Vec<Value>, DomainError> {
        let identifiers = self
            .repository
            .list_children(parent, child)
            .map_err(error::read_error)?;
        identifiers
            .iter()
            .map(|id| self.read_document(child, Some(parent), id, "Resource not found"))
            .collect()
    }

    /// Reads one stored document and reports `missing` when it is absent.
    fn read_document(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        missing: &str,
    ) -> Result<Value, DomainError> {
        self.repository
            .read_at(resource, parent, id)
            .map_err(|error| error::document_error(error, missing))
    }

    /// The identifier and the document of the first occurrence, for filters
    /// that need the stored fields of an identifier several parents may own.
    fn first_document(&self, resource: Resource, id: &str) -> io::Result<Value> {
        match resource {
            Resource::Suites | Resource::Cases => {
                let home = self
                    .repository
                    .locate(resource, id)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "resource not found"))?;
                self.repository.read_at(resource, Some(&home), id)
            }
            _ => self.repository.read_at(resource, None, id),
        }
    }

    /// The parent that owns the single occurrence of `id`, for the globally
    /// addressed routes. Zero occurrences is a 404; several is a conflict,
    /// because a bare identifier cannot say which parent was meant.
    fn parent_of(
        &self,
        resource: Resource,
        id: &str,
        missing: &str,
    ) -> Result<Parent, DomainError> {
        let homes = self
            .repository
            .locate(resource, id)
            .map_err(error::read_error)?;
        match homes.len() {
            0 => Err(DomainError::NotFound(missing.to_owned())),
            1 => Ok(homes.into_iter().next().expect("exactly one home")),
            _ => Err(ambiguous(resource, &homes)),
        }
    }

    /// The parent a write addresses, for resources that live inside one.
    fn owner_for_write(
        &self,
        resource: Resource,
        id: &str,
        missing: &str,
    ) -> Result<Option<Parent>, DomainError> {
        match resource {
            Resource::Suites | Resource::Cases => self.parent_of(resource, id, missing).map(Some),
            _ => Ok(None),
        }
    }

    /// Refuses a parent that does not exist, so a write can never invent one.
    fn require_parent(&self, parent: &Parent) -> Result<(), DomainError> {
        let present = match parent {
            Parent::Project(project) => {
                self.repository.exists_at(Resource::Projects, None, project)
            }
            Parent::Suite { project, suite } => {
                let owner = Parent::Project(project.clone());
                sort_result(
                    self.repository.exists_at(Resource::Projects, None, project),
                    self.repository
                        .exists_at(Resource::Suites, Some(&owner), suite),
                )
            }
        };
        match present {
            Ok(true) => Ok(()),
            Ok(false) => Err(DomainError::NotFound(
                parent_missing_message(parent).to_owned(),
            )),
            Err(error) => Err(error::delete_error(error)),
        }
    }

    /// Loads a stored document as its typed model, resolving the occurrence
    /// the way a `GET` would.
    fn load_entity<T: DeserializeOwned>(
        &self,
        resource: Resource,
        id: &str,
    ) -> Result<T, DomainError> {
        let value = self.assembled(resource, id, entity_missing_message(resource))?;
        serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored JSON is invalid".to_owned()))
    }

    fn load<T: DeserializeOwned>(
        &self,
        resource: Resource,
        id: &str,
        missing_message: &str,
    ) -> Result<T, DomainError> {
        let value = self.read_document(resource, None, id, missing_message)?;
        serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored JSON is invalid".to_owned()))
    }

    fn save<T: Serialize>(
        &self,
        resource: Resource,
        id: &str,
        value: &T,
    ) -> Result<(), DomainError> {
        let document = serde_json::to_value(value)
            .map_err(|_| DomainError::Internal("Failed to serialize document".to_owned()))?;
        self.repository
            .write_at(resource, None, id, &document)
            .map_err(DomainError::from)
    }
}

/// Normalises a document on its way to storage: a parent marker keeps its child
/// collections empty, because membership lives in the folders, and the identity
/// field the stored name stands for is filled in when the body did not carry
/// one, so a document the API wrote always reads back as its typed model. A run
/// also records the moment it was stored, because its model requires a
/// timestamp. A value the client supplied is never overwritten.
fn normalise_marker(resource: Resource, id: &str, document: &mut Value) {
    let (collection, identity): (Option<&str>, &str) = match resource {
        Resource::Projects => (Some("testSuites"), "projectId"),
        Resource::Suites => (Some("testCases"), "suiteId"),
        Resource::Runs => (None, "testRunId"),
        Resource::Milestones => (None, "milestoneId"),
        Resource::Configurations => (None, "configId"),
        // A test case cannot arrive without its identity: the id is derived from
        // it, so a body that omits `testCaseId` is refused before storage.
        Resource::Cases => return,
    };
    let Some(object) = document.as_object_mut() else {
        return;
    };
    if let Some(collection) = collection {
        object.insert(collection.to_owned(), Value::Array(Vec::new()));
    }
    if !object.get(identity).is_some_and(|value| value.is_string()) {
        object.insert(identity.to_owned(), Value::String(id.to_owned()));
    }
    if resource == Resource::Runs
        && !object
            .get("timestamp")
            .is_some_and(|value| value.is_string())
    {
        object.insert(
            "timestamp".to_owned(),
            Value::String(current_timestamp_string()),
        );
    }
}

/// Merges a validated update body into the document already stored, so an
/// update is partial: the fields the body carries are replaced and every other
/// stored field is kept.
///
/// Only the top level is merged — a field the body carries replaces the stored
/// field as a whole, so an array is replaced rather than concatenated — and a
/// field carrying `null` counts as not supplied and keeps the stored value,
/// the same reading of `null` the payload validation already uses. A stored
/// document that is not an object cannot be merged, and is reported rather than
/// silently replaced.
fn merged_document(stored: &Value, body: &Value) -> Result<Value, DomainError> {
    let Value::Object(stored) = stored else {
        return Err(DomainError::Internal("Stored JSON is invalid".to_owned()));
    };
    let mut merged = stored.clone();
    if let Some(fields) = body.as_object() {
        for (key, value) in fields {
            if !value.is_null() {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Object(merged))
}

/// Whether a composition body asks for a new entity rather than a placement.
fn creates(resource: Resource, body: &Value) -> Result<bool, DomainError> {
    let mode = body.get("mode");
    if let Some(mode) = mode
        && !matches!(mode.as_str(), Some("copy") | Some("move"))
    {
        return Err(DomainError::invalid_request(
            "Field `mode` must be `copy` or `move`",
        ));
    }

    let creation = match resource {
        Resource::Suites => required_string(body, "name").is_some(),
        Resource::Cases => required_string(body, "title").is_some(),
        _ => true,
    };

    if creation && mode.is_some() {
        return Err(DomainError::invalid_request(
            "A placement request cannot carry the fields that create a new resource",
        ));
    }
    Ok(creation)
}

/// Placement asked for by a body; `copy` is the default.
fn placement_mode(body: &Value) -> Placement {
    match body.get("mode").and_then(Value::as_str) {
        Some("move") => Placement::Move,
        _ => Placement::Copy,
    }
}

/// Translation for a placement failure: a taken name is a conflict, an
/// unusable identifier is a bad request, a missing source is a 404.
fn placement_error(error: io::Error) -> DomainError {
    match error.kind() {
        io::ErrorKind::AlreadyExists => DomainError::Conflict(
            "The target parent already holds a child with this identifier".to_owned(),
        ),
        io::ErrorKind::NotFound => DomainError::NotFound("Resource not found".to_owned()),
        io::ErrorKind::InvalidInput => DomainError::invalid_id(),
        _ => DomainError::from(error),
    }
}

/// Translation for writing an attachment: a name already taken is a conflict,
/// anything else follows the read rules so a bad filename stays a 404.
fn attachment_write_error(error: io::Error) -> DomainError {
    match error.kind() {
        io::ErrorKind::AlreadyExists => DomainError::Conflict("File already exists".to_owned()),
        _ => error::attachment_error(error),
    }
}

/// The conflict reported when a bare identifier names several occurrences.
fn ambiguous(resource: Resource, homes: &[Parent]) -> DomainError {
    let endpoints = match resource {
        Resource::Cases => {
            "POST /projects/{id}/test_cases, POST /test_suites/{id}/test_cases, or the matching /{case_id} delete"
        }
        _ => "POST /projects/{id}/test_suites, or the matching /{suite_id} delete",
    };
    let mut parents: Vec<String> = homes.iter().map(describe_parent).collect();
    parents.sort();
    DomainError::Conflict(format!(
        "This identifier is used by {} parents ({}); address the intended one through {endpoints}",
        homes.len(),
        parents.join(", ")
    ))
}

fn describe_parent(parent: &Parent) -> String {
    match parent {
        Parent::Project(project) => project.clone(),
        Parent::Suite { project, suite } => format!("{project}/{suite}"),
    }
}

fn parent_missing_message(parent: &Parent) -> &'static str {
    match parent {
        Parent::Project(_) => "Project not found",
        Parent::Suite { .. } => "Test suite not found",
    }
}

fn entity_missing_message(resource: Resource) -> &'static str {
    match resource {
        Resource::Projects => "Project not found",
        Resource::Suites => "Test suite not found",
        Resource::Cases => "Test case not found",
        _ => "Resource not found",
    }
}

/// The existence of a suite depends on its project, so both answers are needed
/// before the suite's own marker tells the truth.
fn sort_result(project: io::Result<bool>, suite: io::Result<bool>) -> io::Result<bool> {
    if !project? {
        return Ok(false);
    }
    suite
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

    /// Creates `checkout` and returns the parent that owns its children.
    fn project(service: &TestService<FileRepository>) -> Parent {
        service
            .create(
                Resource::Projects,
                &json!({ "projectId": "P-1", "name": "checkout" }),
            )
            .expect("project");
        Parent::Project("checkout.json".to_owned())
    }

    /// The parent of suite `smoke` inside project `checkout`.
    fn smoke() -> Parent {
        Parent::Suite {
            project: "checkout.json".to_owned(),
            suite: "smoke.json".to_owned(),
        }
    }

    /// Creates suite `smoke` inside `project` and returns its parent.
    fn add_suite(service: &TestService<FileRepository>, project: &Parent) -> Parent {
        service
            .create_in(
                Resource::Suites,
                project,
                &json!({ "suiteId": "S-1", "name": "smoke" }),
            )
            .expect("suite");
        smoke()
    }

    fn case(service: &TestService<FileRepository>, parent: &Parent, id: &str) {
        service
            .create_in(
                Resource::Cases,
                parent,
                &json!({ "testCaseId": id, "title": "T", "expectedResult": "E" }),
            )
            .expect("case");
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
                    configuration: None,
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
                    configuration: None,
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
                    configuration: None,
                    filter: None,
                    tags: Some("does-not-exist".to_owned()),
                },
            )
            .expect("tags");
        assert!(untagged.is_empty());
    }

    #[test]
    fn runs_are_listed_by_the_configuration_they_link() {
        let (service, _directory) = service();
        for (id, name) in [("R-1", "nightly"), ("R-2", "weekly"), ("R-3", "release")] {
            service
                .create(
                    Resource::Runs,
                    &json!({ "testRunId": id, "name": name, "timestamp": "1", "tags": ["ci"] }),
                )
                .expect("run");
        }
        for name in ["chrome-linux", "firefox-windows"] {
            service
                .create(Resource::Configurations, &json!({ "name": name }))
                .expect("configuration");
        }
        for (run, configuration) in [
            ("nightly.json", "chrome-linux.json"),
            ("weekly.json", "firefox-windows.json"),
        ] {
            service
                .link_configuration_to_run(run, &json!({ "configId": configuration }))
                .expect("link");
        }

        let query = |configuration: &str| ListQuery {
            configuration: Some(configuration.to_owned()),
            ..ListQuery::default()
        };

        assert_eq!(
            service
                .list(Resource::Runs, &query("chrome-linux.json"))
                .unwrap(),
            vec!["nightly.json"]
        );
        assert_eq!(
            service
                .list(Resource::Runs, &query("firefox-windows.json"))
                .unwrap(),
            vec!["weekly.json"]
        );

        // A configuration no run links yields an empty listing rather than an
        // error.
        assert_eq!(
            service
                .list(Resource::Runs, &query("firefox-linux.json"))
                .unwrap(),
            Vec::<String>::new()
        );

        // The configuration filter composes with the substring and tag filters.
        let composed = ListQuery {
            filter: Some("NIGHT".to_owned()),
            tags: Some(" ci ".to_owned()),
            configuration: Some("chrome-linux.json".to_owned()),
        };
        assert_eq!(
            service.list(Resource::Runs, &composed).unwrap(),
            vec!["nightly.json"]
        );

        // A run that links a different configuration drops out of the composed
        // listing even though it carries the same tag.
        let tag_only = ListQuery {
            tags: Some("ci".to_owned()),
            configuration: Some("chrome-linux.json".to_owned()),
            ..ListQuery::default()
        };
        assert_eq!(
            service.list(Resource::Runs, &tag_only).unwrap(),
            vec!["nightly.json"]
        );

        // The filter says nothing about other collections, which keeps their
        // listings intact rather than emptying them.
        assert_eq!(
            service
                .list(Resource::Configurations, &query("chrome-linux.json"))
                .unwrap(),
            vec!["chrome-linux.json", "firefox-windows.json"]
        );
    }

    #[test]
    fn reading_a_project_assembles_its_children() {
        let (service, _directory) = service();
        let project = project(&service);
        let suite = add_suite(&service, &project);
        case(&service, &suite, "TC-suite");
        case(&service, &project, "TC-direct");

        let document = service
            .get(Resource::Projects, "checkout.json")
            .expect("project");
        assert_eq!(document["testSuites"].as_array().map(Vec::len), Some(1));
        assert_eq!(document["testSuites"][0]["name"], "smoke");
        assert_eq!(
            document["testSuites"][0]["testCases"][0]["testCaseId"],
            "TC-suite"
        );
        assert_eq!(document["testCases"].as_array().map(Vec::len), Some(1));
        assert_eq!(document["testCases"][0]["testCaseId"], "TC-direct");

        let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
        assert_eq!(suite_document["testCases"][0]["testCaseId"], "TC-suite");
    }

    #[test]
    fn a_project_without_direct_cases_omits_the_test_cases_field() {
        let (service, _directory) = service();
        project(&service);
        let document = service
            .get(Resource::Projects, "checkout.json")
            .expect("project");
        assert!(document.get("testCases").is_none());
        assert_eq!(document["testSuites"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn markers_store_empty_child_arrays() {
        let (service, _directory) = service();
        let project = project(&service);
        service
            .create_in(
                Resource::Suites,
                &project,
                &json!({
                    "suiteId": "S-1",
                    "name": "smoke",
                    "testCases": [{
                        "testCaseId": "TC-ignored",
                        "title": "ignored",
                        "expectedResult": "ignored"
                    }]
                }),
            )
            .expect("suite");

        let suite = Parent::Suite {
            project: "checkout.json".to_owned(),
            suite: "smoke.json".to_owned(),
        };
        case(&service, &suite, "TC-real");

        let marker = service
            .repository
            .read_at(Resource::Suites, Some(&project), "smoke.json")
            .expect("marker");
        assert_eq!(
            marker["testCases"].as_array().map(Vec::len),
            Some(0),
            "the marker must not duplicate membership"
        );

        // A nested case in the payload is not materialised as a child either.
        assert_eq!(
            list(&service, Resource::Cases),
            vec!["TC-real".to_owned()],
            "only folders are children"
        );

        let project_marker = service
            .repository
            .read_at(Resource::Projects, None, "checkout.json")
            .expect("project marker");
        assert_eq!(
            project_marker["testSuites"].as_array().map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn an_identifier_in_several_parents_is_a_conflict() {
        let (service, _directory) = service();
        let project = project(&service);
        let suite = add_suite(&service, &project);
        case(&service, &project, "TC-1");
        case(&service, &suite, "TC-1");

        let error = service
            .get(Resource::Cases, "TC-1")
            .expect_err("ambiguous identifier");
        assert!(
            matches!(error, DomainError::Conflict(ref message) if message.contains("2 parents"))
        );
        assert_eq!(
            list(&service, Resource::Cases),
            vec!["TC-1".to_owned()],
            "lists de-duplicate rather than fail"
        );
        assert!(service.delete(Resource::Cases, "TC-1").is_err());
    }

    #[test]
    fn cases_join_suites_by_copy_and_leave_the_source_alone() {
        let (service, _directory) = service();
        let project = project(&service);
        add_suite(&service, &project);
        case(&service, &project, "TC-001");

        service
            .add_case_to_suite("smoke.json", &json!({ "testCaseId": "TC-001" }))
            .expect("copied into the suite");
        let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
        assert_eq!(
            suite_document["testCases"].as_array().map(Vec::len),
            Some(1)
        );

        let duplicate = service
            .add_case_to_suite("smoke.json", &json!({ "testCaseId": "TC-001" }))
            .expect_err("the suite already owns the case");
        assert!(matches!(duplicate, DomainError::Conflict(_)));

        let missing = service
            .add_case_to_suite("smoke.json", &json!({}))
            .expect_err("missing field");
        assert!(matches!(missing, DomainError::InvalidRequest { .. }));

        // The identifier now names two folders, so the global route refuses it.
        assert!(matches!(
            service
                .require_test_case("TC-001")
                .expect_err("two occurrences"),
            DomainError::Conflict(_)
        ));

        service
            .remove_case_from_suite("smoke.json", "TC-001")
            .expect("remove");
        let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
        assert_eq!(
            suite_document["testCases"].as_array().map(Vec::len),
            Some(0)
        );

        let project_document = service
            .get(Resource::Projects, "checkout.json")
            .expect("project");
        assert_eq!(
            project_document["testCases"].as_array().map(Vec::len),
            Some(1),
            "the copied case left the project's own case untouched"
        );
    }

    #[test]
    fn moving_a_case_leaves_it_with_one_home() {
        let (service, _directory) = service();
        let project = project(&service);
        let suite = add_suite(&service, &project);
        case(&service, &project, "TC-001");

        let composed = service
            .compose(
                Resource::Cases,
                &suite,
                &json!({ "testCaseId": "TC-001", "mode": "move" }),
            )
            .expect("move");
        assert_eq!(
            composed,
            Composed::Placed {
                id: "TC-001".to_owned(),
                mode: Placement::Move
            }
        );

        let document = service
            .get(Resource::Projects, "checkout.json")
            .expect("project");
        assert!(document.get("testCases").is_none(), "the old home lost it");
        let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
        assert_eq!(
            suite_document["testCases"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(list(&service, Resource::Cases), vec!["TC-001".to_owned()]);
    }

    #[test]
    fn a_suite_can_be_placed_into_another_project() {
        let (service, _directory) = service();
        let project = project(&service);
        add_suite(&service, &project);
        service
            .create(Resource::Projects, &json!({ "name": "other" }))
            .expect("other project");
        let other = Parent::Project("other.json".to_owned());

        service
            .compose(
                Resource::Suites,
                &other,
                &json!({ "suiteId": "smoke.json" }),
            )
            .expect("copy");

        assert_eq!(
            service
                .list_children(&other, Resource::Suites)
                .expect("children"),
            vec!["smoke.json".to_owned()]
        );
        assert_eq!(
            service
                .list_children(&project, Resource::Suites)
                .expect("children"),
            vec!["smoke.json".to_owned()],
            "a copy leaves the source in place"
        );
    }

    #[test]
    fn composition_grammar_rejects_mixed_and_unknown_modes() {
        let (service, _directory) = service();
        let project = project(&service);

        let mixed = service
            .compose(
                Resource::Suites,
                &project,
                &json!({ "name": "smoke", "mode": "copy" }),
            )
            .expect_err("mixed");
        assert!(matches!(mixed, DomainError::InvalidRequest { .. }));

        let unknown = service
            .compose(
                Resource::Suites,
                &project,
                &json!({ "suiteId": "smoke.json", "mode": "sideways" }),
            )
            .expect_err("unknown mode");
        assert!(matches!(unknown, DomainError::InvalidRequest { .. }));

        let unnamed = service
            .compose(Resource::Suites, &project, &json!({}))
            .expect_err("nothing to create or place");
        assert!(matches!(unnamed, DomainError::InvalidRequest { .. }));
    }

    #[test]
    fn creating_in_a_missing_parent_is_not_found() {
        let (service, _directory) = service();
        let missing = Parent::Project("ghost.json".to_owned());
        let error = service
            .create_in(
                Resource::Suites,
                &missing,
                &json!({ "suiteId": "S-1", "name": "smoke" }),
            )
            .expect_err("no project");
        assert!(matches!(error, DomainError::NotFound(ref m) if m == "Project not found"));
        assert!(list(&service, Resource::Suites).is_empty());
    }

    #[test]
    fn deleting_a_project_cascades_through_the_tree() {
        let (service, _directory) = service();
        let project = project(&service);
        let suite = add_suite(&service, &project);
        case(&service, &suite, "TC-suite");
        case(&service, &project, "TC-direct");

        service
            .delete(Resource::Projects, "checkout.json")
            .expect("cascade");

        assert!(list(&service, Resource::Projects).is_empty());
        assert!(list(&service, Resource::Suites).is_empty());
        assert!(list(&service, Resource::Cases).is_empty());
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
    fn a_duplicated_suite_stays_in_its_project() {
        let (service, _directory) = service();
        let project = project(&service);
        service
            .create_in(
                Resource::Suites,
                &project,
                &json!({ "suiteId": "S-1", "name": "smoke" }),
            )
            .expect("suite");

        let new_id = service
            .duplicate(
                &duplicate::SUITE,
                "smoke.json",
                &json!({ "newId": "S-2.json" }),
            )
            .expect("duplicate");
        assert_eq!(new_id, "S-2.json");
        assert_eq!(
            service
                .list_children(&project, Resource::Suites)
                .expect("children"),
            vec!["S-2.json".to_owned(), "smoke.json".to_owned()]
        );
    }

    #[test]
    fn duplicating_onto_an_existing_identifier_is_a_conflict() {
        let (service, _directory) = service();
        service
            .create(Resource::Projects, &json!({ "name": "checkout" }))
            .expect("source");
        service
            .create(Resource::Projects, &json!({ "name": "occupied" }))
            .expect("target");

        let error = service
            .duplicate(
                &duplicate::PROJECT,
                "checkout.json",
                &json!({ "newId": "occupied.json" }),
            )
            .expect_err("duplicate onto an occupied id");
        assert!(matches!(error, DomainError::Conflict(_)));

        let target = service
            .get(Resource::Projects, "occupied.json")
            .expect("target intact");
        assert_eq!(target["name"], "occupied");
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
    fn a_run_embeds_a_snapshot_of_the_case_it_selected() {
        let (service, _directory) = service();
        let project = project(&service);
        case(&service, &project, "TC-1");
        service
            .create(
                Resource::Runs,
                &json!({ "testRunId": "R-1", "name": "nightly", "timestamp": "1" }),
            )
            .expect("run");

        service
            .add_case_to_run("nightly.json", &json!({ "testCaseId": "TC-1" }))
            .expect("add");

        // Editing the source afterwards must not rewrite the recorded run.
        service
            .update(
                Resource::Cases,
                "TC-1",
                &json!({
                    "testCaseId": "TC-1",
                    "title": "changed",
                    "expectedResult": "changed"
                }),
            )
            .expect("update");

        let run = service.get(Resource::Runs, "nightly.json").expect("run");
        assert_eq!(run["testCases"][0]["title"], "T");
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
    fn attachments_are_recorded_in_the_case_and_round_trip() {
        let (service, _directory) = service();
        let project = project(&service);
        case(&service, &project, "TC-1");

        let parent = service.require_test_case("TC-1").expect("parent");
        let stored = service
            .store_attachment(&parent, "TC-1", "notes.txt", b"evidence")
            .expect("store");
        assert_eq!(stored.original_name, "notes.txt");
        assert_eq!(stored.size, 8);
        assert!(stored.filename.ends_with("-notes.txt"));

        assert_eq!(
            service
                .read_attachment(&parent, "TC-1", &stored.filename)
                .expect("read"),
            b"evidence"
        );

        let document = service.get(Resource::Cases, "TC-1").expect("case");
        assert_eq!(
            document["attachments"][0]["filename"],
            json!(stored.filename)
        );
        assert_eq!(document["attachments"][0]["originalName"], "notes.txt");
        assert_eq!(document["attachments"][0]["mimeType"], "text/plain");

        service
            .delete_attachment(&parent, "TC-1", &stored.filename)
            .expect("delete");
        assert!(
            service
                .read_attachment(&parent, "TC-1", &stored.filename)
                .is_err()
        );
        let document = service.get(Resource::Cases, "TC-1").expect("case");
        assert!(document.get("attachments").is_none());
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
