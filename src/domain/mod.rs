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

/// Content type recorded for a stored attachment.
///
/// This is metadata about the file, kept in the case document's `mimeType`; it
/// is not the type a download is answered with, which is always
/// [`ATTACHMENT_MEDIA_TYPE`].
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

/// Media type every attachment download is answered with.
///
/// The stored media type is recorded in the case document (`mimeType`) rather
/// than replayed on the wire: a download is opaque bytes, so a client that
/// decodes by content type can never mis-read a text attachment as a string and
/// non-UTF-8 bytes are never corrupted by a text decode.
pub const ATTACHMENT_MEDIA_TYPE: &str = "application/octet-stream";

/// The name the client supplied for a stored attachment.
///
/// A stored name is `<unique suffix>-<original name>`, so the original is what
/// follows the first hyphen when everything before it is digits. A name that
/// does not have that shape is reported as itself, which is what a caller that
/// addresses an attachment by an already-known name needs.
pub fn original_name(filename: &str) -> &str {
    match filename.split_once('-') {
        Some((suffix, rest))
            if !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            rest
        }
        _ => filename,
    }
}

/// Value of the `Content-Disposition` header for a download of `filename`.
///
/// The uploader's name travels in both the plain and the RFC 5987 forms a
/// client may understand: `filename` carries an ASCII-safe rendering, and
/// `filename*` the exact UTF-8 name percent-encoded, which is added only when
/// the two differ. Every byte outside printable ASCII is replaced in the plain
/// form, so quoting and control characters from an uploaded name can never
/// escape into the header.
pub fn content_disposition(filename: &str) -> String {
    let name = original_name(filename);
    let plain: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_graphic() && character != '"' && character != '\\' {
                character
            } else {
                '_'
            }
        })
        .collect();
    let encoded = percent_encode(name);
    if plain == encoded {
        format!("attachment; filename=\"{plain}\"")
    } else {
        format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
    }
}

/// Percent-encodes a name with the attribute character set RFC 5987 allows.
fn percent_encode(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());
    for &byte in name.as_bytes() {
        let allowed = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            );
        if allowed {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
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

    #[test]
    fn original_name_undoes_the_stored_prefix() {
        assert_eq!(original_name("1789740136589400280-notes.txt"), "notes.txt");
        // The original may carry a numeric prefix of its own; only the stored
        // one is stripped.
        assert_eq!(
            original_name("1789740136589400280-123-notes.txt"),
            "123-notes.txt"
        );
        assert_eq!(original_name("notes.txt"), "notes.txt");
        assert_eq!(original_name("1234.txt"), "1234.txt");
    }

    #[test]
    fn content_disposition_names_the_original_file() {
        assert_eq!(
            content_disposition("1789740136589400280-report.pdf"),
            "attachment; filename=\"report.pdf\""
        );
        // A non-ASCII name keeps an ASCII-safe rendering in `filename` and its
        // exact bytes in the RFC 5987 `filename*` form.
        assert_eq!(
            content_disposition("1789740136589400280-rapport-généré.txt"),
            "attachment; filename=\"rapport-g_n_r_.txt\"; \
             filename*=UTF-8''rapport-g%C3%A9n%C3%A9r%C3%A9.txt"
        );
    }

    #[test]
    fn content_disposition_cannot_escape_the_header() {
        let disposition = content_disposition("17-ev\"il\r\nX-Evil: 1.txt");
        assert_eq!(
            disposition,
            "attachment; filename=\"ev_il__X-Evil:_1.txt\"; \
             filename*=UTF-8''ev%22il%0D%0AX-Evil%3A%201.txt"
        );
        assert_eq!(
            disposition,
            disposition.trim(),
            "the value carries no line break: {disposition:?}"
        );
    }
}
