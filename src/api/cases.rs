//! `/test_cases` — a single executable check, with its attachments.
//!
//! A case has no top-level collection: it is created inside a project or a
//! suite, either through the parent-scoped routes or by placing an existing
//! case there. Attachment routes address a case by its bare identifier and only
//! work when one parent owns it.

use axum::{
    Json, Router,
    extract::{Multipart, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::{DomainError, MAX_ATTACHMENT_BYTES, duplicate, mime_type},
    storage::{Parent, Repository, Resource},
};

use super::{
    AppState,
    crud::prelude::*,
    crud::{composed_response, crud_handlers, duplicate_handler},
};

crud_handlers!(
    list_test_cases,
    get_test_case,
    create_test_case,
    update_test_case,
    delete_test_case,
    Resource::Cases
);

duplicate_handler!(duplicate_test_case, duplicate::CASE);

async fn list_project_cases<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    let items = service.list_children(&Parent::Project(id), Resource::Cases)?;
    Ok(Json(json!(items)))
}

async fn create_project_case<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let composed = service.compose(Resource::Cases, &Parent::Project(id), &body)?;
    Ok(composed_response(&composed, "Test case"))
}

async fn delete_project_case<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    service.delete_in(Resource::Cases, &Parent::Project(id), &case_id)?;
    Ok(Json(json!({ "message": "Test case deleted" })))
}

async fn upload_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = service.require_test_case(&id)?;

    let field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => return Err(missing_file()),
        Err(_) => return Err(invalid_multipart("Invalid multipart request")),
    };
    let Some(original_name) = field.file_name().map(str::to_owned) else {
        return Err(missing_file());
    };
    let Ok(contents) = field.bytes().await else {
        return Err(invalid_multipart("Unable to read uploaded file"));
    };
    if contents.len() > MAX_ATTACHMENT_BYTES {
        return Err(DomainError::PayloadTooLarge);
    }

    let stored = service.store_attachment(&parent, &id, &original_name, &contents)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "message": "File uploaded successfully",
            "filename": stored.filename,
            "originalName": stored.original_name,
            "size": stored.size,
        })),
    ))
}

async fn download_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Response, DomainError> {
    let parent = service.require_test_case(&id)?;
    let contents = service.read_attachment(&parent, &id, &filename)?;
    Ok(([(header::CONTENT_TYPE, mime_type(&filename))], contents).into_response())
}

async fn delete_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    service.delete_attachment(&parent, &id, &filename)?;
    Ok(Json(json!({ "message": "File deleted successfully" })))
}

async fn list_step_attachments<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, step_index)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    let step_index = parse_step_index(&step_index)?;
    let attachments = service.list_step_attachments(&parent, &id, step_index)?;
    Ok(Json(json!(attachments)))
}

async fn upload_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, step_index)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = service.require_test_case(&id)?;
    let step_index = parse_step_index(&step_index)?;

    let field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => return Err(missing_file()),
        Err(_) => return Err(invalid_multipart("Invalid multipart request")),
    };
    let Some(original_name) = field.file_name().map(str::to_owned) else {
        return Err(missing_file());
    };
    let Ok(contents) = field.bytes().await else {
        return Err(invalid_multipart("Unable to read uploaded file"));
    };
    if contents.len() > MAX_ATTACHMENT_BYTES {
        return Err(DomainError::PayloadTooLarge);
    }

    let stored =
        service.store_step_attachment(&parent, &id, step_index, &original_name, &contents)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "message": "File uploaded successfully",
            "filename": stored.filename,
            "originalName": stored.original_name,
            "size": stored.size,
        })),
    ))
}

async fn delete_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, step_index, filename)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    let step_index = parse_step_index(&step_index)?;
    service.delete_step_attachment(&parent, &id, step_index, &filename)?;
    Ok(Json(json!({ "message": "File deleted successfully" })))
}

async fn list_case_history<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    let history = service.list_case_history(&parent, &id)?;
    Ok(Json(json!(history)))
}

async fn read_case_revision<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, version)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    let version = parse_version(&version)?;
    Ok(Json(service.read_case_revision(&parent, &id, version)?))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
        // Retired: a case is created inside a project or a suite. The handler
        // answers with an explanation rather than a document.
        .route(
            "/test_cases",
            get(list_test_cases::<R>).post(create_test_case::<R>),
        )
        .route(
            "/test_cases/{id}",
            get(get_test_case::<R>)
                .put(update_test_case::<R>)
                .delete(delete_test_case::<R>),
        )
        .route("/test_cases/{id}/duplicate", post(duplicate_test_case::<R>))
        .route("/test_cases/{id}/attachments", post(upload_attachment::<R>))
        .route(
            "/test_cases/{id}/attachments/{filename}",
            get(download_attachment::<R>).delete(delete_attachment::<R>),
        )
        .route(
            "/test_cases/{id}/steps/{step_index}/attachments",
            get(list_step_attachments::<R>).post(upload_step_attachment::<R>),
        )
        .route(
            "/test_cases/{id}/steps/{step_index}/attachments/{filename}",
            delete(delete_step_attachment::<R>),
        )
        .route("/test_cases/{id}/history", get(list_case_history::<R>))
        .route(
            "/test_cases/{id}/history/{version}",
            get(read_case_revision::<R>),
        )
        .route(
            "/projects/{id}/test_cases",
            get(list_project_cases::<R>).post(create_project_case::<R>),
        )
        .route(
            "/projects/{id}/test_cases/{case_id}",
            delete(delete_project_case::<R>),
        )
}

fn invalid_multipart(message: &str) -> DomainError {
    DomainError::InvalidRequest {
        code: "invalid_multipart",
        message: message.to_owned(),
    }
}

fn missing_file() -> DomainError {
    DomainError::InvalidRequest {
        code: "missing_file",
        message: "No file uploaded".to_owned(),
    }
}

fn parse_step_index(value: &str) -> Result<usize, DomainError> {
    value.parse::<usize>().map_err(|_| {
        DomainError::invalid_request(format!(
            "Step index `{value}` is not a non-negative integer"
        ))
    })
}

fn parse_version(value: &str) -> Result<u64, DomainError> {
    match value.parse::<u64>() {
        Ok(version) if version >= 1 => Ok(version),
        _ => Err(DomainError::invalid_request(format!(
            "Version `{value}` is not a positive integer"
        ))),
    }
}
