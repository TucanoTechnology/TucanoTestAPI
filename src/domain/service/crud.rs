//! Generic CRUD over documents: listing, reading, creating, updating, deleting.

use super::*;

impl<R: Repository> TestService<R> {
    // --- generic CRUD --------------------------------------------------

    /// Lists a resource, applying the optional substring and tag filters.
    pub fn list(&self, resource: Resource, query: &ListQuery) -> Result<Vec<String>, DomainError> {
        let mut items = self.repository.list(resource)?;
        apply_list_query(resource, query, &mut items, |item| {
            self.first_document(resource, item).ok()
        });
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
        self.list_children_matching(parent, child, &ListQuery::default())
    }

    /// Identifiers of the children `parent` owns that satisfy `query`.
    ///
    /// A project-scoped listing judges each child on the document the named
    /// parent holds, not on the first occurrence a global lookup resolves: a
    /// suite or case identifier two projects hold is filtered as the occurrence
    /// the addressed parent owns, exactly as it is deleted (`delete_in`) and
    /// read back through the parent-scoped routes.
    ///
    /// The filters themselves are the ones the global listings apply, shared
    /// through `apply_list_query`, so `?filter=`, `?tags=` and the runs-only
    /// `?configuration=` narrow a parent's children the same way they narrow a
    /// global scan. A filter that matches nothing yields an empty listing
    /// rather than an error.
    pub fn list_children_matching(
        &self,
        parent: &Parent,
        child: Resource,
        query: &ListQuery,
    ) -> Result<Vec<String>, DomainError> {
        self.require_parent(parent)?;
        let mut items = self
            .repository
            .list_children(parent, child)
            .map_err(error::read_error)?;
        apply_list_query(child, query, &mut items, |item| {
            self.repository.read_at(child, Some(parent), item).ok()
        });
        Ok(items)
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
        self.update_with_etag(resource, id, value, None)
    }

    /// Validates a partial body, checks an optional `If-Match` ETag, and merges
    /// the body into the stored document under one lock.
    ///
    /// When `expected_etag` is present, the stored document's content hash is
    /// compared to it under the advisory lock. A mismatch returns
    /// [`DomainError::PreconditionFailed`] carrying the current ETag so the
    /// client can re-read and retry. An absent ETag preserves the legacy
    /// last-writer-wins behaviour.
    ///
    /// The write goes back to the addressed identifier, so an identity field
    /// the body carries is checked rather than obeyed by
    /// `refuse_foreign_identity` below.
    pub fn update_with_etag(
        &self,
        resource: Resource,
        id: &str,
        value: &Value,
        expected_etag: Option<String>,
    ) -> Result<(), DomainError> {
        validation::validate_payload(resource, value)?;
        let parent = self.owner_for_write(resource, id, "Resource not found")?;
        self.refuse_foreign_identity(resource, parent.as_ref(), id, value)?;
        let value = value.clone();
        let etag_ref = expected_etag.as_deref().filter(|s| !s.is_empty());
        audited(resource_noun(resource), "update", id, || {
            self.repository
                .transform_at(resource, parent.as_ref(), id, etag_ref, |stored| {
                    let mut merged = merged_document(&stored, &value)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
                    if resource == Resource::Cases {
                        self.revise_case(parent.as_ref(), id, &stored, &mut merged)
                            .map_err(|e| io::Error::other(e.to_string()))?;
                    }
                    let mut document = merged;
                    super::normalise_marker(resource, id, &mut document);
                    Ok(document)
                })
                .map_err(|e| {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        DomainError::PreconditionFailed {
                            current_etag: e.to_string(),
                        }
                    } else {
                        error::document_error(e, "Resource not found")
                    }
                })
        })
    }

    /// Refuses an identity field in a `PUT` body that does not name the
    /// document the request addressed.
    ///
    /// The write always lands on the addressed identifier, so obeying a
    /// differing value would file a document whose identity names something
    /// else. A value that is not a usable identifier at all is `invalid_id`; a
    /// usable value that names another document is `invalid_request`, because
    /// an identifier is immutable and a rename is a delete followed by a
    /// create.
    ///
    /// The identity the stored document already carries is accepted before the
    /// value is judged usable, so a client that re-sends a document it read
    /// round-trips unchanged — including a document stored under a legacy
    /// identity that no longer passes [`crate::storage::validate_document_id`].
    fn refuse_foreign_identity(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> Result<(), DomainError> {
        let Some(field) = body_identity_field(resource) else {
            return Ok(());
        };
        let Some(supplied) = value.get(field).and_then(Value::as_str) else {
            return Ok(());
        };
        if supplied == id {
            return Ok(());
        }
        let stored = self
            .repository
            .read_at(resource, parent, id)
            .map_err(|error| error::document_error(error, "Resource not found"))?;
        if stored.get(field).and_then(Value::as_str) == Some(supplied) {
            return Ok(());
        }
        crate::storage::validate_document_id(resource, supplied)
            .map_err(|_| DomainError::invalid_id())?;
        Err(DomainError::invalid_request(format!(
            "Field `{field}` names another document; an identifier is immutable, delete the resource and recreate it to rename it"
        )))
    }

    /// Removes a document, its folder, and everything it owns.
    pub fn delete(&self, resource: Resource, id: &str) -> Result<(), DomainError> {
        let parent = self.owner_for_write(resource, id, "Resource not found")?;
        audited(resource_noun(resource), "delete", id, || {
            self.repository
                .delete_at(resource, parent.as_ref(), id)
                .map_err(error::delete_error)
        })
    }

    /// Removes one occurrence of a child from a parent the caller named.
    pub fn delete_in(
        &self,
        resource: Resource,
        parent: &Parent,
        id: &str,
    ) -> Result<(), DomainError> {
        self.require_parent(parent)?;
        audited(resource_noun(resource), "delete", id, || {
            self.repository
                .delete_at(resource, Some(parent), id)
                .map_err(error::delete_error)
        })
    }
}

