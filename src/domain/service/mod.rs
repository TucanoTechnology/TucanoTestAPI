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
    MilestoneProgress, SummaryReport, TestCase, TestConfiguration, TestRun, TestSuite,
};
use crate::storage::{
    MAX_COMPONENT_BYTES, Parent, Placement, Repository, Resource, StorageProbe, unique_suffix,
};

use super::duplicate::{self, DuplicateSpec};
use super::error::{self, DomainError};
use super::import::{self, ImportStatus, ParsedCase};
use super::{
    Created, ListQuery, MAX_ATTACHMENT_BYTES, StoredAttachment, current_iso8601_timestamp,
    current_timestamp_string, defect, mime_type, progress, reports, required_string, resources,
    validation,
};
use audit::{ATTACHMENT_RESOURCE, audited, placement_action, resource_noun};

pub use audit::AUDIT_TARGET;

// The `TestService` implementation is grouped by responsibility; each sub-module
// holds one cohesive slice of the inherent methods and nothing else.
mod attachments;
mod audit;
mod composition;
mod crud;
mod duplication;
mod history;
mod metadata;
mod reporting;

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

    /// The storage backend this service reads from and writes to.
    pub fn repository(&self) -> &R {
        &self.repository
    }

    /// Compute the ETag for the stored document addressed by `resource` and `id`.
    ///
    /// Returns `None` when the resource cannot be resolved (missing, ambiguous)
    /// or the raw bytes cannot be read, which the handler treats as "no ETag"
    /// rather than as an error — the GET still succeeds, just without the
    /// header.
    pub fn etag(&self, resource: Resource, id: &str) -> Option<String> {
        let parent = self.owner_for_write(resource, id, "").ok()?;
        let raw = self
            .repository
            .read_raw_at(resource, parent.as_ref(), id)
            .ok()?;
        Some(crate::storage::compute_etag(&raw))
    }

    // --- operations ----------------------------------------------------

    /// Reports what a readiness or diagnostics probe can learn about the store.
    ///
    /// The storage boundary is only reachable through this service, so this is
    /// a passthrough with no rule of its own: what counts as ready is a
    /// property of the store, not a judgement the domain makes.
    pub fn probe_storage(&self) -> StorageProbe {
        self.repository.probe_readiness()
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
        audited(resource_noun(resource), "create", &id, || {
            if self.repository.exists_at(resource, parent, &id)? {
                return Err(DomainError::Conflict("Resource already exists".to_owned()));
            }
            let mut document = value.clone();
            if resource == Resource::Cases {
                stamp_case_creation(&mut document);
            }
            self.write_marker(resource, parent, &id, &document)?;
            Ok(Created { id: id.clone() })
        })
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
            // The caller (transform_at) already holds the advisory lock, so
            // the revision snapshot is written through the unlocked variant to
            // avoid deadlocking on a second lock_exclusive() call.
            self.repository
                .save_revision_locked(parent, id, current, stored)
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
        audited(resource_noun(resource), placement_action(mode), &id, || {
            self.repository
                .place(resource, &source, &id, target, mode)
                .map_err(placement_error)
        })?;

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
/// timestamp. A value the client supplied is never overwritten, except for the
/// configuration identity: an id is resolved to the file that holds it, so a
/// stored `configId` that disagreed with its own document would name nothing,
/// and the identity is derived from the name on every write instead (Issue
/// #288).
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
    let supplied_identity_is_authoritative = resource != Resource::Configurations
        && object.get(identity).is_some_and(|value| value.is_string());
    if !supplied_identity_is_authoritative {
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
            "POST /projects/{id}/test_cases, POST /test_suites/{id}/test_cases, the matching /{case_id} delete, or the parent-scoped attachment routes /projects/{id}/test_cases/{case_id}/attachments and /test_suites/{id}/test_cases/{case_id}/attachments"
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

/// One home as the conflict message lists it.
///
/// Each is labelled with the kind of parent it is, so the items in the list can
/// be counted against the number the message opens with: a suite names itself
/// and the project holding it, which a bare `project/suite` path would not make
/// obvious.
fn describe_parent(parent: &Parent) -> String {
    match parent {
        Parent::Project(project) => format!("project {project}"),
        Parent::Suite { project, suite } => format!("suite {suite} in project {project}"),
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
mod tests;
