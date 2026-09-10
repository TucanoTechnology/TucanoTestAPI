//! Domain layer: the rules that decide what the API does, expressed without any
//! knowledge of HTTP or of the filesystem.
//!
//! Handlers in [`crate::api`] translate requests into calls on [`TestService`];
//! persistence lives behind [`crate::storage::Repository`]. Everything in
//! between — payload validation, identifier derivation, composition rules,
//! duplication and progress maths — belongs here, and each piece is unit tested
//! without spinning up a server.

pub mod composition;
pub mod duplicate;
pub mod error;
pub mod progress;
pub mod resources;
pub mod service;
pub mod validation;

pub use error::DomainError;
pub use service::{Composed, TestService};

use serde::Deserialize;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Largest attachment — and therefore request body — the API accepts, in bytes.
pub const MAX_ATTACHMENT_BYTES: usize = 50 * 1024 * 1024;

/// Query parameters accepted by every list endpoint.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListQuery {
    pub filter: Option<String>,
    pub tags: Option<String>,
}

/// Identifier assigned to a freshly created resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub id: String,
}

/// Metadata describing an attachment that was just persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAttachment {
    pub filename: String,
    pub original_name: String,
    pub size: usize,
}

/// Reads a non-empty string field, mirroring the legacy handler helper.
pub fn required_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Timestamp written into new test runs, as Unix seconds rendered as a string.
///
/// The legacy JSON shape stores this value as a string even though it is not an
/// ISO-8601 timestamp; keeping the format avoids changing stored documents.
pub fn current_timestamp_string() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

/// Content type recorded for, and served with, a stored attachment.
pub fn mime_type(filename: &str) -> &'static str {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}
