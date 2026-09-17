//! Test-case history: the revisions a qualifying update records.

use super::*;

impl<R: Repository> TestService<R> {
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
}
