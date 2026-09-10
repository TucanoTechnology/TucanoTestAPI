//! `/test_suites` — a group of test cases inside a project.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::{DomainError, duplicate},
    storage::{Repository, Resource},
};

use super::{
    AppState,
    crud::prelude::*,
    crud::{crud_handlers, duplicate_handler},
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

async fn add_case_to_suite<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    service.add_case_to_suite(&id, &body)?;
    Ok(Json(json!({ "message": "Test case added to suite" })))
}

async fn remove_case_from_suite<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    service.remove_case_from_suite(&id, &case_id)?;
    Ok(Json(json!({ "message": "Test case removed from suite" })))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
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
        .route("/test_suites/{id}/test_cases", post(add_case_to_suite::<R>))
        .route(
            "/test_suites/{id}/test_cases/{case_id}",
            delete(remove_case_from_suite::<R>),
        )
}
