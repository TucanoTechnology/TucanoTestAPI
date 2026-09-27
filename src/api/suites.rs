//! `/test_suites` — a group of test cases inside a project.
//!
//! A suite has no top-level collection: it is created inside a project, either
//! through `/projects/{id}/test_suites` or by placing an existing suite there.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::{DomainError, duplicate},
    storage::{Parent, Repository, Resource},
};

use super::{
    AppState,
    crud::prelude::*,
    crud::{composed_response, crud_handlers, duplicate_handler},
};

crud_handlers!(
    list_test_suites,
    get_test_suite,
    create_test_suite,
    update_test_suite,
    delete_test_suite,
    Resource::Suites
);

duplicate_handler!(duplicate_suite, duplicate::SUITE);

async fn list_project_suites<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require(&service, &principal, &id, Role::Viewer)?;
        let items =
            service.list_children_matching(&Parent::Project(id), Resource::Suites, &query)?;
        Ok(Json(json!(items)))
    })
    .await
}

async fn create_project_suite<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<(StatusCode, Json<Value>), DomainError> {
        access::guard_composition(
            &service,
            &principal,
            Resource::Suites,
            &id,
            &body,
            Role::Editor,
        )?;
        let composed = service.compose(Resource::Suites, &Parent::Project(id), &body)?;
        Ok(composed_response(&composed, "Test suite"))
    })
    .await
}

async fn delete_project_suite<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, suite_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        // The route names the project that owns the occurrence, so the role is
        // checked there rather than through the global lookup in `guard_delete`,
        // which conflicts while two projects hold the same suite identifier.
        access::require(&service, &principal, &id, Role::Editor)?;
        service.delete_in(Resource::Suites, &Parent::Project(id), &suite_id)?;
        Ok(Json(json!({ "message": "Test suite deleted" })))
    })
    .await
}

async fn list_suite_cases<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        let parent = service.suite_parent(&id)?;
        access::require(&service, &principal, parent.project(), Role::Viewer)?;
        // Deliberately unfiltered: a suite's cases are answered exhaustively.
        // Issue #293 adds the query to the project-scoped listings only.
        let items = service.list_children(&parent, Resource::Cases)?;
        Ok(Json(json!(items)))
    })
    .await
}

async fn add_case_to_suite<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<(StatusCode, Json<Value>), DomainError> {
        let parent = service.suite_parent(&id)?;
        access::guard_composition(
            &service,
            &principal,
            Resource::Cases,
            parent.project(),
            &body,
            Role::Editor,
        )?;
        let composed = service.compose(Resource::Cases, &parent, &body)?;
        Ok(composed_response(&composed, "Test case"))
    })
    .await
}

async fn remove_case_from_suite<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::guard_removal(&service, &principal, Resource::Suites, &id, Role::Editor)?;
        service.remove_case_from_suite(&id, &case_id)?;
        Ok(Json(json!({ "message": "Test case removed from suite" })))
    })
    .await
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        // Retired: a suite is created inside a project. The handler answers with
        // an explanation rather than a document.
        .route(
            "/test_suites",
            get(list_test_suites::<R>).post(create_test_suite::<R>),
        )
        .route(
            "/test_suites/{id}",
            get(get_test_suite::<R>)
                .put(update_test_suite::<R>)
                .delete(delete_test_suite::<R>),
        )
        .route("/test_suites/{id}/duplicate", post(duplicate_suite::<R>))
        .route(
            "/test_suites/{id}/test_cases",
            get(list_suite_cases::<R>).post(add_case_to_suite::<R>),
        )
        .route(
            "/test_suites/{id}/test_cases/{case_id}",
            delete(remove_case_from_suite::<R>),
        )
        .route(
            "/projects/{id}/test_suites",
            get(list_project_suites::<R>).post(create_project_suite::<R>),
        )
        .route(
            "/projects/{id}/test_suites/{suite_id}",
            delete(delete_project_suite::<R>),
        )
}
