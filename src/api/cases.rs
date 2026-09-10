//! `/test_cases` — a single executable check, with its attachments.

use axum::{
    Json, Router,
    extract::{Multipart, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::{DomainError, MAX_ATTACHMENT_BYTES, duplicate},
    storage::{Repository, Resource},
};

use super::{
    AppState,
    crud::prelude::*,
    crud::{crud_handlers, duplicate_handler},
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

async fn upload_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    service.require_test_case(&id)?;

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

    let stored = service.store_attachment(&id, &original_name, &contents)?;
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
    let contents = service.read_attachment(&id, &filename)?;
    Ok(([(header::CONTENT_TYPE, mime_type(&filename))], contents).into_response())
}

async fn delete_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    service.delete_attachment(&id, &filename)?;
    Ok(Json(json!({ "message": "File deleted successfully" })))
}

pub(crate) fn routes<R: Repository + 'static>() -> Router<AppState<R>> {
    Router::new()
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

fn mime_type(filename: &str) -> &'static str {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}
