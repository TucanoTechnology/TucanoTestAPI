//! `/test_runs` — a point-in-time execution of suites and cases.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::{DomainError, duplicate},
    models::ImportSummary,
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
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    access::require_run_source(
        &service,
        &principal,
        &id,
        Resource::Suites,
        body.get("suiteId").and_then(Value::as_str),
        Role::Editor,
    )?;
    service.add_suite_to_run(&id, &body)?;
    Ok(Json(json!({ "message": "Test suite added to test run" })))
}

async fn add_case_to_run<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    access::require_run_source(
        &service,
        &principal,
        &id,
        Resource::Cases,
        body.get("testCaseId").and_then(Value::as_str),
        Role::Editor,
    )?;
    service.add_case_to_run(&id, &body)?;
    Ok(Json(json!({ "message": "Test case added to test run" })))
}

async fn record_run_result<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    access::require_run(&service, &principal, &id, Role::Editor)?;
    service.record_run_result(&id, &body)?;
    Ok(Json(json!({ "message": "Test result recorded in run" })))
}

async fn list_result_defects<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::require_run(&service, &principal, &id, Role::Viewer)?;
    let defects = service.list_defects(&id, &case_id)?;
    Ok(Json(json!({ "defects": defects })))
}

async fn link_result_defect<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    access::require_run(&service, &principal, &id, Role::Editor)?;
    let link = service.link_defect_to_result(&id, &case_id, &body)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "message": "Defect linked to test result", "id": link.link_id })),
    ))
}

async fn unlink_result_defect<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, link_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::require_run(&service, &principal, &id, Role::Editor)?;
    service.unlink_defect_from_result(&id, &case_id, &link_id)?;
    Ok(Json(
        json!({ "message": "Defect unlinked from test result" }),
    ))
}

async fn import_junit_results<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<ImportSummary>, DomainError> {
    access::require_run(&service, &principal, &id, Role::Editor)?;
    let xml = std::str::from_utf8(&body)
        .map_err(|_| DomainError::invalid_request("JUnit XML must be valid UTF-8"))?;
    Ok(Json(service.import_junit_results(&id, xml)?))
}

async fn import_json_results<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<ImportSummary>, DomainError> {
    access::require_run(&service, &principal, &id, Role::Editor)?;
    Ok(Json(service.import_json_results(&id, &body)?))
}

async fn link_run_configuration<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    access::require_run(&service, &principal, &id, Role::Editor)?;
    service.link_configuration_to_run(&id, &body)?;
    Ok(Json(
        json!({ "message": "Test configuration linked to test run" }),
    ))
}

async fn unlink_run_configuration<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, config_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::require_run(&service, &principal, &id, Role::Editor)?;
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
            "/test_runs/{id}/results/{case_id}/defects",
            get(list_result_defects::<R>).post(link_result_defect::<R>),
        )
        .route(
            "/test_runs/{id}/results/{case_id}/defects/{link_id}",
            delete(unlink_result_defect::<R>),
        )
        .route(
            "/test_runs/{id}/import/junit",
            post(import_junit_results::<R>),
        )
        .route(
            "/test_runs/{id}/import/json",
            post(import_json_results::<R>),
        )
        .route(
            "/test_runs/{id}/configurations",
            post(link_run_configuration::<R>),
        )
        .route(
            "/test_runs/{id}/configurations/{config_id}",
            delete(unlink_run_configuration::<R>),
        )
}
