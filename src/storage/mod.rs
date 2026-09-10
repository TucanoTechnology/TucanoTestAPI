//! Persistence boundary.
//!
//! HTTP and domain layers depend on [`Repository`], never on a concrete
//! implementation. The filesystem backend lives in [`fs`].

pub mod fs;
pub mod layout;

pub use fs::FileRepository;
pub use layout::{
    Resource, attachment_path, document_path, ensure_within, resource_dir, set_private_permissions,
    test_case_dir, unique_suffix, validate_component,
};

use serde_json::Value;
use std::io;

/// Storage operations the domain needs, expressed in terms of [`Resource`].
///
/// Implementations return raw [`io::Error`]s; translating them into domain
/// errors is the domain layer's job.
pub trait Repository: Send + Sync {
    /// Identifiers stored for a resource, sorted.
    fn list(&self, resource: Resource) -> io::Result<Vec<String>>;

    /// Read a stored document.
    fn read(&self, resource: Resource, id: &str) -> io::Result<Value>;

    /// Whether a document exists.
    fn exists(&self, resource: Resource, id: &str) -> io::Result<bool>;

    /// Atomically persist a document.
    fn write(&self, resource: Resource, id: &str, value: &Value) -> io::Result<()>;

    /// Remove a document and everything it owns.
    fn delete(&self, resource: Resource, id: &str) -> io::Result<()>;

    /// Store a supplementary file for a test case.
    fn save_attachment(&self, id: &str, filename: &str, contents: &[u8]) -> io::Result<()>;

    /// Read a supplementary file of a test case.
    fn read_attachment(&self, id: &str, filename: &str) -> io::Result<Vec<u8>>;

    /// Remove a supplementary file of a test case.
    fn delete_attachment(&self, id: &str, filename: &str) -> io::Result<()>;
}
