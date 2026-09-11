//! Domain layer: the rules that decide what the API does, expressed without any
//! knowledge of HTTP or of the filesystem.
//!
//! Handlers in [`crate::api`] translate requests into calls on [`TestService`];
//! persistence lives behind [`crate::storage::Repository`]. Everything in
//! between — payload validation, identifier derivation, composition rules,
//! duplication and progress maths — belongs here, and each piece is unit tested
//! without spinning up a server.

pub mod composition;
pub mod defect;
pub mod duplicate;
pub mod error;
pub mod import;
pub mod progress;
pub mod reports;
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
///
/// `configuration` names a top-level configuration and only means something for
/// `test_runs`, the one collection whose documents carry configuration
/// references; the other listings ignore it.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListQuery {
    pub filter: Option<String>,
    pub tags: Option<String>,
    pub configuration: Option<String>,
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

/// Timestamp recorded as a test case's `lastModified`, in ISO-8601 UTC.
///
/// Test-case versioning records when a version was written in ISO-8601 UTC
/// (`2023-11-14T22:13:20Z`), as the versioning plan specifies. That deliberately
/// diverges from the Unix-seconds string [`current_timestamp_string`] writes
/// into runs, so the two formats coexist: a stored document keeps whichever
/// format its own field documents.
pub fn current_iso8601_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_iso8601(now)
}

/// The calendar date (`YYYY-MM-DD`) of an instant given as Unix seconds, which
/// is what the summary report's date filters compare against.
pub fn iso8601_date(seconds: u64) -> String {
    format_iso8601(seconds)[..10].to_owned()
}

/// Renders seconds since the Unix epoch as an ISO-8601 UTC timestamp.
///
/// Hand-rolled rather than pulled from a date-time crate: the API has no such
/// dependency, and this needs exactly one fixed, UTC-only shape.
fn format_iso8601(seconds: u64) -> String {
    let days = seconds / 86_400;
    let rem = seconds % 86_400;
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);

    // Days-to-civil conversion (Howard Hinnant's `civil_from_days`), shifted so
    // the era boundary sits on a 400-year cycle of 146 097 days.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_renders_known_epochs() {
        for (seconds, expected) in [
            (0_u64, "1970-01-01T00:00:00Z"),
            (1, "1970-01-01T00:00:01Z"),
            (86_399, "1970-01-01T23:59:59Z"),
            (86_400, "1970-01-02T00:00:00Z"),
            (1_700_000_000, "2023-11-14T22:13:20Z"),
            // A leap day: 2024-02-29 is 28 days after 2024-02-01.
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (1_704_067_199, "2023-12-31T23:59:59Z"),
        ] {
            assert_eq!(format_iso8601(seconds), expected, "epoch {seconds}");
        }
    }

    #[test]
    fn current_iso8601_timestamp_has_the_documented_shape() {
        let value = current_iso8601_timestamp();
        assert_eq!(value.len(), 20, "timestamp: {value}");
        assert!(value.ends_with('Z'), "timestamp: {value}");
        assert_eq!(&value[4..5], "-");
        assert_eq!(&value[10..11], "T");
    }
}
