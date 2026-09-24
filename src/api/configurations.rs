//! `/configurations` — the environments a run can be executed against.
//!
//! A configuration belongs to the project it is created in, through
//! `/projects/{id}/configurations`; the bare collection route stays registered
//! to explain that, and the document routes address a configuration by id and
//! resolve the project holding it. A run may still link a configuration from
//! any project the caller reaches.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get},
};
use serde_json::{Value, json};

use crate::{
    domain::DomainError,
    storage::{Parent, Repository, Resource},
};

use super::{
    AppState,
    crud::prelude::*,
    crud::{created_response, crud_handlers},
};

crud_handlers!(
    list_configurations,
    get_configuration,
    create_configuration,
    update_configuration,
    delete_configuration,
    Resource::Configurations
);

async fn list_project_configurations<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    access::require(&service, &principal, &id, Role::Viewer)?;
    let items = service.list_children(&Parent::Project(id), Resource::Configurations)?;
    Ok(Json(json!(items)))
}

async fn create_project_configuration<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    access::guard_project_create(&service, &principal, Resource::Configurations, &id, &body)?;
    let created = service.create_in(Resource::Configurations, &Parent::Project(id), &body)?;
    Ok(created_response("Test configuration", created.id))
}

/// Removes the occurrence the caller named, so an identifier two projects hold
/// is deleted from the one the path says.
async fn delete_project_configuration<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, config_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::require(&service, &principal, &id, Role::Editor)?;
    service.delete_in(Resource::Configurations, &Parent::Project(id), &config_id)?;
    Ok(Json(json!({ "message": "Test configuration deleted" })))
}

/// The environments the caller can execute a run against: the distinct names of
/// every configuration it reaches, sorted so the GUI can render them as they
/// arrive.
///
/// A listing, not a dereference, so a project outside the caller's grants is
/// filtered out of the result rather than refusing the request.
async fn list_environments<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
) -> Result<Json<Vec<String>>, DomainError> {
    let reachable = access::scope(service.auth(), &principal)?;
    let reachable: Option<Vec<String>> = reachable.map(|set| set.into_iter().collect());
    Ok(Json(service.environment_names(reachable.as_deref())?))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        // Retired: a configuration is created inside a project. The handler
        // answers with an explanation rather than a document.
        .route(
            "/configurations",
            get(list_configurations::<R>).post(create_configuration::<R>),
        )
        .route(
            "/configurations/{id}",
            get(get_configuration::<R>)
                .put(update_configuration::<R>)
                .delete(delete_configuration::<R>),
        )
        .route("/environments", get(list_environments::<R>))
        .route(
            "/projects/{id}/configurations",
            get(list_project_configurations::<R>).post(create_project_configuration::<R>),
        )
        .route(
            "/projects/{id}/configurations/{config_id}",
            delete(delete_project_configuration::<R>),
        )
}
