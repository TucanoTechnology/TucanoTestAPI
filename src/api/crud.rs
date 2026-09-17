//! The five handlers every CRUD resource shares, plus the duplication handler.
//!
//! All six resources (`projects`, `test_suites`, `test_runs`, `test_cases`,
//! `milestones`, `configurations`) speak the same protocol: list, read, create,
//! update and delete, with the same response bodies. Rather than repeat thirty
//! near-identical functions, the macros here generate them per resource; a
//! resource module then only adds what makes it different — composition,
//! attachments, progress, duplication.
//!
//! `macro_rules!` resolves the names in a macro body at the point where the
//! macro is invoked, so a module using these must have [`prelude`] in scope:
//!
//! ```ignore
//! use super::crud::{crud_handlers, duplicate_handler};
//! use super::crud::prelude::*;
//! ```

use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

use crate::domain::Composed;
use crate::domain::duplicate::DuplicateSpec;

/// Generates `list_*`, `get_*`, `create_*`, `update_*` and `delete_*` for one
/// resource. Import [`prelude`] before invoking it.
///
/// Each handler authorizes before it touches storage: a listing is filtered to
/// the projects the caller can reach, and the rest ask [`super::access`] whether
/// the caller may reach the resource this request names.
macro_rules! crud_handlers {
    ($list:ident, $get:ident, $create:ident, $update:ident, $delete:ident, $resource:expr) => {
        pub(crate) async fn $list<R: Repository>(
            State(state): State<AppState<R>>,
            principal: Principal,
            Query(query): Query<ListQuery>,
        ) -> Result<Json<Value>, DomainError> {
            let scope = access::scope(state.auth(), &principal)?;
            let items = state.list($resource, &query)?;
            Ok(Json(json!(access::filter_list(
                &state,
                $resource,
                items,
                scope.as_ref(),
            )?)))
        }

        pub(crate) async fn $get<R: Repository>(
            State(state): State<AppState<R>>,
            principal: Principal,
            Path(id): Path<String>,
        ) -> Result<(HeaderMap, Json<Value>), DomainError> {
            access::guard_get(&state, &principal, $resource, &id)?;
            let document = state.get($resource, &id)?;
            let mut headers = HeaderMap::new();
            if let Some(hash) = state.etag($resource, &id) {
                let etag = format!("\"{hash}\"");
                if let Ok(value) = HeaderValue::from_str(&etag) {
                    headers.insert(ETAG, value);
                }
            }
            Ok((headers, Json(document)))
        }

        pub(crate) async fn $create<R: Repository>(
            State(state): State<AppState<R>>,
            principal: Principal,
            Json(body): Json<Value>,
        ) -> Result<(StatusCode, Json<Value>), DomainError> {
            access::guard_create(&state, &principal, $resource, &body)?;
            let created = state.create($resource, &body)?;
            Ok((
                StatusCode::CREATED,
                Json(json!({ "message": "Resource created", "id": created.id })),
            ))
        }

        pub(crate) async fn $update<R: Repository>(
            State(state): State<AppState<R>>,
            principal: Principal,
            Path(id): Path<String>,
            if_match: IfMatchHeader,
            Json(body): Json<Value>,
        ) -> Result<Json<Value>, DomainError> {
            access::guard_update(&state, &principal, $resource, &id, &body)?;
            let expected_etag = if if_match.0.is_empty() {
                None
            } else {
                Some(if_match.0)
            };
            state.update_with_etag($resource, &id, &body, expected_etag)?;
            Ok(Json(json!({ "message": "Resource updated" })))
        }

        pub(crate) async fn $delete<R: Repository>(
            State(state): State<AppState<R>>,
            principal: Principal,
            Path(id): Path<String>,
        ) -> Result<Json<Value>, DomainError> {
            access::guard_delete(&state, &principal, $resource, &id)?;
            state.delete($resource, &id)?;
            Ok(Json(json!({ "message": "Resource deleted" })))
        }
    };
}

/// Generates the `duplicate_*` handler for one resource. Import [`prelude`]
/// before invoking it.
macro_rules! duplicate_handler {
    ($handler:ident, $spec:expr) => {
        pub(crate) async fn $handler<R: Repository>(
            State(state): State<AppState<R>>,
            principal: Principal,
            Path(id): Path<String>,
            Json(body): Json<Value>,
        ) -> Result<(StatusCode, Json<Value>), DomainError> {
            access::guard_duplicate(&state, &principal, $spec.resource, &id)?;
            let new_id = state.duplicate(&$spec, &id, &body)?;
            Ok($crate::api::crud::duplicated(&$spec, &new_id))
        }
    };
}

/// The 201 body a successful duplication returns, unchanged from the legacy
/// handlers.
pub(crate) fn duplicated(spec: &DuplicateSpec, new_id: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::CREATED,
        Json(json!({ "message": spec.duplicated_message, "id": new_id })),
    )
}

/// The 201 body a parent-scoped creation returns. `noun` names the resource, so
/// the message reads "Test run created" rather than repeating a generic one.
pub(crate) fn created_response(noun: &str, id: String) -> (StatusCode, Json<Value>) {
    (
        StatusCode::CREATED,
        Json(json!({ "message": format!("{noun} created"), "id": id })),
    )
}

/// The 201 body a create-or-place request returns. `noun` names the resource so
/// the message says whether it was created, copied, or moved.
pub(crate) fn composed_response(composed: &Composed, noun: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::CREATED,
        Json(json!({ "message": composed.message(noun), "id": composed.id() })),
    )
}

pub(crate) use crud_handlers;
pub(crate) use duplicate_handler;

/// Everything the generated handlers name, plus the pieces the hand-written
/// handlers reach for most often (`Role`, `access`), in one import.
pub mod prelude {
    pub(crate) use axum::http::{HeaderMap, HeaderValue};
    pub(crate) use axum::{
        Json,
        extract::{Path, Query, State},
        http::{StatusCode, header::ETAG},
    };
    pub(crate) use serde_json::{Value, json};

    pub(crate) use crate::{
        api::access,
        auth::{Principal, Role},
        domain::{DomainError, ListQuery},
        storage::Repository,
    };

    /// Extracts the `If-Match` header value, stripping surrounding quotes.
    ///
    /// A client sends `If-Match: "abc123"` (with quotes, per RFC 7232); the
    /// ETag the server computed is stored without them, so the quotes are
    /// stripped here before comparison. An absent header yields an empty
    /// string, which the handler maps to `None` (last-writer-wins).
    pub(crate) struct IfMatchHeader(pub String);

    impl<S: Send + Sync> axum::extract::FromRequestParts<S> for IfMatchHeader {
        type Rejection = std::convert::Infallible;

        async fn from_request_parts(
            parts: &mut axum::http::request::Parts,
            _state: &S,
        ) -> Result<Self, Self::Rejection> {
            let value = parts
                .headers
                .get(axum::http::header::IF_MATCH)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim_matches('"').to_owned())
                .unwrap_or_default();
            Ok(IfMatchHeader(value))
        }
    }
}
