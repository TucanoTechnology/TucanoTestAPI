//! Revision storage: the history a qualifying update records.

use super::*;

impl FileRepository {
    pub(super) fn save_revision(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
        value: &Value,
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = self.save_revision_inner(parent, case, version, value);
        lock.unlock()?;
        result
    }

    /// Write a revision snapshot without acquiring the advisory lock.
    ///
    /// Used by callers that already hold the lock (such as `transform_at`)
    /// to avoid deadlocking on a second `lock_exclusive()` call.
    pub(super) fn save_revision_locked(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
        value: &Value,
    ) -> io::Result<()> {
        self.save_revision_inner(parent, case, version, value)
    }

    fn save_revision_inner(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
        value: &Value,
    ) -> io::Result<()> {
        let marker = case_marker(&self.root, parent, case)?;
        if !marker.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "test case does not exist",
            ));
        }
        let revision = revision_marker(&self.root, parent, case, version)?;
        if revision.exists() {
            // A revision snapshot is immutable: an existing one is never rewritten.
            return Ok(());
        }
        self.write_json(&revision, value)
    }

    pub(super) fn list_revisions(&self, parent: &Parent, case: &str) -> io::Result<Vec<u64>> {
        let directory = revision_dir(&self.root, parent, case)?;
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut versions = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            })
            .filter_map(|entry| entry.file_name().to_str().and_then(revision_number))
            .collect::<Vec<_>>();
        versions.sort_unstable();
        Ok(versions)
    }

    pub(super) fn read_revision(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
    ) -> io::Result<Value> {
        self.read_json(&revision_marker(&self.root, parent, case, version)?)
    }
}
