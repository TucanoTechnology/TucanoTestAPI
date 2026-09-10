//! `/configurations` — the environments a run can be executed against.

use axum::{Router, routing::get};

use crate::storage::{Repository, Resource};

use super::{AppState, crud::crud_handlers, crud::prelude::*};

crud_handlers!(
    list_configurations,
    get_configuration,
    create_configuration,
    update_configuration,
    delete_configuration,
    Resource::Configurations
);

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
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
}
