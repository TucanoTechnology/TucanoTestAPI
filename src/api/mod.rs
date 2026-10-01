//! HTTP layer.
//!
//! One module per resource — `projects`, `suites`, `runs`, `cases`,
//! `milestones`, `configurations` — each owning the routes that address it.
//! [`auth`] is the exception: it owns the session endpoints a client signs in
//! through rather than a resource.
//! A handler does three things only: pull values out of the request, call
//! [`TestService`], and shape the response. Every rule lives in
//! [`crate::domain`]; every byte that reaches disk goes through
//! [`crate::storage`].

pub(crate) mod access;
pub mod auth;
mod cases;
mod configurations;
mod crud;
mod error;
pub mod guardrails;
mod metrics;
mod milestones;
mod projects;
pub mod redact;
mod reports;
mod request_id;
mod runs;
mod suites;
mod workflows;

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use self::auth::AuthState;
use self::guardrails::{GuardrailState, Guardrails};
use self::metrics::HttpMetrics;
use crate::{
    domain::{DomainError, MAX_ATTACHMENT_BYTES, TestService},
    storage::{Repository, StorageProbe},
};

pub use crate::domain::ListQuery;

/// Run a fully synchronous request body (domain service + storage + auth
/// store calls) on the blocking pool instead of the async worker threads
/// (#410).
///
/// The storage layer is intentionally synchronous: advisory-lock waits poll
/// with `std::thread::sleep`, and every read/write/fsync is a `std::fs`
/// call. On the runtime's own threads that made a busy volume starve the
/// executor itself — timers (including the request-timeout guardrail),
/// keep-alives and `/health` alike, down to a handful of workers on a
/// 1-CPU container. Parking the synchronous phase on the blocking pool
/// occupies pool threads sized for exactly this, and frees the executor.
/// Body extractors stay async (streams keep working on workers); only the
/// disk-and-lock phase moves — and when a request is cancelled, its parked
/// operation still runs to its ATOMIC completion, the same guarantee every
/// dropped write already had.
/// Query parameters every `GET` document route accepts.
///
/// Only `children` is meaningful (#415): `?children=ids` answers a project
/// or suite read with its children's identifiers instead of embedded
/// documents. Unknown parameters are ignored by the extractor, and an
/// unknown `children` value is the default shape — documented at
/// `ChildExpansion::from_query`.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct GetQuery {
    pub(crate) children: Option<String>,
}

pub(crate) async fn on_blocking<T, F>(work: F) -> Result<T, DomainError>
where
    F: FnOnce() -> Result<T, DomainError> + Send + 'static,
    T: Send + 'static,
{
    // The audit actor does not cross threads on its own; capture it here and
    // reinstall it inside the blocking closure (#416).
    let actor = crate::domain::current_actor();
    tokio::task::spawn_blocking(move || crate::domain::with_actor(actor, work))
        .await
        // A panic inside the synchronous phase surfaces as the ordinary
        // storage failure (trace on stderr from the pool thread itself);
        // the request still answers the safe envelope.
        .unwrap_or_else(|_| Err(DomainError::Internal("Storage operation failed".to_owned())))
}

/// A parking variant for the probe endpoints, which compute a value rather
/// than a result: `fallback` answers only if the blocking worker itself dies
/// (a panic in probe code — the probes cannot fail any other way) (#410).
pub(crate) async fn park<T, F>(work: F, fallback: T) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let actor = crate::domain::current_actor();
    tokio::task::spawn_blocking(move || crate::domain::with_actor(actor, work))
        .await
        .unwrap_or(fallback)
}

/// The authenticated subject, carried on the response to reach the span
/// recording that runs at response time inside the trace layer.
#[derive(Clone, Debug)]
pub(crate) struct RequestActor(pub String);

