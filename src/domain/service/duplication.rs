//! Duplication of an existing document under a new identifier.

use super::*;

impl<R: Repository> TestService<R> {
    // --- duplication ---------------------------------------------------

    /// Copies a document, applying the request-body overrides, and returns the
    /// identifier of the copy.
    ///
    /// A copy lands in the home the source belongs to, so it stays where the
    /// original is; only a project lives at the top level. Neither a duplicate
    /// nor a `PUT` can therefore move a document into another project.
    ///
    /// A `newId` the body supplies has to be an identifier the store can file:
    /// anything else is `invalid_id` rather than a name the store would refuse
    /// later under a different code.
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
        crate::storage::validate_document_id(spec.resource, &new_id)
            .map_err(|_| DomainError::invalid_id())?;

        // Existence check and write are ONE operation under one lock
        // (`create_at`): a separate `exists_at` followed by an unconditional
        // write lets a racing duplicate or create of the same `newId` destroy
        // a document (#406). The loser reports the conflict with the
        // operation's own message; normalisation matches `write_marker`.
        audited(resource_noun(spec.resource), "duplicate", &new_id, || {
            let mut stored = document.clone();
            normalise_marker(spec.resource, &new_id, &mut stored);
            self.repository
                .create_at(spec.resource, parent.as_ref(), &new_id, &stored)
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        DomainError::Conflict(spec.already_exists_message.to_owned())
                    } else {
                        DomainError::from(error)
                    }
                })
        })?;
        Ok(new_id)
    }
}
