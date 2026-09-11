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
        ) -> Result<Json<Value>, DomainError> {
            access::guard_get(&state, &principal, $resource, &id)?;
            Ok(Json(state.get($resource, &id)?))
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
            Json(body): Json<Value>,
        ) -> Result<Json<Value>, DomainError> {
            access::guard_update(&state, &principal, $resource, &id, &body)?;
            state.update($resource, &id, &body)?;
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
    pub(crate) use axum::{
        Json,
        extract::{Path, Query, State},
        http::StatusCode,
    };
    pub(crate) use serde_json::{Value, json};

    pub(crate) use crate::{
        api::access,
        auth::{Principal, Role},
        domain::{DomainError, ListQuery},
        storage::Repository,
    };
}
