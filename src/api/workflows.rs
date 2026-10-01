//! `/workflows` — ordered plans that point at live cases and suites.
//!
//! A workflow is a project-scoped document naming an ordered list of
//! references to live cases and suites. Runs materialise the plan at
//! execution time; the workflow itself carries no snapshot.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::DomainError,
    storage::{Parent, Repository, Resource},
};

use super::{
    AppState,
    access,
    crud::prelude::*,
    crud::{composed_response, crud_handlers},
};

crud_handlers!(
    list_workflows,
    get_workflow,
    create_workflow,
    update_workflow,
    delete_workflow,
    Resource::Workflows
);

async fn create_project_workflow<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    super::on_blocking(move || -> Result<(StatusCode, Json<Value>), DomainError> {
        access::guard_create(&service, &principal, Resource::Workflows, &body)?;
        validate_workflow_steps(&service, &id, &body)?;
        let composed = service.compose(Resource::Workflows, &Parent::Project(id), &body)?;
        Ok(composed_response(&composed, "Workflow created"))
    })
    .await
}

/// Validates that every step target is reachable from the home project.
///
/// A case is reachable if it is held directly by the project or by one of its
/// suites. A suite is reachable if it is held by the project. A step that
/// names neither a reachable case nor a reachable suite is refused with
/// `invalid_request` naming the offending index.
fn validate_workflow_steps<R: Repository + 'static>(
    service: &AppState<R>,
    project_id: &str,
    body: &Value,
) -> Result<(), DomainError> {
    let Some(steps) = body.get("steps").and_then(Value::as_array) else {
        return Ok(());
    };
    let project_parent = Parent::Project(project_id.to_owned());
    // Collect suite IDs once, to avoid re-listing per case step.
    let suite_ids: Vec<String> = service.list_children(&project_parent, Resource::Suites)?;
    for (index, step) in steps.iter().enumerate() {
        let Some(step_obj) = step.as_object() else {
            return Err(DomainError::invalid_request(format!(
                "Step {index} is not an object"
            )));
        };
        let case_id = step_obj.get("testCaseId").and_then(Value::as_str);
        let suite_id = step_obj.get("suiteId").and_then(Value::as_str);
        match (case_id, suite_id) {
            (Some(case), None) => {
                // Case must be held directly by the project or by one of its suites.
                let direct = service
                    .document_in(Resource::Cases, &project_parent, case, "")
                    .is_ok();
                if direct {
                    continue;
                }
                let in_suite = suite_ids.iter().any(|sid| {
                    service
                        .document_in(
                            Resource::Cases,
                            &Parent::Suite {
                                project: project_id.to_owned(),
                                suite: sid.clone(),
                            },
                            case,
                            "",
                        )
                        .is_ok()
                });
                if !in_suite {
                    return Err(DomainError::invalid_request(format!(
                        "Step {index} names case `{case}` which is not reachable from project `{project_id}`"
                    )));
                }
            }
            (None, Some(suite)) => {
                // Suite must be held by the project.
                service.document_in(Resource::Suites, &project_parent, suite, "Test suite not found")?;
            }
            (Some(_), Some(_)) => {
                return Err(DomainError::invalid_request(format!(
                    "Step {index} names both a case and a suite; exactly one is required"
                )));
            }
            (None, None) => {
                return Err(DomainError::invalid_request(format!(
                    "Step {index} names neither a case nor a suite; exactly one is required"
                )));
            }
        }
    }
    Ok(())
}

async fn delete_project_workflow<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, workflow_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    super::on_blocking(move || -> Result<Json<Value>, DomainError> {
        access::require(&service, &principal, &id, crate::auth::Role::Editor)?;
        service.delete_in(Resource::Workflows, &Parent::Project(id), &workflow_id)?;
        Ok(Json(json!({ "message": "Workflow deleted" })))
    })
    .await
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        // Retired: a workflow is created inside a project. The handler
        // answers with an explanation rather than a document.
        .route(
            "/workflows",
            get(list_workflows::<R>).post(create_workflow::<R>),
        )
        .route(
            "/workflows/{id}",
            get(get_workflow::<R>)
                .put(update_workflow::<R>)
                .delete(delete_workflow::<R>),
        )
        .route(
            "/projects/{id}/workflows",
            get(list_workflows::<R>).post(create_project_workflow::<R>),
        )
        .route(
            "/projects/{id}/workflows/{workflow_id}",
            delete(delete_project_workflow::<R>),
        )
        .route(
            "/projects/{id}/workflows/{workflow_id}/run",
            post(run_from_workflow::<R>),
        )
}

async fn run_from_workflow<R: Repository + 'static>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, workflow_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    super::on_blocking(move || -> Result<(StatusCode, Json<Value>), DomainError> {
        access::require(&service, &principal, &id, crate::auth::Role::Editor)?;
        
        // Read the workflow.
        let workflow = service.document_in(
            Resource::Workflows,
            &Parent::Project(id.clone()),
            &workflow_id,
            "Workflow not found",
        )?;
        
        // Build run body from workflow steps.
        let steps = workflow.get("steps").and_then(Value::as_array).ok_or_else(|| {
            DomainError::invalid_request("Workflow has no steps")
        })?;
        
        let mut test_cases = Vec::new();
        let mut test_suites = Vec::new();
        
        for step in steps {
            if let Some(obj) = step.as_object() {
                if let Some(case_id) = obj.get("testCaseId").and_then(Value::as_str) {
                    test_cases.push(json!({"testCaseId": case_id}));
                } else if let Some(suite_id) = obj.get("suiteId").and_then(Value::as_str) {
                    test_suites.push(json!({"suiteId": suite_id}));
                }
            }
        }
        
        let run_name = body.get("name").and_then(Value::as_str).unwrap_or(&workflow_id);
        let mut run_body = json!({
            "name": run_name,
            "testCases": test_cases,
            "testSuites": test_suites,
            "sourceWorkflowId": workflow_id
        });
        
        // Add projects if specified.
        if let Some(projects) = body.get("projects") {
            run_body["projects"] = projects.clone();
        } else {
            run_body["projects"] = json!([{"projectId": id}]);
        }
        
        let created = service.create_in(Resource::Runs, &Parent::Project(id), &run_body)?;
        Ok((StatusCode::CREATED, Json(json!({
            "message": "Run created from workflow",
            "id": created.id
        }))))
    })
    .await
}