/// Applies the `ListQuery` filters to an identifier listing, in the order the
/// contract documents: `?filter=`, then `?tags=`, then the runs-only
/// `?configuration=`. Every parameter sent has to be satisfied.
///
/// `load` reads the document an identifier names, from whichever location the
/// caller lists — the global scan for [`TestService::list`], the named parent
/// for [`TestService::list_children_matching`]. An identifier whose document
/// `load` cannot produce never matches, and neither does a document without the
/// array a filter walks, so an unnamed tag or configuration yields an empty
/// listing rather than an error.
fn apply_list_query<F: Fn(&str) -> Option<Value>>(
    resource: Resource,
    query: &ListQuery,
    items: &mut Vec<String>,
    load: F,
) {
    if let Some(filter) = query.filter.as_ref() {
        let needle = filter.to_lowercase();
        items.retain(|item| item.to_lowercase().contains(&needle));
    }

    if let Some(tags_param) = query.tags.as_ref() {
        let requested = requested_tags(tags_param);
        items.retain(|item| {
            load(item).is_some_and(|value| document_has_any_tag(&value, &requested))
        });
    }

    // Only runs carry configuration references.
    if resource == Resource::Runs
        && let Some(config_id) = query.configuration.as_ref()
    {
        items.retain(|item| {
            load(item).is_some_and(|value| document_links_configuration(&value, config_id))
        });
    }
}

/// The requested tags, compared as the stored ones are: comma-separated, each
/// element trimmed and matched case-insensitively.
fn requested_tags(parameter: &str) -> Vec<String> {
    parameter
        .split(',')
        .map(|tag| tag.trim().to_lowercase())
        .collect()
}

/// Whether `document` carries at least one of `requested`.
///
/// A document without a `tags` array never matches: `?tags=` selects what a
/// resource is labelled with, never what it lacks.
fn document_has_any_tag(document: &Value, requested: &[String]) -> bool {
    let Some(tags) = document.get("tags").and_then(Value::as_array) else {
        return false;
    };
    tags.iter()
        .filter_map(|tag| tag.as_str())
        .any(|tag| requested.contains(&tag.to_lowercase()))
}

/// Whether `document` links the configuration `config_id` through its
/// `configurations` array.
fn document_links_configuration(document: &Value, config_id: &str) -> bool {
    let Some(configurations) = document.get("configurations").and_then(Value::as_array) else {
        return false;
    };
    configurations.iter().any(|configuration| {
        configuration.get("configId").and_then(Value::as_str) == Some(config_id)
    })
}

/// The identity field a `PUT` body may carry, for the routes that name a
/// document by a `.json` identifier.
///
/// A test case is absent on purpose: its identifier names the case verbatim
/// and the test-case routes publish `invalid_request` alone, so the case
/// document routes read an identifier from the path only.
fn body_identity_field(resource: Resource) -> Option<&'static str> {
    match resource {
        Resource::Projects => Some("projectId"),
        Resource::Suites => Some("suiteId"),
        Resource::Runs => Some("testRunId"),
        Resource::Milestones => Some("milestoneId"),
        Resource::Configurations => Some("configId"),
        Resource::Cases => None,
    }
}
