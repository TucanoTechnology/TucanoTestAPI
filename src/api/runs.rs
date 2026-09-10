//! `/test_runs` — a point-in-time execution of suites and cases.

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
    list_test_runs,
    get_test_run,
    create_test_run,
    update_test_run,
    delete_test_run,
    Resource::Runs
);

duplicate_handler!(duplicate_test_run, duplicate::RUN);

async fn add_suite_to_run<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    service.add_suite_to_run(&id, &body)?;
    Ok(Json(json!({ "message": "Test suite added to test run" })))
}

async fn add_case_to_run<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    service.add_case_to_run(&id, &body)?;
    Ok(Json(json!({ "message": "Test case added to test run" })))
}

async fn record_run_result<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    service.record_run_result(&id, &body)?;
    Ok(Json(json!({ "message": "Test result recorded in run" })))
}

async fn link_run_configuration<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    service.link_configuration_to_run(&id, &body)?;
    Ok(Json(
        json!({ "message": "Test configuration linked to test run" }),
    ))
}

async fn unlink_run_configuration<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, config_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    service.unlink_configuration_from_run(&id, &config_id)?;
    Ok(Json(
        json!({ "message": "Test configuration unlinked from test run" }),
    ))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        .route(
            "/test_runs",
            get(list_test_runs::<R>).post(create_test_run::<R>),
        )
        .route(
            "/test_runs/{id}",
            get(get_test_run::<R>)
                .put(update_test_run::<R>)
                .delete(delete_test_run::<R>),
        )
        .route("/test_runs/{id}/duplicate", post(duplicate_test_run::<R>))
        .route("/test_runs/{id}/test_suites", post(add_suite_to_run::<R>))
        .route("/test_runs/{id}/test_cases", post(add_case_to_run::<R>))
        .route("/test_runs/{id}/results", post(record_run_result::<R>))
        .route(
            "/test_runs/{id}/configurations",
            post(link_run_configuration::<R>),
        )
        .route(
            "/test_runs/{id}/configurations/{config_id}",
            delete(unlink_run_configuration::<R>),
        )
}
