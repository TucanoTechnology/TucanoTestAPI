//! Attachments of a case and of its structured steps.

use super::*;

impl<R: Repository> TestService<R> {
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
}
