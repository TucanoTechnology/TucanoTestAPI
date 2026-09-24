//! Gives every request an id, so a client report can be traced to its lines.
//!
//! An inbound `X-Request-Id` is honoured when it is present and usable;
//! otherwise the API mints one. Either way the id is echoed back on the
//! response, carried in the error envelope, and written to the request span.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};

use super::redact;

/// The header the API reads an inbound id from and echoes it back on.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// A per-request correlation id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(String);

impl RequestId {
    /// Borrows the id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Mints an id that is unique in practice and needs no dependency.
    ///
    /// [`RandomState`] is seeded per process and re-seeded on each call, and
    /// the counter separates two calls that land inside the same seed. Neither
    /// half is cryptographic: the id is a correlation handle, not a secret.
    fn mint() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        Self(format!("{:016x}", hasher.finish()))
    }
}

tokio::task_local! {
    static CURRENT_REQUEST_ID: RequestId;
}

/// Reads the inbound id, if there is a usable one.
///
/// Returns the id together with the header value to echo, so the response
/// carries back exactly the bytes the client sent.
fn inbound(request: &Request) -> Option<(RequestId, HeaderValue)> {
    let value = request.headers().get(REQUEST_ID_HEADER)?.clone();
    if value.is_empty() {
        return None;
    }
    let text = value.to_str().ok()?;
    Some((RequestId(text.to_owned()), value))
}

/// Resolves an id for the request, publishes it, and echoes it back.
pub async fn propagate(mut request: Request, next: Next) -> Response {
    let (id, echo) = inbound(&request).unwrap_or_else(|| {
        let id = RequestId::mint();
        let echo = HeaderValue::from_str(id.as_str()).expect("minted id is visible ASCII");
        (id, echo)
    });

    request.extensions_mut().insert(id.clone());
    let mut response = CURRENT_REQUEST_ID.scope(id, next.run(request)).await;
    response
        .headers_mut()
        .insert(HeaderName::from_static(REQUEST_ID_HEADER), echo);
    response
}

/// The id of the request being served, if there is one.
pub fn current() -> Option<String> {
    CURRENT_REQUEST_ID
        .try_with(|id| id.as_str().to_owned())
        .ok()
}

/// Names the request span, stamps the id on it, and redacts its query string.
///
/// The span carries the path rather than the whole URI, because a client can
/// put a credential in a query string — `?token=…`, `?api_key=…` — and a span
/// is copied to every sink the deployment configures. The query is recorded
/// separately, through [`redact::query`], so an operator still sees which
/// parameters a request carried without seeing what they held. Headers are
/// never logged at all.
///
/// `status` and `duration_ms` are declared empty here because neither is known
/// until the response exists; [`super::metrics::record_outcome`] fills them in
/// when the response is produced, which keeps one line per request rather than
/// two.
pub fn request_span(request: &Request) -> tracing::Span {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or("-", RequestId::as_str);
    let span = tracing::info_span!(
        "http.request",
        method = %request.method(),
        path = %request.uri().path(),
        query = tracing::field::Empty,
        request_id = %id,
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    );
    if let Some(query) = request.uri().query() {
        span.record("query", tracing::field::display(redact::query(query)));
    }
    span
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_ids_differ_and_are_sixteen_hex_characters() {
        let first = RequestId::mint();
        let second = RequestId::mint();
        assert_ne!(first, second);
        assert_eq!(first.as_str().len(), 16);
        assert!(first.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn current_is_absent_outside_a_request() {
        assert_eq!(current(), None);
    }
}
