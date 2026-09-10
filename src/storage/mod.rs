//! Persistence boundary.
//!
//! HTTP and domain layers depend on [`Repository`], never on a concrete
//! implementation. The filesystem backend lives in [`fs`].

pub mod fs;
pub mod layout;

pub use fs::FileRepository;
pub use layout::{
    Parent, Placement, Resource, attachment_path, case_dir, case_marker, document_path,
    ensure_within, folder_name, folder_wire_id, node_folder, parent_dir, parent_marker,
    project_dir, project_marker, root_dir, set_private_permissions, step_attachment_path, step_dir,
    suite_dir, suite_marker, unique_suffix, validate_component,
};

use serde_json::Value;
use std::io;

/// Storage operations the domain needs, expressed in terms of [`Resource`].
///
/// Hierarchy resources (projects, suites, cases) are addressed with the
/// [`Parent`] that owns them; flat resources (runs, milestones, configurations)
/// pass `None` and rely on a globally unique identifier. Implementations return
/// raw [`io::Error`]s; translating them into domain errors is the domain layer's
/// job.
pub trait Repository: Send + Sync {
    /// Identifiers stored for a resource across the whole tree, de-duplicated
    /// and sorted.
    fn list(&self, resource: Resource) -> io::Result<Vec<String>>;

    /// Every parent that owns an occurrence of a hierarchy node, in a stable
    /// order. Empty means nothing owns it; more than one is ambiguous.
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
}
