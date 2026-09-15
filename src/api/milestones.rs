//! `/milestones` — progress derived from the runs it references.
//!
//! A milestone is created inside a project, through
//! `/projects/{id}/milestones`; the bare collection route stays registered to
//! explain that, and the document routes address a milestone by id and resolve
//! the project holding it.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::{DomainError, duplicate},
    models::MilestoneProgress,
    storage::{Parent, Repository, Resource},
};

use super::{
    AppState,
    crud::prelude::*,
    crud::{created_response, crud_handlers, duplicate_handler},
};

crud_handlers!(
    list_milestones,
    get_milestone,
    create_milestone,
    update_milestone,
    delete_milestone,
    Resource::Milestones
);

duplicate_handler!(duplicate_milestone, duplicate::MILESTONE);

async fn list_project_milestones<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    access::require(&service, &principal, &id, Role::Viewer)?;
    let items = service.list_children(&Parent::Project(id), Resource::Milestones)?;
    Ok(Json(json!(items)))
}

async fn create_project_milestone<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    access::guard_project_create(&service, &principal, Resource::Milestones, &id, &body)?;
    let created = service.create_in(Resource::Milestones, &Parent::Project(id), &body)?;
    Ok(created_response("Milestone", created.id))
}

/// Removes the occurrence the caller named, so an identifier two projects hold
/// is deleted from the one the path says.
async fn delete_project_milestone<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, milestone_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::require(&service, &principal, &id, Role::Owner)?;
    service.delete_in(Resource::Milestones, &Parent::Project(id), &milestone_id)?;
    Ok(Json(json!({ "message": "Milestone deleted" })))
}

async fn get_milestone_progress<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
) -> Result<Json<MilestoneProgress>, DomainError> {
    access::guard_get(&service, &principal, Resource::Milestones, &id)?;
    Ok(Json(service.milestone_progress(&id)?))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        // Retired: a milestone is created inside a project. The handler answers
        // with an explanation rather than a document.
        .route(
            "/milestones",
            get(list_milestones::<R>).post(create_milestone::<R>),
        )
        .route(
            "/milestones/{id}",
            get(get_milestone::<R>)
                .put(update_milestone::<R>)
                .delete(delete_milestone::<R>),
        )
        .route("/milestones/{id}/duplicate", post(duplicate_milestone::<R>))
        .route(
            "/milestones/{id}/progress",
            get(get_milestone_progress::<R>),
        )
        .route(
            "/projects/{id}/milestones",
            get(list_project_milestones::<R>).post(create_project_milestone::<R>),
        )
        .route(
            "/projects/{id}/milestones/{milestone_id}",
            delete(delete_project_milestone::<R>),
        )
}
