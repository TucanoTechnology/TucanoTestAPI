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

use crate::domain::duplicate::DuplicateSpec;

/// Generates `list_*`, `get_*`, `create_*`, `update_*` and `delete_*` for one
/// resource. Import [`prelude`] before invoking it.
macro_rules! crud_handlers {
    ($list:ident, $get:ident, $create:ident, $update:ident, $delete:ident, $resource:expr) => {
        pub(crate) async fn $list<R: Repository>(
            State(service): State<AppState<R>>,
            Query(query): Query<ListQuery>,
        ) -> Result<Json<Value>, DomainError> {
            let items = service.list($resource, &query)?;
            Ok(Json(json!(items)))
        }

        pub(crate) async fn $get<R: Repository>(
            State(service): State<AppState<R>>,
            Path(id): Path<String>,
        ) -> Result<Json<Value>, DomainError> {
            Ok(Json(service.get($resource, &id)?))
        }

        pub(crate) async fn $create<R: Repository>(
            State(service): State<AppState<R>>,
            Json(body): Json<Value>,
        ) -> Result<(StatusCode, Json<Value>), DomainError> {
            let created = service.create($resource, &body)?;
            Ok((
                StatusCode::CREATED,
                Json(json!({ "message": "Resource created", "id": created.id })),
            ))
        }

        pub(crate) async fn $update<R: Repository>(
            State(service): State<AppState<R>>,
            Path(id): Path<String>,
            Json(body): Json<Value>,
        ) -> Result<Json<Value>, DomainError> {
            service.update($resource, &id, &body)?;
            Ok(Json(json!({ "message": "Resource updated" })))
        }

        pub(crate) async fn $delete<R: Repository>(
            State(service): State<AppState<R>>,
            Path(id): Path<String>,
        ) -> Result<Json<Value>, DomainError> {
            service.delete($resource, &id)?;
            Ok(Json(json!({ "message": "Resource deleted" })))
        }
    };
}

/// Generates the `duplicate_*` handler for one resource. Import [`prelude`]
/// before invoking it.
macro_rules! duplicate_handler {
    ($handler:ident, $spec:expr) => {
        pub(crate) async fn $handler<R: Repository>(
            State(service): State<AppState<R>>,
            Path(id): Path<String>,
            Json(body): Json<Value>,
        ) -> Result<(StatusCode, Json<Value>), DomainError> {
            let new_id = service.duplicate(&$spec, &id, &body)?;
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

pub(crate) use crud_handlers;
pub(crate) use duplicate_handler;

/// Everything the generated handlers name, in one import.
pub mod prelude {
    pub(crate) use axum::{
        Json,
        extract::{Path, Query, State},
        http::StatusCode,
    };
    pub(crate) use serde_json::{Value, json};

    pub(crate) use crate::{
        domain::{DomainError, ListQuery},
        storage::Repository,
    };
}
