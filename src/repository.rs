//! Compatibility re-exports.
//!
//! Persistence moved to [`crate::storage`] as part of the layered architecture
//! split; this module keeps the previous crate paths working.

pub use crate::storage::fs::FileRepository;
pub use crate::storage::{Repository, Resource};