/// Installs the acting subject for the request (#416).
///
/// Runs the same extractor a protected handler runs, once, here: it records
/// the verified account on the request span (via the trace layer's response
/// callback, the one place holding the span handle) and holds the actor in
/// a task-local that `on_blocking` re-establishes on the pool thread — so
/// the `audited()` line a write emits names WHO performed it even though
/// the write happens off the request task. A request with no (or an
/// invalid) token keeps no actor: writes possible without one happen on a
/// deployment that enforces no authentication and are honestly attributed
/// to `-`.
async fn audit_actor<R: crate::storage::Repository + 'static>(
    State(state): State<AppState<R>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let (mut parts, body) = request.into_parts();
    let actor = <crate::auth::Principal as axum::extract::FromRequestParts<AppState<R>>>::from_request_parts(
        &mut parts,
        &state,
    )
    .await
    .ok();
    let request = axum::extract::Request::from_parts(parts, body);
    // An enforced token or nothing: on a trusted deployment the extractor
    // answers an anonymous principal whose subject is empty, and the audit
    // trail attributes such writes to the documented `-` rather than an
    // empty string pretending to be an identity.
    match actor.filter(|principal| !principal.user_id.is_empty()) {
        Some(principal) => {
            let user = principal.user_id;
            let mut response =
                crate::domain::with_request_actor(user.clone(), next.run(request)).await;
            response.extensions_mut().insert(RequestActor(user));
            response
        }
        None => next.run(request).await,
    }
}

/// Largest request body the API accepts, in bytes. Uploads beyond this are
/// rejected before they are read.
pub const MAX_BODY_BYTES: usize = MAX_ATTACHMENT_BYTES;

/// Everything a request may need: the service that reaches storage, and the
/// authentication material a guard reads.
///
/// It resolves to the service through [`std::ops::Deref`], so a handler that
/// only touches storage calls the service's methods exactly as it did when the
/// state was a bare [`Arc`]; the authentication half is reached through
/// [`AppState::auth`] and the [`Principal`](crate::auth::Principal) extractor.
pub struct AppState<R> {
    service: Arc<TestService<R>>,
    auth: AuthState,
    metrics: Arc<HttpMetrics>,
    guardrails: Arc<GuardrailState>,
}

impl<R> AppState<R> {
    /// Pairs `service` with the authentication material `auth`.
    pub fn new(service: TestService<R>, auth: AuthState) -> Self {
        Self::with_guardrails(service, auth, Guardrails::default())
    }

    /// The bounds the guardrail middleware enforces on every request.
    pub(crate) fn guardrails(&self) -> &Arc<GuardrailState> {
        &self.guardrails
    }

    /// Pairs `service` with `auth` under an explicit set of request bounds;
    /// the guardrails a deployment configured at startup (#103).
    pub(crate) fn with_guardrails(
        service: TestService<R>,
        auth: AuthState,
        guardrails: Guardrails,
    ) -> Self {
        Self {
            service: Arc::new(service),
            auth,
            metrics: Arc::new(HttpMetrics::new()),
            guardrails: Arc::new(guardrails.state()),
        }
    }

    /// The authentication material a handler's guard reads.
    pub fn auth(&self) -> &AuthState {
        &self.auth
    }

    /// The request counters `/metrics` renders.
    pub fn metrics(&self) -> &HttpMetrics {
        &self.metrics
    }
}

impl<R> std::ops::Deref for AppState<R> {
    type Target = TestService<R>;

    fn deref(&self) -> &Self::Target {
        &self.service
    }
}

// Written out rather than derived so the bound stays `R: Repository` instead of
// growing an `R: Clone` the state does not need.
impl<R> Clone for AppState<R> {
    fn clone(&self) -> Self {
        Self {
            service: Arc::clone(&self.service),
            auth: self.auth.clone(),
            metrics: Arc::clone(&self.metrics),
            guardrails: Arc::clone(&self.guardrails),
        }
    }
}

// Lets the [`Principal`](crate::auth::Principal) extractor reach the
// authentication material from the router's state alone.
impl<R> axum::extract::FromRef<AppState<R>> for AuthState {
    fn from_ref(state: &AppState<R>) -> Self {
        state.auth.clone()
    }
}

