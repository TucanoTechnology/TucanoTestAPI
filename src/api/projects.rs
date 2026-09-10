//! `/projects` — the container a suite or a case lives in.

use axum::{
    Router,
    routing::{get, post},
};

use crate::{domain::duplicate, storage::Resource};

use super::{
    AppState,
    crud::prelude::*,
    crud::{crud_handlers, duplicate_handler},
};

crud_handlers!(
    list_projects,
    get_project,
    create_project,
    update_project,
    delete_project,
    Resource::Projects
);

duplicate_handler!(duplicate_project, duplicate::PROJECT);

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        .route(
            "/projects",
            get(list_projects::<R>).post(create_project::<R>),
        )
        .route(
            "/projects/{id}",
            get(get_project::<R>)
                .put(update_project::<R>)
                .delete(delete_project::<R>),
        )
        .route("/projects/{id}/duplicate", post(duplicate_project::<R>))
}
