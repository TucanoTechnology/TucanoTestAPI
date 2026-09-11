//! Typed domain errors, plus the single place where filesystem
//! [`io::ErrorKind`]s are translated into them.
//!
//! Storage deals in [`std::io::Error`]; nothing above it should have to. Each
//! classifier below mirrors one of the legacy inline `match error.kind()` blocks
//! so the HTTP layer only has to map [`DomainError`] variants onto status codes.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

/// A failure the API can render as a stable error envelope.
#[derive(Debug)]
pub enum DomainError {
    /// The requested resource does not exist.
    NotFound(String),
    /// The request was malformed; `code` is the stable machine-readable code.
    InvalidRequest { code: &'static str, message: String },
    /// The request conflicts with existing state.
    Conflict(String),
    /// The uploaded attachment is larger than [`crate::domain::MAX_ATTACHMENT_BYTES`].
    PayloadTooLarge,
    /// An unexpected internal failure carrying a specific message.
    Internal(String),
    /// A storage failure whose details must not reach the client.
    Storage,
    /// The request carried no usable credential; `code` names what was wrong.
    Unauthenticated { code: &'static str, message: String },
    /// The credential is valid, but the caller may not perform this request.
    Forbidden(String),
}

impl DomainError {
    /// `invalid_id` — the identifier in the path is not a usable component.
    pub fn invalid_id() -> Self {
        Self::InvalidRequest {
            code: "invalid_id",
            message: "Invalid resource ID".to_owned(),
        }
    }

    /// `invalid_request` — a generic 400 carrying a caller-supplied message.
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            code: "invalid_request",
            message: message.into(),
        }
    }

    /// `invalid_status` — the status value is not one of the accepted results.
    pub fn invalid_status() -> Self {
        Self::InvalidRequest {
            code: "invalid_status",
            message: "Status must be Passed, Failed, Blocked, Untested, or Retest".to_owned(),
        }
    }

    /// `invalid_status` — an imported status is not one the importer can map.
    ///
    /// Reports and result arrays can only describe the three outcomes a runner
    /// produces, so an import narrows the run's five statuses rather than
    /// accepting `Untested` or `Retest`, which no report can mean.
    pub fn invalid_import_status() -> Self {
        Self::InvalidRequest {
            code: "invalid_status",
            message: "Imported status must be Passed, Failed, or Blocked".to_owned(),
        }
    }

    /// `missing_token` — no `Authorization` header, or no bearer credential in it.
    pub fn missing_token() -> Self {
        Self::Unauthenticated {
            code: "missing_token",
            message: "Authentication required".to_owned(),
        }
    }

    /// `invalid_token` — the access token could not be verified.
    pub fn invalid_token() -> Self {
        Self::Unauthenticated {
            code: "invalid_token",
            message: "Invalid access token".to_owned(),
        }
    }

    /// `token_expired` — the access token was well-formed but has expired.
    pub fn token_expired() -> Self {
        Self::Unauthenticated {
            code: "token_expired",
            message: "Access token has expired".to_owned(),
        }
    }

    /// `invalid_credentials` — the username or the password is wrong.
    ///
    /// The same answer for both, so the reply never says which half was right.
    pub fn invalid_credentials() -> Self {
        Self::Unauthenticated {
            code: "invalid_credentials",
            message: "Invalid username or password".to_owned(),
        }
    }

    /// `invalid_refresh_token` — the refresh token is unknown, expired, or spent.
    pub fn invalid_refresh_token() -> Self {
        Self::Unauthenticated {
            code: "invalid_refresh_token",
            message: "Invalid refresh token".to_owned(),
        }
    }

    /// `forbidden` — authenticated, but without the role this request needs.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }
}

impl Display for DomainError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) => write!(formatter, "not found: {message}"),
            Self::InvalidRequest { code, message } => write!(formatter, "{code}: {message}"),
            Self::Conflict(message) => write!(formatter, "conflict: {message}"),
            Self::PayloadTooLarge => write!(formatter, "payload too large"),
            Self::Internal(message) => write!(formatter, "internal error: {message}"),
            Self::Storage => write!(formatter, "storage operation failed"),
            Self::Unauthenticated { code, message } => write!(formatter, "{code}: {message}"),
            Self::Forbidden(message) => write!(formatter, "forbidden: {message}"),
        }
    }
}

impl Error for DomainError {}

impl From<io::Error> for DomainError {
    /// Generic storage translation, matching the legacy `storage_error` helper.
    ///
    /// `AlreadyExists` is storage's way of saying a name is taken — by another
    /// child of the same parent, by a folder holding a different kind of child,
    /// or by the parent's own marker file — so it becomes a conflict rather
    /// than an opaque storage failure.
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound("Resource not found".to_owned()),
            io::ErrorKind::InvalidInput => Self::invalid_request("Invalid request"),
            io::ErrorKind::AlreadyExists => Self::Conflict("Resource already exists".to_owned()),
            _ => Self::Storage,
        }
    }
}

/// Translation for the get-resource path.
pub fn read_error(error: io::Error) -> DomainError {
    match error.kind() {
        io::ErrorKind::InvalidInput => DomainError::invalid_id(),
        io::ErrorKind::NotFound => DomainError::NotFound("Resource not found".to_owned()),
        io::ErrorKind::InvalidData => DomainError::Internal("Stored JSON is invalid".to_owned()),
        _ => DomainError::from(error),
    }
}

/// Translation for delete, where a missing document is reported and an
/// unusable identifier is a client error.
pub fn delete_error(error: io::Error) -> DomainError {
    match error.kind() {
        io::ErrorKind::NotFound => DomainError::NotFound("Resource not found".to_owned()),
        io::ErrorKind::InvalidInput => DomainError::invalid_id(),
        _ => DomainError::from(error),
    }
}

/// Translation for reading one stored document, where the caller names the
/// missing entity and an unusable identifier is reported as such rather than
/// as a generic bad request.
pub fn document_error(error: io::Error, missing: &str) -> DomainError {
    match error.kind() {
        io::ErrorKind::NotFound => DomainError::NotFound(missing.to_owned()),
        io::ErrorKind::InvalidInput => DomainError::invalid_id(),
        _ => DomainError::from(error),
    }
}

/// Translation for loading a related document, where the caller names the
/// missing entity (composition, duplication).
pub fn load_error(error: io::Error, missing: &str) -> DomainError {
    match error.kind() {
        io::ErrorKind::NotFound => DomainError::NotFound(missing.to_owned()),
        _ => DomainError::from(error),
    }
}

/// Translation for the milestone-progress load.
pub fn milestone_error(error: io::Error) -> DomainError {
    match error.kind() {
        io::ErrorKind::NotFound => DomainError::NotFound("Milestone not found".to_owned()),
        io::ErrorKind::InvalidInput => DomainError::invalid_id(),
        _ => DomainError::from(error),
    }
}

/// Translation for attachment access: a missing or unusable file is a 404.
pub fn attachment_error(error: io::Error) -> DomainError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::InvalidInput | io::ErrorKind::IsADirectory => {
            DomainError::NotFound("File not found".to_owned())
        }
        _ => DomainError::from(error),
    }
}
