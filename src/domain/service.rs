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
    CaseHistoryEntry, CoverageReport, DefectLink, ImportCounts, ImportSummary, Milestone,
    MilestoneProgress, SummaryReport, TestCase, TestCaseResult, TestConfiguration, TestRun,
    TestSuite,
};
use crate::storage::{Parent, Placement, Repository, Resource, unique_suffix};

use super::duplicate::{self, DuplicateSpec};
use super::error::{self, DomainError};
use super::import::{self, ImportStatus, ParsedCase};
use super::{
    Created, ListQuery, MAX_ATTACHMENT_BYTES, StoredAttachment, composition,
    current_iso8601_timestamp, current_timestamp_string, defect, mime_type, progress, reports,
    required_string, resources, validation,
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

    /// Validates, names and stores a new document in the collection that has no
    /// parent of its own — projects.
    ///
    /// A run, a milestone and a configuration are stored inside a project, so
    /// they are created through [`Self::create_in`] with the project that owns
    /// them. The retired flat routes stay registered to name their replacement,
    /// exactly as the suite and case ones do since Issue #66.
    pub fn create(&self, resource: Resource, value: &Value) -> Result<Created, DomainError> {
        match resource {
            Resource::Suites => Err(DomainError::invalid_request(
                "Test suites are created inside a project: POST /projects/{id}/test_suites",
            )),
            Resource::Cases => Err(DomainError::invalid_request(
                "Test cases are created inside a project or a suite: POST /projects/{id}/test_cases or POST /test_suites/{id}/test_cases",
            )),
            Resource::Runs => Err(DomainError::invalid_request(
                "Test runs are created inside a project: POST /projects/{id}/test_runs",
            )),
            Resource::Milestones => Err(DomainError::invalid_request(
                "Milestones are created inside a project: POST /projects/{id}/milestones",
            )),
            Resource::Configurations => Err(DomainError::invalid_request(
                "Configurations are created inside a project: POST /projects/{id}/configurations",
            )),
            Resource::Projects => self.create_at(resource, None, value),
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
        let mut merged = merged_document(&stored, value)?;
        if resource == Resource::Cases {
            self.revise_case(parent.as_ref(), id, &stored, &mut merged)?;
        }
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

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        let suite = self.load_entity::<TestSuite>(Resource::Suites, &suite_id)?;
        composition::attach_suite_to_run(&mut run, &suite, &suite_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Adds the case named in `body` to a run, embedding a snapshot copy.
    pub fn add_case_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let test_case_id = required_string(body, "testCaseId")
            .ok_or_else(|| DomainError::invalid_request("Required field testCaseId is missing"))?;

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        let test_case = self.load_entity::<TestCase>(Resource::Cases, &test_case_id)?;
        composition::attach_case_to_run(&mut run, &test_case, &test_case_id)?;
        composition::capture_case_version(&mut run, &test_case.test_case_id, test_case.version);
        self.save(Resource::Runs, run_id, Some(&home), &run)
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

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        // The result may name a case the store does not hold — the API has
        // always accepted that — so the case is looked up best-effort: its
        // version is pinned when it can be read and the run falls back to
        // version 1 when it cannot.
        let held = self
            .load_entity::<TestCase>(Resource::Cases, &test_case_id)
            .ok();
        composition::capture_case_version(
            &mut run,
            held.as_ref()
                .map_or(&test_case_id, |case| &case.test_case_id),
            held.as_ref().and_then(|case| case.version),
        );
        let result = TestCaseResult {
            test_case_id,
            status,
            timestamp: required_string(body, "timestamp").unwrap_or_else(current_timestamp_string),
            notes: required_string(body, "notes"),
            duration_ms: body.get("durationMs").and_then(Value::as_u64),
            attachments: None,
            defect_links: None,
        };
        composition::upsert_result(&mut run, result);
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Lists the defects linked to one case's result in a run.
    ///
    /// A run that does not exist, or one that records no result for `case_id`,
    /// answers `404`: an empty list means "this result has no linked defect",
    /// which is a different answer from "there is no result to link to".
    pub fn list_defects(
        &self,
        run_id: &str,
        case_id: &str,
    ) -> Result<Vec<DefectLink>, DomainError> {
        let run = self.load::<TestRun>(Resource::Runs, run_id, None, "Test run not found")?;
        let result = run
            .results
            .as_ref()
            .and_then(|results| results.iter().find(|result| result.test_case_id == case_id))
            .ok_or_else(|| DomainError::NotFound("Test result not found in test run".to_owned()))?;
        Ok(result.defect_links.clone().unwrap_or_default())
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
        self.store_imported_results(run_id, report.cases, report.errors)
    }

    /// Imports a JSON result array into a run's results.
    ///
    /// Every entry must be usable: the whole body is parsed before anything is
    /// written, so a body that is malformed, names an unknown field, omits
    /// `testCaseId` or `status`, or carries a status that is not `Passed`,
    /// `Failed` or `Blocked` answers a `400` and leaves the run exactly as it
    /// was. Cases the run already records are counted as duplicates and left
    /// untouched, exactly as the JUnit importer does.
    pub fn import_json_results(
        &self,
        run_id: &str,
        body: &[u8],
    ) -> Result<ImportSummary, DomainError> {
        let cases = import::parse_json(body)?;
        self.store_imported_results(run_id, cases, 0)
    }

    /// Writes already-parsed import cases into a run, refusing to overwrite a
    /// case the run already records and reporting what was written.
    ///
    /// Both importers share this: they differ only in how they read a body and
    /// in whether a case they cannot use fails the request or is counted in
    /// `errors`.
    fn store_imported_results(
        &self,
        run_id: &str,
        cases: Vec<ParsedCase>,
        errors: usize,
    ) -> Result<ImportSummary, DomainError> {
        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
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

        for case in cases {
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
                    duration_ms: None,
                    attachments: None,
                    defect_links: None,
                },
            );
            imported += 1;
        }

        let outcome = ImportSummary {
            imported,
            skipped: duplicates + errors,
            errors,
            duplicates,
            summary,
        };
        self.save(Resource::Runs, run_id, Some(&home), &run)?;
        Ok(outcome)
    }

    /// Links the top-level configuration named in `body` to a run by reference,
    /// refusing one the run already references.
    ///
    /// A configuration owns its storage, so the run keeps a reference to it
    /// rather than a copy; the referenced configuration is verified to exist.
    /// A run may link a configuration from any project, and the run's own home
    /// is preferred, so the reference means that project's configuration first.
    pub fn link_configuration_to_run(&self, run_id: &str, body: &Value) -> Result<(), DomainError> {
        let config_id = required_string(body, "configId")
            .ok_or_else(|| DomainError::invalid_request("Required field configId is missing"))?;

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        let configuration = self.load::<TestConfiguration>(
            Resource::Configurations,
            &config_id,
            Some(&home),
            entity_missing_message(Resource::Configurations),
        )?;
        composition::attach_configuration_to_run(&mut run, &configuration, &config_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Removes a configuration reference from a run.
    ///
    /// The reference names a configuration, so it resolves the same way a link
    /// does: the run's home is preferred, one no project holds is absent, and
    /// one two projects hold is refused rather than guessed at.
    pub fn unlink_configuration_from_run(
        &self,
        run_id: &str,
        config_id: &str,
    ) -> Result<(), DomainError> {
        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        self.resolve(
            Resource::Configurations,
            config_id,
            Some(&home),
            entity_missing_message(Resource::Configurations),
        )?;
        composition::detach_configuration_from_run(&mut run, config_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    /// Links a defect to the result a run records for `case_id`.
    ///
    /// The client names where the defect lives; the API derives the link's own
    /// identifier and the moment it was made, and returns the built link. The
    /// link is returned for the caller to address rather than to serialise: the
    /// HTTP handler responds with the `CreateResponse` shape (`message` plus the
    /// derived `id`), not with this document, so a client learns the value it
    /// must use to unlink from `id` and reads the rest from the listing route.
    /// The URL is checked against the tracker it claims to belong to, and a
    /// defect the result already links is a conflict.
    pub fn link_defect_to_result(
        &self,
        run_id: &str,
        case_id: &str,
        body: &Value,
    ) -> Result<DefectLink, DomainError> {
        let request = defect::parse_request(body)?;
        let link = defect::new_link(
            request,
            format!("link-{}", unique_suffix()),
            current_timestamp_string(),
        );

        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        composition::attach_defect_to_result(&mut run, case_id, link.clone())?;
        self.save(Resource::Runs, run_id, Some(&home), &run)?;
        Ok(link)
    }

    /// Removes the defect link `link_id` from the result a run records for
    /// `case_id`; a link the result does not carry is a 404.
    pub fn unlink_defect_from_result(
        &self,
        run_id: &str,
        case_id: &str,
        link_id: &str,
    ) -> Result<(), DomainError> {
        let home = self.run_home(run_id)?;
        let mut run =
            self.load::<TestRun>(Resource::Runs, run_id, Some(&home), "Test run not found")?;
        composition::detach_defect_from_result(&mut run, case_id, link_id)?;
        self.save(Resource::Runs, run_id, Some(&home), &run)
    }

    // --- duplication ---------------------------------------------------

    /// Copies a document, applying the request-body overrides, and returns the
    /// identifier of the copy.
    ///
    /// A copy lands in the home the source belongs to, so it stays where the
    /// original is; only a project lives at the top level. Neither a duplicate
    /// nor a `PUT` can therefore move a document into another project.
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
    ///
    /// A reference means the milestone's own project first, so a run identifier
    /// another project happens to use too still names the intended run. One
    /// that no project holds is skipped and progress recomputes over the runs
    /// that remain; one that two or more hold outside the home is refused,
    /// because an arbitrary pick would report a wrong number.
    pub fn milestone_progress(&self, id: &str) -> Result<MilestoneProgress, DomainError> {
        let home = self.resolve(Resource::Milestones, id, None, "Milestone not found")?;
        let value = self
            .repository
            .read_at(Resource::Milestones, Some(&home), id)
            .map_err(error::milestone_error)?;
        let milestone: Milestone = serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored milestone JSON is invalid".to_owned()))?;

        let mut runs = Vec::new();
        for run_id in milestone.test_run_ids.as_deref().unwrap_or_default() {
            let run_home =
                match self.resolve(Resource::Runs, run_id, Some(&home), "Test run not found") {
                    Ok(run_home) => run_home,
                    Err(DomainError::NotFound(_)) => continue,
                    Err(error) => return Err(error),
                };
            let Ok(run_value) = self.read_document(
                Resource::Runs,
                Some(&run_home),
                run_id,
                "Test run not found",
            ) else {
                continue;
            };
            // A run that cannot be decoded is skipped rather than failing the
            // whole report; the format-version reader check is #98's.
            let Ok(run) = serde_json::from_value::<TestRun>(run_value) else {
                continue;
            };
            runs.push(run);
        }

        Ok(progress::compute(&milestone, &runs))
    }

    /// Reports how many cases the tree holds, per suite and in total, for the
    /// projects the caller asked for.
    ///
    /// The scope decides which projects are walked: [`reports::Scope::All`]
    /// walks every project, [`reports::Scope::Project`] verifies that one
    /// project exists and walks it, and [`reports::Scope::Projects`] walks the
    /// identifiers as given — its caller already filtered them by reachability,
    /// so a project that has since been deleted contributes nothing rather than
    /// failing the report.
    pub fn coverage_report(&self, scope: reports::Scope) -> Result<CoverageReport, DomainError> {
        let (echo, projects): (Option<String>, Vec<String>) = match scope {
            reports::Scope::All => (
                None,
                self.repository
                    .list(Resource::Projects)
                    .map_err(error::read_error)?,
            ),
            reports::Scope::Project(id) => {
                self.require_parent(&Parent::Project(id.clone()))?;
                (Some(id.clone()), vec![id])
            }
            reports::Scope::Projects(ids) => (None, ids),
        };

        let mut project_cases = Vec::with_capacity(projects.len());
        for project in projects {
            let parent = Parent::Project(project);
            let direct = self
                .repository
                .list_children(&parent, Resource::Cases)
                .map_err(error::read_error)?
                .len();
            let suite_ids = self
                .repository
                .list_children(&parent, Resource::Suites)
                .map_err(error::read_error)?;
            let mut suites = Vec::with_capacity(suite_ids.len());
            for suite_id in suite_ids {
                let document = self
                    .repository
                    .read_at(Resource::Suites, Some(&parent), &suite_id)
                    .map_err(error::read_error)?;
                // A suite's marker records its own name; fall back to the
                // identifier when a legacy document omits it.
                let name = document
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&suite_id)
                    .to_owned();
                let suite = Parent::Suite {
                    project: parent.project().to_owned(),
                    suite: suite_id.clone(),
                };
                let case_count = self
                    .repository
                    .list_children(&suite, Resource::Cases)
                    .map_err(error::read_error)?
                    .len();
                suites.push(reports::SuiteCases {
                    suite_id,
                    name,
                    case_count,
                });
            }
            project_cases.push(reports::ProjectCases { direct, suites });
        }

        Ok(reports::coverage(echo.as_deref(), project_cases))
    }

    /// Reports how the results recorded across the runs in scope split by
    /// status, together with their pass rate and total duration.
    ///
    /// Every filter is optional and the ones supplied combine: a run is only
    /// counted when it satisfies all of them. Runs that cannot be read or
    /// decoded are skipped rather than failing the whole report, matching how
    /// [`Self::milestone_progress`] treats its references.
    ///
    /// `reachable`, when set, is the authorisation filter: a run that names a
    /// project outside it is skipped even though it would otherwise be in scope.
    /// A trusted caller passes `None` and sees every run.
    pub fn summary_report(
        &self,
        filters: &reports::SummaryFilters,
        reachable: Option<&[String]>,
    ) -> Result<SummaryReport, DomainError> {
        let mut filters = filters.clone();
        if let Some(project_id) = filters.project_id.as_deref() {
            self.require_parent(&Parent::Project(project_id.to_owned()))?;
        }

        let milestone_runs = match filters.milestone_id.as_deref() {
            Some(id) => {
                // A filter names no home to prefer, so the milestone resolves
                // globally exactly as the route that reads it does.
                let home = self.resolve(Resource::Milestones, id, None, "Milestone not found")?;
                let value = self
                    .repository
                    .read_at(Resource::Milestones, Some(&home), id)
                    .map_err(error::milestone_error)?;
                let milestone: Milestone = serde_json::from_value(value).map_err(|_| {
                    DomainError::Internal("Stored milestone JSON is invalid".to_owned())
                })?;
                Some(milestone.test_run_ids.unwrap_or_default())
            }
            None => None,
        };

        if let Some(config_id) = filters.configuration_id.as_deref() {
            // A filter value is not a dereference, so an identifier two projects
            // hold is not a conflict here; one none holds is still absent.
            match self.repository.locate(Resource::Configurations, config_id) {
                Ok(homes) if !homes.is_empty() => {}
                Ok(_) => {
                    return Err(DomainError::NotFound(
                        entity_missing_message(Resource::Configurations).to_owned(),
                    ));
                }
                Err(error) => return Err(error::delete_error(error)),
            }
        }

        if let Some(raw) = std::mem::take(&mut filters.from) {
            filters.from = Some(reports::parse_date_filter(&raw)?);
        }
        if let Some(raw) = std::mem::take(&mut filters.to) {
            filters.to = Some(reports::parse_date_filter(&raw)?);
        }

        let mut results = Vec::new();
        // A global listing de-duplicates, so two runs sharing an identifier
        // would be counted once and the report would under-report. The walk is
        // per project instead, reading each run from the home that owns it.
        for project in self
            .repository
            .list(Resource::Projects)
            .map_err(error::read_error)?
        {
            let home = Parent::Project(project);
            let run_ids = self
                .repository
                .list_children(&home, Resource::Runs)
                .map_err(error::read_error)?;
            for run_id in run_ids {
                let Ok(value) = self
                    .repository
                    .read_at(Resource::Runs, Some(&home), &run_id)
                else {
                    continue;
                };
                let Ok(run) = serde_json::from_value::<TestRun>(value) else {
                    continue;
                };
                if let Some(reachable) = reachable
                    && !reports::run_reachable(&run, home.project(), reachable)
                {
                    continue;
                }
                if !reports::run_is_in_scope(
                    &run,
                    home.project(),
                    &run_id,
                    &filters,
                    milestone_runs.as_deref(),
                ) {
                    continue;
                }
                results.extend(run.results.unwrap_or_default());
            }
        }

        Ok(reports::summary(&results))
    }

    // --- authorization scope -------------------------------------------

    /// The project an entity addressed by `id` belongs to.
    ///
    /// Every resource but a project lives inside one, so this names the home of
    /// a suite, a case, a run, a milestone or a configuration alike.
    ///
    /// The authorization guard runs before the handler acts, so it must be able
    /// to name the project without reading the document the request will read
    /// next; this is the same resolution [`Self::update`] and [`Self::delete`]
    /// perform, exposed for that one caller.
    ///
    /// # Errors
    ///
    /// [`DomainError::NotFound`] when nothing has that identifier, and a
    /// conflict when more than one occurrence does, exactly as the routes that
    /// act on the identifier answer.
    pub fn project_of(
        &self,
        resource: Resource,
        id: &str,
        missing: &str,
    ) -> Result<String, DomainError> {
        Ok(self.parent_of(resource, id, missing)?.project().to_owned())
    }

    /// Reads a document as stored, without assembling its children.
    ///
    /// Used where a route needs a resource's own fields — the projects a run or
    /// a milestone references — and not the tree below it. The home is resolved
    /// the same way a `GET` resolves it, so an identifier two projects hold is
    /// a conflict here too.
    pub fn document(&self, resource: Resource, id: &str) -> Result<Value, DomainError> {
        let parent = self.owner_for_write(resource, id, entity_missing_message(resource))?;
        self.repository
            .read_at(resource, parent.as_ref(), id)
            .map_err(|error| error::load_error(error, entity_missing_message(resource)))
    }

    /// Reads the occurrence stored in `parent`, without resolving globally.
    ///
    /// The caller named the home, so nothing about the request is ambiguous: an
    /// identifier two projects hold is read from this one and never answers a
    /// conflict, and one this parent does not hold is simply absent, reported
    /// under `missing`. This is the home branch of the rule [`Self::document`]
    /// applies globally, exposed for the authorization guard, which knows the
    /// acting document's home before it dereferences a reference inside it.
    pub fn document_in(
        &self,
        resource: Resource,
        parent: &Parent,
        id: &str,
        missing: &str,
    ) -> Result<Value, DomainError> {
        self.read_document(resource, Some(parent), id, missing)
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
        let mut document = value.clone();
        if resource == Resource::Cases {
            stamp_case_creation(&mut document);
        }
        self.write_marker(resource, parent, &id, &document)?;
        Ok(Created { id })
    }

    /// Applies the test-case version rules to a merged `PUT` document.
    ///
    /// A change to a qualifying field snapshots the pre-update document under
    /// `revisions/` and starts a new version; any other update writes the
    /// document without touching the history. The server owns `version` and
    /// `lastModified`, so a value the client supplied is always discarded —
    /// a legacy document that never carried them keeps its on-disk shape.
    fn revise_case(
        &self,
        parent: Option<&Parent>,
        id: &str,
        stored: &Value,
        merged: &mut Value,
    ) -> Result<(), DomainError> {
        let Some(parent) = parent else {
            return Err(DomainError::Internal(
                "A test case update needs a parent folder".to_owned(),
            ));
        };
        let current = stored.get("version").and_then(Value::as_u64).unwrap_or(1);
        let stored_last_modified = stored.get("lastModified").cloned();
        let changed = case_content_changed(stored, merged);
        let Some(object) = merged.as_object_mut() else {
            return Err(DomainError::Internal("Stored JSON is invalid".to_owned()));
        };
        if changed {
            self.repository
                .save_revision(parent, id, current, stored)
                .map_err(DomainError::from)?;
            object.insert("version".to_owned(), Value::from(current + 1));
            object.insert(
                "lastModified".to_owned(),
                Value::String(current_iso8601_timestamp()),
            );
        } else {
            match stored_last_modified {
                Some(value) => {
                    object.insert("lastModified".to_owned(), value);
                }
                None => {
                    object.remove("lastModified");
                }
            }
            if stored.get("version").is_some() {
                object.insert("version".to_owned(), Value::from(current));
            } else {
                object.remove("version");
            }
        }
        Ok(())
    }

    // --- case history ---

    /// Lists a case's recorded revisions, oldest first, each with the
    /// qualifying fields the update after it changed. The current live
    /// document is not a snapshot and is not listed; a case that has never
    /// had a qualifying update has an empty history.
    pub fn list_case_history(
        &self,
        parent: &Parent,
        id: &str,
    ) -> Result<Vec<CaseHistoryEntry>, DomainError> {
        let versions = self
            .repository
            .list_revisions(parent, id)
            .map_err(error::read_error)?;
        if versions.is_empty() {
            return Ok(Vec::new());
        }
        let live = self.read_document(Resource::Cases, Some(parent), id, "Test case not found")?;
        let mut snapshots = Vec::with_capacity(versions.len());
        for version in &versions {
            snapshots.push(
                self.repository
                    .read_revision(parent, id, *version)
                    .map_err(|error| error::document_error(error, "Revision not found"))?,
            );
        }
        let mut history = Vec::with_capacity(snapshots.len());
        for (index, version) in versions.iter().enumerate() {
            let snapshot = &snapshots[index];
            let successor = snapshots.get(index + 1).unwrap_or(&live);
            history.push(CaseHistoryEntry {
                version: *version,
                last_modified: snapshot
                    .get("lastModified")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                changed_fields: QUALIFYING_CASE_FIELDS
                    .iter()
                    .filter(|field| snapshot.get(*field) != successor.get(*field))
                    .map(|field| (*field).to_owned())
                    .collect(),
            });
        }
        Ok(history)
    }

    /// Reads the immutable snapshot a case recorded at `version`.
    pub fn read_case_revision(
        &self,
        parent: &Parent,
        id: &str,
        version: u64,
    ) -> Result<Value, DomainError> {
        self.repository
            .read_revision(parent, id, version)
            .map_err(|error| error::document_error(error, "Revision not found"))
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
            // A run, a milestone and a configuration are read from the project
            // that owns them, so a bare identifier two projects hold is
            // ambiguous exactly as it already is for a suite or a case. A `GET`
            // names no home to prefer, so the identifier resolves globally.
            Resource::Runs | Resource::Milestones | Resource::Configurations => {
                let parent = self.parent_of(resource, id, missing)?;
                self.read_document(resource, Some(&parent), id, missing)
            }
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
            Resource::Projects => self.repository.read_at(resource, None, id),
            _ => {
                let home = self
                    .repository
                    .locate(resource, id)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "resource not found"))?;
                self.repository.read_at(resource, Some(&home), id)
            }
        }
    }

    /// The home an identifier names, preferring the one `home` points at.
    ///
    /// One rule resolves every reference and every bare identifier, so a
    /// document that names another means its own project first: a home holding
    /// the identifier wins, and otherwise the identifier resolves globally.
    /// Nothing owns it is a 404; one owner is that occurrence; two or more is a
    /// conflict, because a bare identifier cannot say which parent was meant
    /// and guessing would silently act on the wrong document.
    fn resolve(
        &self,
        resource: Resource,
        id: &str,
        home: Option<&Parent>,
        missing: &str,
    ) -> Result<Parent, DomainError> {
        if let Some(home) = home
            && self
                .repository
                .exists_at(resource, Some(home), id)
                .map_err(error::read_error)?
        {
            return Ok(home.clone());
        }
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

    /// The parent that owns the single occurrence of `id`, for the globally
    /// addressed routes, which name no home to prefer.
    fn parent_of(
        &self,
        resource: Resource,
        id: &str,
        missing: &str,
    ) -> Result<Parent, DomainError> {
        self.resolve(resource, id, None, missing)
    }

    /// The project a run addressed by a bare identifier lives in.
    ///
    /// Every run sub-route resolves the home once, before it reads, and passes
    /// it to both the read and the write: the sub-route's own references prefer
    /// that home, and the write goes back to the occurrence the read came from.
    fn run_home(&self, run_id: &str) -> Result<Parent, DomainError> {
        self.resolve(Resource::Runs, run_id, None, "Test run not found")
    }

    /// The parent a write addresses, for resources that live inside one.
    ///
    /// Only projects are addressed without a parent; every other resource is
    /// stored in the parent that owns it, and an identifier several parents own
    /// is refused by the endpoint hint [`ambiguous`] answers with.
    fn owner_for_write(
        &self,
        resource: Resource,
        id: &str,
        missing: &str,
    ) -> Result<Option<Parent>, DomainError> {
        match resource {
            Resource::Projects => Ok(None),
            _ => self.parent_of(resource, id, missing).map(Some),
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

    /// Loads a stored document as its typed model, resolving its home the way a
    /// `GET` would and preferring `home` when the caller already knows it.
    fn load<T: DeserializeOwned>(
        &self,
        resource: Resource,
        id: &str,
        home: Option<&Parent>,
        missing_message: &str,
    ) -> Result<T, DomainError> {
        let parent = self.resolve(resource, id, home, missing_message)?;
        let value = self.read_document(resource, Some(&parent), id, missing_message)?;
        serde_json::from_value(value)
            .map_err(|_| DomainError::Internal("Stored JSON is invalid".to_owned()))
    }

    /// Writes a document back to `parent`, the home its caller resolved.
    ///
    /// A write never moves a document: `parent` is the home the matching read
    /// resolved, so an update addresses the same occurrence it loaded.
    fn save<T: Serialize>(
        &self,
        resource: Resource,
        id: &str,
        parent: Option<&Parent>,
        value: &T,
    ) -> Result<(), DomainError> {
        let document = serde_json::to_value(value)
            .map_err(|_| DomainError::Internal("Failed to serialize document".to_owned()))?;
        // A document is written back to the parent that owns it, which for a
        // run is the project it was created in.
        let parent = match parent {
            Some(parent) => Some(parent.clone()),
            None => self.owner_for_write(resource, id, entity_missing_message(resource))?,
        };
        self.repository
            .write_at(resource, parent.as_ref(), id, &document)
            .map_err(DomainError::from)
    }
}

/// The fields whose change starts a new test-case revision. The set is narrow on
/// purpose: history tracks the executable content of a case, not incidental
/// metadata churn.
const QUALIFYING_CASE_FIELDS: [&str; 4] = ["title", "steps", "preconditions", "expectedResult"];

/// Whether a merged update changed any qualifying field of a test case.
fn case_content_changed(stored: &Value, merged: &Value) -> bool {
    QUALIFYING_CASE_FIELDS
        .iter()
        .any(|field| stored.get(field) != merged.get(field))
}

/// Stamps the version bookkeeping a new test case starts with. The server owns
/// both fields, so a value the client supplied is overwritten: a fresh case
/// always begins its history at version 1.
fn stamp_case_creation(document: &mut Value) {
    let Some(object) = document.as_object_mut() else {
        return;
    };
    object.insert("version".to_owned(), Value::from(1u64));
    object.insert(
        "lastModified".to_owned(),
        Value::String(current_iso8601_timestamp()),
    );
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
///
/// The message names the parent-scoped routes that address one occurrence
/// directly, so the caller learns how to say which parent it meant.
fn ambiguous(resource: Resource, homes: &[Parent]) -> DomainError {
    let endpoints = match resource {
        Resource::Cases => {
            "POST /projects/{id}/test_cases, POST /test_suites/{id}/test_cases, or the matching /{case_id} delete"
        }
        Resource::Runs => "POST /projects/{id}/test_runs, or the matching /{run_id} delete",
        Resource::Milestones => {
            "POST /projects/{id}/milestones, or the matching /{milestone_id} delete"
        }
        Resource::Configurations => {
            "POST /projects/{id}/configurations, or the matching /{config_id} delete"
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
        let home = project(&service);
        for (id, name) in [("R-1", "nightly"), ("R-2", "weekly"), ("R-3", "release")] {
            service
                .create_in(
                    Resource::Runs,
                    &home,
                    &json!({ "testRunId": id, "name": name, "timestamp": "1", "tags": ["ci"] }),
                )
                .expect("run");
        }
        for name in ["chrome-linux", "firefox-windows"] {
            service
                .create_in(Resource::Configurations, &home, &json!({ "name": name }))
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
        let home = project(&service);
        service
            .create_in(
                Resource::Runs,
                &home,
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
            .create_in(
                Resource::Runs,
                &project,
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
        let home = project(&service);
        service
            .create_in(
                Resource::Runs,
                &home,
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
            .create_in(
                Resource::Milestones,
                &home,
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

    // --- project homes -------------------------------------------------

    /// Creates a project named `name` beside `checkout` and returns its parent.
    fn another_project(service: &TestService<FileRepository>, name: &str) -> Parent {
        service
            .create(Resource::Projects, &json!({ "name": name }))
            .expect("project");
        Parent::Project(format!("{name}.json"))
    }

    /// Stores a run called `name` in `home`, recording one passed result.
    fn run_in(service: &TestService<FileRepository>, home: &Parent, name: &str) {
        service
            .create_in(
                Resource::Runs,
                home,
                &json!({
                    "name": name,
                    "timestamp": "1",
                    "results": [{ "testCaseId": "TC-1", "status": "Passed", "timestamp": "1" }]
                }),
            )
            .expect("run");
    }

    #[test]
    fn a_home_preferred_identifier_resolves_the_occurrence_it_names() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        run_in(&service, &checkout, "nightly");
        run_in(&service, &billing, "nightly");

        // A bare identifier resolves globally, so two occurrences are refused.
        let error = service
            .resolve(Resource::Runs, "nightly.json", None, "Test run not found")
            .expect_err("two homes");
        assert_eq!(
            error.to_string(),
            ambiguous(Resource::Runs, &[checkout.clone(), billing.clone()]).to_string(),
            "the global rule must not pick one of the two"
        );

        // Either home says which occurrence is meant.
        assert_eq!(
            service
                .resolve(
                    Resource::Runs,
                    "nightly.json",
                    Some(&billing),
                    "Test run not found"
                )
                .expect("billing's run"),
            billing
        );
        assert_eq!(
            service
                .resolve(
                    Resource::Runs,
                    "nightly.json",
                    Some(&checkout),
                    "Test run not found"
                )
                .expect("checkout's run"),
            checkout
        );

        // A home that does not hold the identifier falls back to the global
        // rule, so a unique identifier still resolves from an unrelated home.
        run_in(&service, &checkout, "weekly");
        assert_eq!(
            service
                .resolve(
                    Resource::Runs,
                    "weekly.json",
                    Some(&billing),
                    "Test run not found"
                )
                .expect("one home only"),
            checkout
        );
    }

    #[test]
    fn an_ambiguous_identifier_names_both_homes() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        run_in(&service, &checkout, "nightly");
        run_in(&service, &billing, "nightly");

        let error = service
            .get(Resource::Runs, "nightly.json")
            .expect_err("ambiguous");
        match error {
            DomainError::Conflict(message) => {
                assert!(message.contains("2 parents"), "{message}");
                assert!(message.contains("billing.json"), "{message}");
                assert!(message.contains("checkout.json"), "{message}");
                assert!(
                    message.contains("POST /projects/{id}/test_runs"),
                    "the conflict must name the parent-scoped route: {message}"
                );
            }
            other => panic!("expected a conflict, got {other:?}"),
        }

        // A write is refused too, rather than landing in an arbitrary home.
        assert!(matches!(
            service
                .record_run_result(
                    "nightly.json",
                    &json!({ "testCaseId": "TC-9", "status": "Passed" })
                )
                .expect_err("ambiguous"),
            DomainError::Conflict(_)
        ));
    }

    #[test]
    fn the_conflict_names_the_parent_scoped_routes_of_its_own_resource() {
        let homes = [
            Parent::Project("billing.json".to_owned()),
            Parent::Project("checkout.json".to_owned()),
        ];
        for (resource, endpoint) in [
            (
                Resource::Runs,
                "POST /projects/{id}/test_runs, or the matching /{run_id} delete",
            ),
            (
                Resource::Milestones,
                "POST /projects/{id}/milestones, or the matching /{milestone_id} delete",
            ),
            (
                Resource::Configurations,
                "POST /projects/{id}/configurations, or the matching /{config_id} delete",
            ),
            (
                Resource::Suites,
                "POST /projects/{id}/test_suites, or the matching /{suite_id} delete",
            ),
            (
                Resource::Cases,
                "POST /projects/{id}/test_cases, POST /test_suites/{id}/test_cases, or the matching /{case_id} delete",
            ),
        ] {
            let message = ambiguous(resource, &homes).to_string();
            assert!(message.contains(endpoint), "{resource:?}: {message}");
            assert!(
                message.contains("2 parents (billing.json, checkout.json)"),
                "{resource:?}: {message}"
            );
        }
    }

    #[test]
    fn the_retired_flat_creates_name_their_replacement() {
        let (service, _directory) = service();
        project(&service);

        for (resource, body, message) in [
            (
                Resource::Runs,
                json!({ "name": "nightly", "timestamp": "1" }),
                "Test runs are created inside a project: POST /projects/{id}/test_runs",
            ),
            (
                Resource::Milestones,
                json!({ "name": "v1.0" }),
                "Milestones are created inside a project: POST /projects/{id}/milestones",
            ),
            (
                Resource::Configurations,
                json!({ "name": "chrome" }),
                "Configurations are created inside a project: POST /projects/{id}/configurations",
            ),
        ] {
            let error = service.create(resource, &body).expect_err("retired route");
            match error {
                DomainError::InvalidRequest { code, message: got } => {
                    assert_eq!(code, "invalid_request", "{resource:?}");
                    assert_eq!(got, message, "{resource:?}");
                }
                other => panic!("{resource:?} produced {other:?}"),
            }
            assert!(
                list(&service, resource).is_empty(),
                "{resource:?}: a retired route must store nothing"
            );
        }
    }

    #[test]
    fn an_identifier_no_project_holds_is_not_found() {
        let (service, _directory) = service();
        project(&service);

        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            let error = service
                .resolve(
                    resource,
                    "missing.json",
                    None,
                    entity_missing_message(resource),
                )
                .expect_err("nothing holds it");
            assert!(
                matches!(&error, DomainError::NotFound(message)
                    if message == entity_missing_message(resource)),
                "{resource:?} produced {error:?}"
            );
        }

        assert!(matches!(
            service
                .milestone_progress("missing.json")
                .expect_err("missing"),
            DomainError::NotFound(message) if message == "Milestone not found"
        ));
    }

    #[test]
    fn duplicating_a_run_stores_the_copy_in_its_own_home() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        run_in(&service, &billing, "nightly");

        let new_id = service
            .duplicate(&duplicate::RUN, "nightly.json", &json!({}))
            .expect("duplicate");

        let copies = service
            .list_children(&billing, Resource::Runs)
            .expect("billing's runs");
        assert_eq!(copies.len(), 2, "{copies:?}");
        assert!(copies.contains(&new_id), "{copies:?}");
        assert!(
            service
                .list_children(&checkout, Resource::Runs)
                .expect("checkout's runs")
                .is_empty(),
            "a duplicate must not move or copy into another project"
        );
    }

    #[test]
    fn the_summary_report_counts_two_runs_sharing_an_identifier() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        run_in(&service, &checkout, "nightly");
        run_in(&service, &billing, "nightly");

        // A global listing de-duplicates to one `nightly.json`; the report
        // walks projects instead, so both runs contribute their result.
        assert_eq!(
            list(&service, Resource::Runs),
            vec!["nightly.json".to_owned()],
            "the listing still de-duplicates"
        );

        let report = service
            .summary_report(&reports::SummaryFilters::default(), None)
            .expect("report");
        assert_eq!(report.total, 2, "one result per project's run");
        assert_eq!(report.passed, 2);
    }

    #[test]
    fn milestone_progress_prefers_its_own_home_for_a_shared_identifier() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        // The milestone's home holds no run of this identifier, and two other
        // projects do, so the reference cannot be resolved.
        run_in(&service, &checkout, "nightly");
        run_in(&service, &billing, "nightly");
        let platform = another_project(&service, "platform");
        service
            .create_in(
                Resource::Milestones,
                &platform,
                &json!({ "name": "v1.0", "testRunIds": ["nightly.json"] }),
            )
            .expect("milestone");

        let error = service
            .milestone_progress("v1.0.json")
            .expect_err("two runs answer to the reference");
        assert!(
            matches!(&error, DomainError::Conflict(_)),
            "an arbitrary pick would report a wrong number, got {error:?}"
        );
    }

    #[test]
    fn milestone_progress_resolves_a_shared_identifier_from_its_home() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        run_in(&service, &checkout, "nightly");
        run_in(&service, &billing, "nightly");
        // The milestone lives in `billing`, so its reference means that run
        // even though `checkout` holds the same identifier.
        service
            .create_in(
                Resource::Milestones,
                &billing,
                &json!({ "name": "v1.0", "testRunIds": ["nightly.json"] }),
            )
            .expect("milestone");

        let progress = service.milestone_progress("v1.0.json").expect("progress");
        assert_eq!(progress.total_cases, 1);
        assert_eq!(progress.passed, 1);
    }

    #[test]
    fn milestone_progress_skips_a_reference_no_project_holds() {
        let (service, _directory) = service();
        let checkout = project(&service);
        run_in(&service, &checkout, "nightly");
        service
            .create_in(
                Resource::Milestones,
                &checkout,
                &json!({
                    "name": "v1.0",
                    "testRunIds": ["nightly.json", "deleted.json"]
                }),
            )
            .expect("milestone");

        let progress = service.milestone_progress("v1.0.json").expect("progress");
        assert_eq!(
            progress.total_cases, 1,
            "progress recomputes over the runs that still exist"
        );
    }

    #[test]
    fn a_run_links_the_configuration_its_own_home_holds() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        for home in [&checkout, &billing] {
            service
                .create_in(Resource::Configurations, home, &json!({ "name": "chrome" }))
                .expect("configuration");
        }
        run_in(&service, &billing, "nightly");

        // The identifier is ambiguous globally, but the run's home decides it.
        assert!(matches!(
            service
                .document(Resource::Configurations, "chrome.json")
                .expect_err("two homes"),
            DomainError::Conflict(_)
        ));
        service
            .link_configuration_to_run("nightly.json", &json!({ "configId": "chrome.json" }))
            .expect("the run's own configuration");

        let run = service.get(Resource::Runs, "nightly.json").expect("run");
        assert_eq!(run["configurations"][0]["configId"], "chrome.json");

        service
            .unlink_configuration_from_run("nightly.json", "chrome.json")
            .expect("unlink");
        let run = service.get(Resource::Runs, "nightly.json").expect("run");
        assert!(run["configurations"].as_array().is_none_or(Vec::is_empty));
    }

    #[test]
    fn a_write_goes_back_to_the_home_the_read_resolved() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        run_in(&service, &billing, "nightly");

        service
            .record_run_result(
                "nightly.json",
                &json!({ "testCaseId": "TC-9", "status": "Failed" }),
            )
            .expect("record");

        assert_eq!(
            service
                .list_children(&billing, Resource::Runs)
                .expect("billing's runs"),
            vec!["nightly.json".to_owned()],
            "the write must not create a second occurrence elsewhere"
        );
        assert!(
            service
                .list_children(&checkout, Resource::Runs)
                .expect("checkout's runs")
                .is_empty()
        );

        let run = service.get(Resource::Runs, "nightly.json").expect("run");
        assert_eq!(run["results"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn a_parent_addressed_read_never_answers_a_conflict() {
        let (service, _directory) = service();
        let checkout = project(&service);
        let billing = another_project(&service, "billing");
        for (home, marker) in [(&checkout, "in-checkout"), (&billing, "in-billing")] {
            service
                .create_in(
                    Resource::Runs,
                    home,
                    &json!({
                        "name": "nightly",
                        "testRunId": marker,
                        "timestamp": "1"
                    }),
                )
                .expect("run");
        }

        // The global read cannot choose, but a named home can.
        assert!(matches!(
            service
                .document(Resource::Runs, "nightly.json")
                .expect_err("two homes"),
            DomainError::Conflict(_)
        ));
        for (home, marker) in [(&checkout, "in-checkout"), (&billing, "in-billing")] {
            let document = service
                .document_in(Resource::Runs, home, "nightly.json", "Test run not found")
                .expect("the named home's occurrence");
            assert_eq!(document["testRunId"], marker, "{home:?}");
        }

        // A home that does not hold the identifier reports the caller's own
        // missing message, and never a conflict.
        let platform = another_project(&service, "platform");
        let error = service
            .document_in(
                Resource::Runs,
                &platform,
                "nightly.json",
                "Test run not found",
            )
            .expect_err("this home does not hold it");
        assert!(
            matches!(&error, DomainError::NotFound(message) if message == "Test run not found"),
            "{error:?}"
        );

        // So does an identifier nothing holds anywhere.
        assert!(matches!(
            service
                .document_in(Resource::Runs, &billing, "ghost.json", "Test run not found")
                .expect_err("absent"),
            DomainError::NotFound(_)
        ));
    }
}
