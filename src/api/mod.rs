//! HTTP layer.
//!
//! One module per resource — [`projects`], [`suites`], [`runs`], [`cases`],
//! [`milestones`], [`configurations`] — each owning the routes that address it.
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
mod milestones;
mod projects;
mod reports;
mod request_id;
mod runs;
mod suites;

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    http::header,
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use self::auth::AuthState;
use crate::{
    domain::{MAX_ATTACHMENT_BYTES, TestService},
    storage::Repository,
};

pub use crate::domain::ListQuery;

/// Largest request body the API accepts, in bytes. Uploads beyond this are
/// rejected before they are read.
pub const MAX_BODY_BYTES: usize = MAX_ATTACHMENT_BYTES;

/// Everything a request may need: the service that reaches storage, and the
/// authentication material a guard reads.
///
/// It resolves to the service through [`Deref`], so a handler that only touches
/// storage calls the service's methods exactly as it did when the state was a
/// bare [`Arc`]; the authentication half is reached through [`AppState::auth`]
/// and the [`Principal`](crate::auth::Principal) extractor.
pub struct AppState<R> {
    service: Arc<TestService<R>>,
    auth: AuthState,
}

impl<R> AppState<R> {
    /// Pairs `service` with the authentication material `auth`.
    pub fn new(service: TestService<R>, auth: AuthState) -> Self {
        Self {
            service: Arc::new(service),
            auth,
        }
    }

    /// The authentication material a handler's guard reads.
    pub fn auth(&self) -> &AuthState {
        &self.auth
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
    "/test_suites",
    "/test_suites/{id}",
    "/test_suites/{id}/duplicate",
    "/test_suites/{id}/test_cases",
    "/test_suites/{id}/test_cases/{case_id}",
    "/test_runs",
    "/test_runs/{id}",
    "/test_runs/{id}/duplicate",
    "/test_runs/{id}/test_suites",
    "/test_runs/{id}/test_cases",
    "/test_runs/{id}/results",
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
    "/reports/summary",
    "/configurations",
    "/configurations/{id}",
    "/auth/login",
    "/auth/refresh",
    "/auth/logout",
    "/auth/me",
];

/// Paths that are served but have no separate entry in the published contract.
///
/// The trailing-slash alias of the Swagger UI is a routing convenience, not a
/// distinct operation. `POST /test_suites` and `POST /test_cases` are the
/// retired flat creation routes: they survive only to explain where creation
/// moved, so the published contract documents the parent-scoped routes alone.
pub const UNDOCUMENTED_ROUTES: &[&str] = &["/api-docs/", "/test_suites", "/test_cases"];

/// Builds the application, backed by `repository` and authenticated with
/// `auth`.
pub fn router<R>(repository: R, auth: AuthState) -> Router
where
    R: Repository + 'static,
{
    let state = AppState::new(TestService::new(repository), auth);

    Router::<AppState<R>>::new()
        .route("/health", get(health))
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
        .merge(auth::routes::<R>())
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TraceLayer::new_for_http().make_span_with(request_id::request_span))
        .layer(middleware::from_fn(request_id::propagate))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "storage": "filesystem"}))
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
