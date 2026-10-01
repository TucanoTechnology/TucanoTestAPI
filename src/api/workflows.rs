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
        let composed = service.compose(Resource::Workflows, &Parent::Project(id), &body)?;
        Ok(composed_response(&composed, "Workflow created"))
    })
    .await
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
}
