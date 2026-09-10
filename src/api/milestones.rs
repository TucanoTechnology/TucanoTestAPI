//! `/milestones` — progress derived from the runs it references.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};

use crate::{
    models::MilestoneProgress,
    storage::{Repository, Resource},
};

use super::{AppState, crud::crud_handlers, crud::prelude::*};

crud_handlers!(
    list_milestones,
    get_milestone,
    create_milestone,
    update_milestone,
    delete_milestone,
    Resource::Milestones
);

async fn get_milestone_progress<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
) -> Result<Json<MilestoneProgress>, DomainError> {
    Ok(Json(service.milestone_progress(&id)?))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
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
        .route(
            "/milestones/{id}/progress",
            get(get_milestone_progress::<R>),
        )
}