/// Every path the router answers on. [`crate::api::router`] registers exactly
/// these, and `tests/service.rs` checks that against `openapi.json`.
pub const ROUTES: &[&str] = &[
    "/health",
    "/ready",
    "/diagnostics",
    "/metrics",
    "/openapi.json",
    "/api-docs",
    "/api-docs/",
    "/projects",
    "/projects/{id}",
    "/projects/{id}/duplicate",
    "/projects/{id}/test_suites",
    "/projects/{id}/test_suites/{suite_id}",
    "/projects/{id}/test_cases",
    "/projects/{id}/test_cases/{case_id}",
    "/projects/{id}/test_cases/{case_id}/history",
    "/projects/{id}/test_cases/{case_id}/history/{version}",
    "/projects/{id}/test_cases/{case_id}/attachments",
    "/projects/{id}/test_cases/{case_id}/attachments/{filename}",
    "/projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
    "/projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}",
    "/projects/{id}/test_runs",
    "/projects/{id}/test_runs/{run_id}",
    "/projects/{id}/milestones",
    "/projects/{id}/milestones/{milestone_id}",
    "/projects/{id}/configurations",
    "/projects/{id}/configurations/{config_id}",
    "/projects/{id}/workflows",
    "/projects/{id}/workflows/{workflow_id}",
    "/projects/{id}/workflows/{workflow_id}/run",
    "/test_suites",
    "/test_suites/{id}",
    "/test_suites/{id}/duplicate",
    "/test_suites/{id}/test_cases",
    "/test_suites/{id}/test_cases/{case_id}",
    "/test_suites/{id}/test_cases/{case_id}/history",
    "/test_suites/{id}/test_cases/{case_id}/history/{version}",
    "/test_suites/{id}/test_cases/{case_id}/attachments",
    "/test_suites/{id}/test_cases/{case_id}/attachments/{filename}",
    "/test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
    "/test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}",
    "/test_runs",
    "/test_runs/{id}",
    "/test_runs/{id}/duplicate",
    "/test_runs/{id}/test_suites",
    "/test_runs/{id}/test_cases",
    "/test_runs/{id}/results",
    "/test_runs/{id}/results/{case_id}",
    "/test_runs/{id}/results/{case_id}/defects",
    "/test_runs/{id}/results/{case_id}/defects/{link_id}",
    "/test_runs/{id}/import/junit",
    "/test_runs/{id}/import/json",
    "/test_runs/{id}/configurations",
    "/test_runs/{id}/configurations/{config_id}",
    "/test_cases",
    "/test_cases/{id}",
    "/test_cases/{id}/duplicate",
    "/test_cases/{id}/attachments",
    "/test_cases/{id}/attachments/{filename}",
    "/test_cases/{id}/steps/{step_index}/attachments",
    "/test_cases/{id}/steps/{step_index}/attachments/{filename}",
    "/test_cases/{id}/history",
    "/test_cases/{id}/history/{version}",
    "/milestones",
    "/milestones/{id}",
    "/milestones/{id}/duplicate",
    "/milestones/{id}/progress",
    "/reports/coverage",
    "/reports/last-results",
    "/reports/summary",
    "/releases",
    "/environments",
    "/configurations",
    "/configurations/{id}",
    "/workflows",
    "/workflows/{id}",
    "/workflows/{id}/duplicate",
    "/auth/login",
    "/auth/refresh",
    "/auth/logout",
    "/auth/me",
];

/// Paths that are served but have no separate entry in the published contract.
///
/// The trailing-slash alias of the Swagger UI is a routing convenience, not a
/// distinct operation. The five bare collection paths are the retired flat
/// creation routes: they survive only to explain where creation moved, so the
/// published contract documents the parent-scoped routes alone. Their global
/// scans stay served — reads are global by design — and are undocumented for the
/// same reason `GET /test_suites` and `GET /test_cases` are.
pub const UNDOCUMENTED_ROUTES: &[&str] = &[
    "/api-docs/",
    "/test_suites",
    "/test_cases",
    "/test_runs",
    "/milestones",
    "/configurations",
];

/// Builds the application under the default request bounds.
pub fn router<R>(repository: R, auth: AuthState) -> Router
where
    R: Repository + 'static,
{
    router_with_guardrails(repository, auth, Guardrails::default())
}

