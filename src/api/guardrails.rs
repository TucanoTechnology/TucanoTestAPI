//! Request guardrails: timeout and concurrency limits (#103).
//!
//! Both layers sit inside the tracing and request-id middleware and outside
//! the body limit, so a request they refuse is answered with the ordinary
//! error envelope — code, message, request id, `X-Request-Id` — and counted by
//! the metrics and the request span exactly like every other answer. Neither
//! layer queues: a saturated server refuses with `503` and a `Retry-After`,
//! and a request past its deadline is cut off with `504`. The bound body limit
//! itself lives in the router's `RequestBodyLimitLayer`, sized from the same
//! configuration.
//!
//! Cancellation is part of the contract, not an afterthought. The concurrency
//! permit is owned by the guarded future, so a client disconnect — or the
//! timeout layer dropping everything inside it — releases the permit on drop.
//! Work already handed to the repository's blocking pool finishes on its own
//! terms, and those terms are the atomic write: a cancelled request can leave
//! the old document or the new one, never a partial one, and never a held
//! lock, because the advisory lock is taken and released by the write itself
//! rather than by the request whose future was dropped.

use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use tokio::sync::Semaphore;

use super::{AppState, error::envelope};
use crate::storage::Repository;

/// Default `TUCANO_REQUEST_TIMEOUT_MS`: five minutes.
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 300_000;
/// Default `TUCANO_MAX_CONCURRENCY`: 128 requests in flight.
pub const DEFAULT_MAX_CONCURRENCY: usize = 128;

/// The configured bounds a router enforces around every request.
#[derive(Clone, Debug)]
pub struct Guardrails {
    /// Largest body the router accepts, in bytes (`TUCANO_MAX_BODY_BYTES`).
    pub max_body_bytes: usize,
    /// Wall clock a request may take before it is cut off with `504`
    /// (`TUCANO_REQUEST_TIMEOUT_MS`); `None` disables the timeout.
    pub request_timeout: Option<Duration>,
    /// Requests allowed in flight before new ones are refused with `503`
    /// (`TUCANO_MAX_CONCURRENCY`); `None` removes the cap.
    pub max_concurrency: Option<usize>,
}

impl Default for Guardrails {
    fn default() -> Self {
        Self {
            max_body_bytes: super::MAX_BODY_BYTES,
            // Generous by design: a slow-network attachment upload must not be
            // what this kills, while a wedged request must not hold work
            // forever. Operators tighten it; they never have to enable it.
            request_timeout: Some(Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS)),
            // Comfortably above what a single-volume deployment serves at
            // once; it bounds worst-case work rather than shaping traffic.
            max_concurrency: Some(DEFAULT_MAX_CONCURRENCY),
        }
    }
}

impl Guardrails {
    /// Capacity standing in for "no cap". Tokio's semaphore refuses
    /// `usize::MAX`-ish sizes outright (its permit count shares a word with
    /// the waiter state), so the uncapped default is its stated maximum, an
    /// imaginary number of requests rather than a limit anyone reaches.
    const UNCAPPED_IN_FLIGHT: usize = 1 << 60;

    /// The live half: the configuration plus the semaphore sized by
    /// `max_concurrency`. An uncapped deployment gets an enormous semaphore
    /// rather than an optional one, so the middleware branches on
    /// configuration only where it answers `503`.
    pub fn state(&self) -> GuardrailState {
        GuardrailState {
            limits: self.clone(),
            semaphore: Arc::new(Semaphore::new(
                self.max_concurrency.unwrap_or(Self::UNCAPPED_IN_FLIGHT),
            )),
        }
    }
}

/// The guardrails as the middleware sees them: configuration plus permits.
#[derive(Clone, Debug)]
pub struct GuardrailState {
    pub limits: Guardrails,
    semaphore: Arc<Semaphore>,
}

/// Middleware: cut off requests that outlive the configured timeout.
pub(crate) async fn timeout<R>(
    State(state): State<AppState<R>>,
    req: Request,
    next: Next,
) -> Response
where
    R: Repository + 'static,
{
    let Some(limit) = state.guardrails().limits.request_timeout else {
        return next.run(req).await;
    };
    match tokio::time::timeout(limit, next.run(req)).await {
        Ok(response) => response,
        Err(_elapsed) => envelope(
            StatusCode::GATEWAY_TIMEOUT,
            "request_timeout",
            "The request exceeded the configured timeout. Please retry.",
        ),
    }
}

/// Middleware: refuse new requests while `max_concurrency` are in flight.
pub(crate) async fn concurrency<R>(
    State(state): State<AppState<R>>,
    req: Request,
    next: Next,
) -> Response
where
    R: Repository + 'static,
{
    if state.guardrails().limits.max_concurrency.is_none() {
        return next.run(req).await;
    }
    let Ok(permit) = state.guardrails().semaphore.clone().try_acquire_owned() else {
        let mut response = envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "The server is at its request concurrency limit. Please retry.",
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        return response;
    };
    // The permit is owned by this future: dropping it — the client hanging
    // up, or the timeout layer above cutting this request off — releases it.
    let _permit = permit;
    next.run(req).await
}
