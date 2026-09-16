//! Persistence boundary.
//!
//! HTTP and domain layers depend on [`Repository`], never on a concrete
//! implementation. The filesystem backend lives in [`fs`].

pub mod fs;
pub mod layout;

pub use fs::FileRepository;
pub use layout::{
    Parent, Placement, RESERVED_PROJECT_CHILDREN, Resource, attachment_path, case_dir, case_marker,
    ensure_within, folder_name, folder_wire_id, node_folder, parent_dir, parent_marker,
    project_collection_dir, project_dir, project_document_path, project_marker, revision_dir,
    revision_marker, root_dir, set_private_permissions, step_attachment_path, step_dir, suite_dir,
    suite_marker, unique_suffix, validate_component, validate_document_id,
};

use serde_json::Value;
use std::io;

/// What a readiness or diagnostics probe could learn about a store.
///
/// The probe is deliberately a plain value rather than a `Result`: a store that
/// cannot be reached is exactly what readiness exists to report, so "missing",
/// "not writable" and "no lock" are answers, not errors. Nothing here names a
/// path — a probe is served to clients, and the storage-security rules keep
/// deployment layout out of responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageProbe {
    /// The data root exists and is a directory.
    pub exists: bool,
    /// A scratch file could be created and removed in the data root, which is
    /// what persisting a document requires.
    pub writable: bool,
    /// The advisory lock the store serialises writes with could be taken. A
    /// lock another replica already holds counts as available: the store works,
    /// it is merely busy.
    pub lockable: bool,
    /// Some other process holds the lock right now.
    pub lock_held: bool,
    /// The most recent modification time seen on the data root itself or one of
    /// its immediate entries, in seconds since the Unix epoch. It is a liveness
    /// hint about the volume, not a per-document watermark.
    pub last_write_unix: Option<u64>,
}

impl StorageProbe {
    /// Whether the store can serve requests that write.
    pub fn ready(&self) -> bool {
        self.exists && self.writable && self.lockable
    }

    /// The probe of a store that could not be reached at all.
    pub const UNREACHABLE: StorageProbe = StorageProbe {
        exists: false,
        writable: false,
        lockable: false,
        lock_held: false,
        last_write_unix: None,
    };
}

/// Storage operations the domain needs, expressed in terms of [`Resource`].
///
/// Hierarchy resources (projects, suites, cases) are addressed with the
/// [`Parent`] that owns them; runs, milestones and configurations are addressed
/// with the [`Parent::Project`] that owns them, and their identifiers are unique
/// within a project. Implementations return raw [`io::Error`]s; translating them
/// into domain errors is the domain layer's job.
pub trait Repository: Send + Sync {
    /// Identifiers stored for a resource across the whole tree, de-duplicated
    /// and sorted.
    fn list(&self, resource: Resource) -> io::Result<Vec<String>>;

    /// Every parent that owns an occurrence of an entity, in a stable order.
    /// Empty means nothing owns it; more than one is ambiguous.
    fn locate(&self, resource: Resource, id: &str) -> io::Result<Vec<Parent>>;

    /// Identifiers of the children `parent` owns, sorted.
    fn list_children(&self, parent: &Parent, child: Resource) -> io::Result<Vec<String>>;

    /// Whether a document exists at the addressed location.
    fn exists_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<bool>;

    /// Read a stored document.
    fn read_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<Value>;

    /// Atomically persist a document.
    fn write_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> io::Result<()>;

    /// Remove a document, its folder, and everything it owns.
    fn delete_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<()>;

    /// Copy or move a hierarchy node from one parent to another.
    fn place(
        &self,
        resource: Resource,
        source: &Parent,
        id: &str,
        target: &Parent,
        mode: Placement,
    ) -> io::Result<()>;

    /// Store a supplementary file for a case and record it in the case
    /// document, under one lock so file and metadata never diverge.
    fn save_attachment(
        &self,
        parent: &Parent,
        case: &str,
        filename: &str,
        entry: &Value,
        contents: &[u8],
    ) -> io::Result<()>;

    /// Write an immutable revision snapshot of a case under `revisions/`.
    ///
    /// The snapshot is the full document as it stood at `version`; a snapshot
    /// that already exists is never rewritten, so history stays append-only.
    fn save_revision(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
        value: &Value,
    ) -> io::Result<()>;

    /// List the revision numbers a case has snapshots for, ascending. A case
    /// with no `revisions/` folder has no history, which is not an error.
    fn list_revisions(&self, parent: &Parent, case: &str) -> io::Result<Vec<u64>>;

    /// Read the immutable revision snapshot of a case at `version`.
    fn read_revision(&self, parent: &Parent, case: &str, version: u64) -> io::Result<Value>;

    /// Read a supplementary file of a case.
    fn read_attachment(&self, parent: &Parent, case: &str, filename: &str) -> io::Result<Vec<u8>>;

    /// Remove a supplementary file of a case and its stored metadata.
    fn delete_attachment(&self, parent: &Parent, case: &str, filename: &str) -> io::Result<()>;

    /// Store a supplementary file for one structured step of a case and record
    /// it in that step's metadata, under one lock so file and metadata never
    /// diverge.
    fn save_step_attachment(
        &self,
        parent: &Parent,
        case: &str,
        step_index: usize,
        filename: &str,
        entry: &Value,
        contents: &[u8],
    ) -> io::Result<()>;

    /// Remove a supplementary file of one structured step of a case and its
    /// stored metadata.
    fn delete_step_attachment(
        &self,
        parent: &Parent,
        case: &str,
        step_index: usize,
        filename: &str,
    ) -> io::Result<()>;

    /// Report what a readiness or diagnostics probe can learn about the store.
    ///
    /// This is the one operation that never fails: "the store is missing" is
    /// the answer readiness exists to give, so it is reported in the returned
    /// [`StorageProbe`] instead of as an error.
    fn probe_readiness(&self) -> StorageProbe;
}