/// Builds the application, backed by `repository`, authenticated with `auth`,
/// and bounded by `guardrails` (#103).
pub fn router_with_guardrails<R>(repository: R, auth: AuthState, guardrails: Guardrails) -> Router
where
    R: Repository + 'static,
{
    let state = AppState::with_guardrails(TestService::new(repository), auth, guardrails);

    Router::<AppState<R>>::new()
        .route("/health", get(health))
        .route("/ready", get(ready::<R>))
        .route("/diagnostics", get(diagnostics::<R>))
        .route("/metrics", get(request_metrics::<R>))
        .route("/openapi.json", get(openapi))
        .route("/api-docs", get(swagger_ui))
        .route("/api-docs/", get(swagger_ui))
        .merge(projects::routes::<R>())
        .merge(suites::routes::<R>())
        .merge(runs::routes::<R>())
        .merge(cases::routes::<R>())
        .merge(milestones::routes::<R>())
        .merge(reports::routes::<R>())
        .merge(configurations::routes::<R>())
        .merge(workflows::routes::<R>())
        .merge(auth::routes::<R>())
        .layer(RequestBodyLimitLayer::new(
            state.guardrails().limits.max_body_bytes,
        ))
        // The permit layer sits outside the body limit so the in-flight count
        // covers reading the body, and the timeout outside everything below
        // Trace: a request cut off or refused still carries the span, the
        // request id, and the metric counters like any other answer (#103).
        .layer(middleware::from_fn_with_state(
            state.clone(),
            guardrails::timeout::<R>,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            guardrails::concurrency::<R>,
        ))
        // An EARLIER `.layer` sits CLOSER to the routes: this must run
        // INSIDE the trace layer, because the span recording rides the
        // trace layer's response callback, which fires on the way out
        // before an outer layer could annotate the response (#416).
        .layer(middleware::from_fn_with_state(
            state.clone(),
            audit_actor::<R>,
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(request_id::request_span)
                .on_response(self::metrics::record_outcome),
        )
        .layer(middleware::from_fn(request_id::propagate))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            self::metrics::track::<R>,
        ))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "storage": "filesystem"}))
}

/// Liveness is not readiness: `/health` answers as soon as the process serves,
/// while `/ready` also asks whether the store behind it can be written to.
///
/// The probe is unauthenticated and answers no 404 — an orchestrator asks it
/// before any credential could be presented — but it reveals nothing about the
/// deployment beyond the three booleans that make up readiness.
async fn ready<R>(State(state): State<AppState<R>>) -> Response
where
    R: Repository + 'static,
{
    // The probe writes a scratch file and tries the lock — disk work, parked
    // like every other synchronous phase (#410). A dead probe worker answers
    // the same 503 shape with its own reason.
    park(
        move || -> Response {
            let probe = state.probe_storage();
            if probe.ready() {
                return Json(json!({"status": "ready", "storage": "filesystem"})).into_response();
            }
            error::envelope(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_ready",
                &format!("storage is not ready: {}", not_ready_reason(&probe)),
            )
        },
        error::envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "not_ready",
            "storage is not ready: the readiness probe worker failed",
        ),
    )
    .await
}

/// The operator's half of the probe: the same checks as `/ready`, reported
/// individually so a failing deployment says which one failed.
///
/// It answers `200` even when the store is not ready — that is the report, not
/// an error — and it names no path, quotes no filesystem error and carries no
/// stored content.
async fn diagnostics<R>(State(state): State<AppState<R>>) -> Json<Value>
where
    R: Repository + 'static,
{
    // Probing means touching the volume; parked like `/ready` (#410). A dead
    // worker reports itself as a not-ready store rather than panicking the
    // caller.
    park(
        move || -> Json<Value> {
            let probe = state.probe_storage();
            Json(json!({
                "storage": "filesystem",
                "ready": probe.ready(),
                "exists": probe.exists,
                "writable": probe.writable,
                "lockable": probe.lockable,
                "lockHeld": probe.lock_held,
                "lastWriteUnix": probe.last_write_unix,
            }))
        },
        Json(json!({
            "storage": "filesystem",
            "ready": false,
            "exists": false,
            "writable": false,
            "lockable": false,
            "lockHeld": false,
            "lastWriteUnix": null,
        })),
    )
    .await
}

/// The counters behind `/metrics`, rendered in the Prometheus text exposition
/// format.
///
/// The endpoint is unguarded for the same reason `/health` is: the caller that
/// has to ask how the process is doing is often the one that cannot
/// authenticate. It exposes the routes the deployment serves and how they are
/// answering, which the published contract already names, and nothing about the
/// documents behind them.
async fn request_metrics<R>(State(state): State<AppState<R>>) -> Response
where
    R: Repository + 'static,
{
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics().render(),
    )
        .into_response()
}

/// Which of the three readiness checks failed, in a sentence that names no path
/// and repeats no filesystem error.
fn not_ready_reason(probe: &StorageProbe) -> &'static str {
    if !probe.exists {
        "the data directory is missing"
    } else if !probe.writable {
        "the data directory is not writable"
    } else {
        "the storage lock cannot be taken"
    }
}

async fn openapi() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(include_str!("../../openapi.json")),
    )
        .into_response()
}

async fn swagger_ui() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Body::from(include_str!("../../swagger.html")),
    )
        .into_response()
}
