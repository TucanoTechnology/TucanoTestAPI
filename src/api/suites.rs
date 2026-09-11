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

async fn list_project_suites<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    access::require(&service, &principal, &id, Role::Viewer)?;
    let items = service.list_children(&Parent::Project(id), Resource::Suites)?;
    Ok(Json(json!(items)))
}

async fn create_project_suite<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
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
}

async fn delete_project_suite<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, suite_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::guard_delete(&service, &principal, Resource::Suites, &suite_id)?;
    service.delete_in(Resource::Suites, &Parent::Project(id), &suite_id)?;
    Ok(Json(json!({ "message": "Test suite deleted" })))
}

async fn list_suite_cases<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.suite_parent(&id)?;
    access::require(&service, &principal, parent.project(), Role::Viewer)?;
    let items = service.list_children(&parent, Resource::Cases)?;
    Ok(Json(json!(items)))
}

async fn add_case_to_suite<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
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
}

async fn remove_case_from_suite<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::guard_removal(&service, &principal, Resource::Suites, &id, Role::Editor)?;
    service.remove_case_from_suite(&id, &case_id)?;
    Ok(Json(json!({ "message": "Test case removed from suite" })))
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
