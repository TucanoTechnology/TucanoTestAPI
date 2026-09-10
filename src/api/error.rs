//! Turns a [`DomainError`] into the response envelope the API has always used.
//!
//! Every failure leaves the API as `{"error":{"code":…,"message":…}}` with the
//! status and code the legacy handlers produced, so clients see no change.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::domain::DomainError;

/// Renders the stable error envelope.
pub(crate) fn envelope(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
        .into_response()
}

impl IntoResponse for DomainError {
    fn into_response(self) -> Response {
        match self {
            DomainError::NotFound(message) => {
                envelope(StatusCode::NOT_FOUND, "not_found", &message)
            }
            DomainError::InvalidRequest { code, message } => {
                envelope(StatusCode::BAD_REQUEST, code, &message)
            }
            DomainError::Conflict(message) => envelope(StatusCode::CONFLICT, "conflict", &message),
            DomainError::PayloadTooLarge => envelope(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Attachment exceeds 50 MiB",
            ),
            DomainError::Internal(message) => {
                envelope(StatusCode::INTERNAL_SERVER_ERROR, "storage_error", &message)
            }
            DomainError::Storage => envelope(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                "Storage operation failed",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn the_envelope_carries_a_code_and_a_message() {
        let response = envelope(StatusCode::CONFLICT, "conflict", "already there");
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let body = runtime.block_on(async {
            response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes()
        });
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");

        assert_eq!(value["error"]["code"], "conflict");
        assert_eq!(value["error"]["message"], "already there");
    }
}
