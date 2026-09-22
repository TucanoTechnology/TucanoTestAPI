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
}
