//! `/test_runs` — a point-in-time execution of suites and cases.
//!
//! A run is created inside a project, through `/projects/{id}/test_runs`; the
//! bare collection route stays registered to explain that, and every other run
//! route addresses a run by id and resolves the project holding it.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    routing::{delete, get, post, put},
};
use serde_json::{Value, json};

use crate::{
    domain::{DomainError, duplicate},
    models::ImportSummary,
    storage::{Parent, Repository, Resource},
};

use super::{
    AppState,
    crud::prelude::*,
    crud::{created_response, crud_handlers, duplicate_handler},
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

async fn list_project_runs<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require(&service, &principal, &id, Role::Viewer)?;
        let items = service.list_children_matching(&Parent::Project(id), Resource::Runs, &query)?;
        Ok(Json(json!(items)))
    })
    .await
}

async fn create_project_run<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<(StatusCode, Json<Value>), DomainError> {
        access::guard_project_create(&service, &principal, Resource::Runs, &id, &body)?;
        let created = service.create_in(Resource::Runs, &Parent::Project(id), &body)?;
        Ok(created_response("Test run", created.id))
    })
    .await
}

/// Removes the occurrence the caller named, so an identifier two projects hold
/// is deleted from the one the path says.
async fn delete_project_run<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, run_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require(&service, &principal, &id, Role::Editor)?;
        service.delete_in(Resource::Runs, &Parent::Project(id), &run_id)?;
        Ok(Json(json!({ "message": "Test run deleted" })))
    })
    .await
}

async fn add_suite_to_run<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
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
    })
    .await
}

async fn add_case_to_run<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
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
    })
    .await
}

async fn record_run_result<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        let stored = service.record_run_result(&id, &body)?;
        Ok(Json(json!({
            "message": "Test result recorded in run",
            // The result as stored, so a client paints the row without a
            // re-read: merged notes, the timestamp the API filled (#459).
            "result": serde_json::to_value(stored)
                .map_err(|e| DomainError::Internal(e.to_string()))?,
        })))
    })
    .await
}

async fn replace_run_result<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        let stored = service.replace_run_result(&id, &case_id, &body)?;
        Ok(Json(json!({
            "message": "Test result replaced in run",
            "result": serde_json::to_value(stored)
                .map_err(|e| DomainError::Internal(e.to_string()))?,
        })))
    })
    .await
}

async fn delete_run_result<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        service.delete_run_result(&id, &case_id)?;
        Ok(Json(json!({ "message": "Test result removed from run" })))
    })
    .await
}

async fn list_result_defects<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Viewer)?;
        // #460: the links are the case's, so the case's project must be in
        // view too — a run cannot be used to read a foreign case's defects.
        let parent = service.resolve_result_case(&id, &case_id)?;
        access::require(&service, &principal, parent.project(), Role::Viewer)?;
        let defects = service.case_defects(&parent, &case_id)?;
        Ok(Json(json!({ "defects": defects })))
    })
    .await
}

async fn link_result_defect<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<(StatusCode, Json<Value>), DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        let parent = service.resolve_result_case(&id, &case_id)?;
        access::require(&service, &principal, parent.project(), Role::Editor)?;
        let link = service.link_defect_to_case(&parent, &case_id, &body)?;
        Ok((
            StatusCode::CREATED,
            Json(json!({ "message": "Defect linked to test result", "id": link.link_id })),
        ))
    })
    .await
}

async fn unlink_result_defect<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, link_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        let parent = service.resolve_result_case(&id, &case_id)?;
        access::require(&service, &principal, parent.project(), Role::Editor)?;
        service.unlink_defect_from_case(&parent, &case_id, &link_id)?;
        Ok(Json(
            json!({ "message": "Defect unlinked from test result" }),
        ))
    })
    .await
}

async fn import_junit_results<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<ImportSummary>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<ImportSummary>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        let xml = std::str::from_utf8(&body)
            .map_err(|_| DomainError::invalid_request("JUnit XML must be valid UTF-8"))?;
        Ok(Json(service.import_junit_results(&id, xml)?))
    })
    .await
}

async fn import_json_results<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<ImportSummary>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<ImportSummary>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        Ok(Json(service.import_json_results(&id, &body)?))
    })
    .await
}

async fn link_run_configuration<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require_run_configuration(
            &service,
            &principal,
            &id,
            body.get("configId").and_then(Value::as_str),
            Role::Editor,
        )?;
        service.link_configuration_to_run(&id, &body)?;
        Ok(Json(
            json!({ "message": "Test configuration linked to test run" }),
        ))
    })
    .await
}

async fn unlink_run_configuration<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, config_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    // The storage phase is synchronous by design; park it on the blocking
    // pool so lock waits and fsyncs never occupy an async worker (#410).
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require_run(&service, &principal, &id, Role::Editor)?;
        service.unlink_configuration_from_run(&id, &config_id)?;
        Ok(Json(
            json!({ "message": "Test configuration unlinked from test run" }),
        ))
    })
    .await
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        // Retired: a run is created inside a project. The handler answers with
        // an explanation rather than a document.
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
            "/test_runs/{id}/results/{case_id}",
            put(replace_run_result::<R>).delete(delete_run_result::<R>),
        )
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
        .route(
            "/projects/{id}/test_runs",
            get(list_project_runs::<R>).post(create_project_run::<R>),
        )
        .route(
            "/projects/{id}/test_runs/{run_id}",
            delete(delete_project_run::<R>),
        )
}
