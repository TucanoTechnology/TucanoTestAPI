//! Turns a [`DomainError`] into the response envelope the API has always used.
//!
//! Every failure leaves the API as `{"error":{"code":…,"message":…}}` with the
//! status and code the legacy handlers produced, so clients see no change.
//! When the failure happens inside a request scope the envelope also carries
//! `requestId`, matching the `X-Request-Id` response header. A 401 additionally
//! carries the `WWW-Authenticate` challenge RFC 7235 requires of it.

use axum::{
    Json,
    http::{
        StatusCode,
        header::{HeaderValue, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{api::request_id, domain::DomainError};

/// The `WWW-Authenticate` challenge a 401 must carry.
///
/// RFC 7235 makes the header mandatory on a 401, and RFC 6750 gives the
/// `Bearer` scheme the `error` parameter a client uses to tell a token it
/// should replace from one it merely failed to send. An expired access token is
/// reported as `invalid_token`, the code that tells a client to refresh; a
/// request that carried no usable credentials gets the realm alone, because
/// RFC 6750 asks a server not to name an error when there was nothing to
/// authenticate with.
fn challenge(code: &str) -> HeaderValue {
    let value = match code {
        "invalid_token" | "token_expired" => {
            "Bearer realm=\"Tucano Test API\", error=\"invalid_token\""
        }
        _ => "Bearer realm=\"Tucano Test API\"",
    };
    HeaderValue::from_static(value)
}

/// The envelope as serde sees it: `{"error":{"code":…,"message":…}}`.
#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
    /// Present only when the failure happened inside a request scope, so the
    /// unit test below and any out-of-band caller keep the historical shape.
    #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
}

/// Renders the stable error envelope.
pub(crate) fn envelope(status: StatusCode, code: &str, message: &str) -> Response {
    let body = ErrorEnvelope {
        error: ErrorBody {
            code,
            message,
            request_id: request_id::current(),
        },
    };
    (status, Json(body)).into_response()
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
            DomainError::Unauthenticated { code, message } => {
                let mut response = envelope(StatusCode::UNAUTHORIZED, code, &message);
                response
                    .headers_mut()
                    .insert(WWW_AUTHENTICATE, challenge(code));
                response
            }
            DomainError::Forbidden(message) => {
                envelope(StatusCode::FORBIDDEN, "forbidden", &message)
            }
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

    #[test]
    fn auth_failures_render_401_and_403_with_their_own_codes() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let cases = [
            (
                DomainError::missing_token(),
                StatusCode::UNAUTHORIZED,
                "missing_token",
            ),
            (
                DomainError::invalid_token(),
                StatusCode::UNAUTHORIZED,
                "invalid_token",
            ),
            (
                DomainError::token_expired(),
                StatusCode::UNAUTHORIZED,
                "token_expired",
            ),
            (
                DomainError::invalid_credentials(),
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
            ),
            (
                DomainError::invalid_refresh_token(),
                StatusCode::UNAUTHORIZED,
                "invalid_refresh_token",
            ),
            (
                DomainError::forbidden("Viewer cannot edit this project"),
                StatusCode::FORBIDDEN,
                "forbidden",
            ),
        ];

        for (error, expected_status, expected_code) in cases {
            let response = error.into_response();
            assert_eq!(response.status(), expected_status, "{expected_code}");
            let body = runtime.block_on(async {
                response
                    .into_body()
                    .collect()
                    .await
                    .expect("body")
                    .to_bytes()
            });
            let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(value["error"]["code"], expected_code);
            assert!(
                value["error"]["message"].is_string(),
                "{expected_code} carries a message"
            );
        }
    }

    #[test]
    fn a_401_challenges_the_caller_and_other_failures_do_not() {
        let cases = [
            (
                DomainError::missing_token(),
                "Bearer realm=\"Tucano Test API\"",
            ),
            (
                DomainError::invalid_credentials(),
                "Bearer realm=\"Tucano Test API\"",
            ),
            (
                DomainError::invalid_refresh_token(),
                "Bearer realm=\"Tucano Test API\"",
            ),
            (
                DomainError::invalid_token(),
                "Bearer realm=\"Tucano Test API\", error=\"invalid_token\"",
            ),
            (
                DomainError::token_expired(),
                "Bearer realm=\"Tucano Test API\", error=\"invalid_token\"",
            ),
        ];

        for (error, expected) in cases {
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                response
                    .headers()
                    .get(WWW_AUTHENTICATE)
                    .expect("a 401 must carry a challenge"),
                expected
            );
        }

        for error in [
            DomainError::forbidden("not yours"),
            DomainError::NotFound("missing".into()),
            DomainError::Conflict("taken".into()),
            DomainError::Storage,
        ] {
            assert!(
                error
                    .into_response()
                    .headers()
                    .get(WWW_AUTHENTICATE)
                    .is_none(),
                "only a 401 carries an authentication challenge"
            );
        }
    }
}
