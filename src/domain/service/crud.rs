//! Generic CRUD over documents: listing, reading, creating, updating, deleting.

use super::*;

impl<R: Repository> TestService<R> {
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
    pub fn update_with_etag(
        &self,
        resource: Resource,
        id: &str,
        value: &Value,
        expected_etag: Option<String>,
    ) -> Result<(), DomainError> {
        validation::validate_payload(resource, value)?;
        let parent = self.owner_for_write(resource, id, "Resource not found")?;
        let value = value.clone();
        let etag_ref = expected_etag.as_deref().filter(|s| !s.is_empty());
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
}
