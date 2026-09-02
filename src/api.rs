use crate::repository::FileRepository;
use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

pub const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

type SharedRepository = Arc<FileRepository>;

pub fn router(repository: FileRepository) -> Router {
    let state = Arc::new(repository);
    Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi))
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{id}",
            get(get_project).put(update_project).delete(delete_project),
        )
        .route("/test_suites", get(list_suites).post(create_suite))
        .route(
            "/test_suites/{id}",
            get(get_suite).put(update_suite).delete(delete_suite),
        )
        .route("/test_runs", get(list_runs).post(create_run))
        .route(
            "/test_runs/{id}",
            get(get_run).put(update_run).delete(delete_run),
        )
        .route("/test_cases", get(list_cases).post(create_case))
        .route(
            "/test_cases/{id}",
            get(get_case).put(update_case).delete(delete_case),
        )
        .route("/test_cases/{id}/attachments", post(upload_attachment))
        .route(
            "/test_cases/{id}/attachments/{filename}",
            get(download_attachment).delete(delete_attachment),
        )
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "storage": "filesystem"}))
}

async fn openapi() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(include_str!("../openapi.json")),
    )
        .into_response()
}

macro_rules! crud_handlers {
    ($list:ident, $get:ident, $create:ident, $update:ident, $delete:ident, $resource:literal) => {
        async fn $list(State(repo): State<SharedRepository>) -> Response {
            list_resource(repo, $resource)
        }
        async fn $get(State(repo): State<SharedRepository>, Path(id): Path<String>) -> Response {
            get_resource(repo, $resource, id)
        }
        async fn $create(
            State(repo): State<SharedRepository>,
            Json(value): Json<Value>,
        ) -> Response {
            create_resource(repo, $resource, value)
        }
        async fn $update(
            State(repo): State<SharedRepository>,
            Path(id): Path<String>,
            Json(value): Json<Value>,
        ) -> Response {
            update_resource(repo, $resource, id, value)
        }
        async fn $delete(State(repo): State<SharedRepository>, Path(id): Path<String>) -> Response {
            delete_resource(repo, $resource, id)
        }
    };
}

crud_handlers!(
    list_projects,
    get_project,
    create_project,
    update_project,
    delete_project,
    "projects"
);
crud_handlers!(
    list_suites,
    get_suite,
    create_suite,
    update_suite,
    delete_suite,
    "test_suites"
);
crud_handlers!(
    list_runs,
    get_run,
    create_run,
    update_run,
    delete_run,
    "test_runs"
);
crud_handlers!(
    list_cases,
    get_case,
    create_case,
    update_case,
    delete_case,
    "test_cases"
);

fn list_resource(repo: SharedRepository, resource: &str) -> Response {
    match repo.list(resource) {
        Ok(items) => Json(items).into_response(),
        Err(error) => storage_error(error),
    }
}

fn get_resource(repo: SharedRepository, resource: &str, id: String) -> Response {
    match repo.read(resource, &id) {
        Ok(value) => Json(value).into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            bad_request("invalid_id", "Invalid resource ID")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            not_found("Resource not found")
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            server_error("Stored JSON is invalid")
        }
        Err(error) => storage_error(error),
    }
}

fn create_resource(repo: SharedRepository, resource: &str, value: Value) -> Response {
    let id = if resource == "test_cases" {
        required_string(&value, "testCaseId").and_then(|id| {
            required_string(&value, "title")
                .zip(required_string(&value, "expectedResult"))
                .map(|_| id)
        })
    } else {
        required_string(&value, "name").map(|name| format!("{name}.json"))
    };
    let Some(id) = id else {
        return bad_request("invalid_request", "Required fields are missing");
    };
    match repo.exists(resource, &id) {
        Ok(true) => conflict("Resource already exists"),
        Ok(false) => match repo.write(resource, &id, &value) {
            Ok(()) => (
                StatusCode::CREATED,
                Json(json!({"message": "Resource created", "id": id})),
            )
                .into_response(),
            Err(error) => storage_error(error),
        },
        Err(error) => storage_error(error),
    }
}

fn update_resource(repo: SharedRepository, resource: &str, id: String, value: Value) -> Response {
    match repo.exists(resource, &id) {
        Ok(false) => not_found("Resource not found"),
        Ok(true) => match repo.write(resource, &id, &value) {
            Ok(()) => Json(json!({"message": "Resource updated"})).into_response(),
            Err(error) => storage_error(error),
        },
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            bad_request("invalid_id", "Invalid resource ID")
        }
        Err(error) => storage_error(error),
    }
}

fn delete_resource(repo: SharedRepository, resource: &str, id: String) -> Response {
    match repo.delete(resource, &id) {
        Ok(()) => Json(json!({"message": "Resource deleted"})).into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            not_found("Resource not found")
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            bad_request("invalid_id", "Invalid resource ID")
        }
        Err(error) => storage_error(error),
    }
}

async fn upload_attachment(
    State(repo): State<SharedRepository>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Response {
    match repo.exists("test_cases", &id) {
        Ok(false) => return not_found("Test case not found"),
        Err(error) => return storage_error(error),
        Ok(true) => {}
    }
    let Some(field) = (match multipart.next_field().await {
        Ok(field) => field,
        Err(_) => return bad_request("invalid_multipart", "Invalid multipart request"),
    }) else {
        return bad_request("missing_file", "No file uploaded");
    };
    let Some(original_name) = field.file_name().map(str::to_owned) else {
        return bad_request("missing_file", "No file uploaded");
    };
    let Ok(contents) = field.bytes().await else {
        return bad_request("invalid_multipart", "Unable to read uploaded file");
    };
    if contents.len() > MAX_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(
                json!({"error":{"code":"payload_too_large","message":"Attachment exceeds 50 MiB"}}),
            ),
        )
            .into_response();
    }
    let filename = format!("{}-{}", unique_suffix(), original_name);
    match repo.save_attachment(&id, &filename, &contents) {
        Ok(()) => (StatusCode::CREATED, Json(json!({"message":"File uploaded successfully","filename":filename,"originalName":original_name,"size":contents.len()}))).into_response(),
        Err(error) => storage_error(error),
    }
}

async fn download_attachment(
    State(repo): State<SharedRepository>,
    Path((id, filename)): Path<(String, String)>,
) -> Response {
    match repo.read_attachment(&id, &filename) {
        Ok(contents) => ([(header::CONTENT_TYPE, mime_type(&filename))], contents).into_response(),
        Err(error)
            if error.kind() == std::io::ErrorKind::InvalidInput
                || error.kind() == std::io::ErrorKind::NotFound =>
        {
            not_found("File not found")
        }
        Err(error) => storage_error(error),
    }
}

async fn delete_attachment(
    State(repo): State<SharedRepository>,
    Path((id, filename)): Path<(String, String)>,
) -> Response {
    match repo.delete_attachment(&id, &filename) {
        Ok(()) => Json(json!({"message":"File deleted successfully"})).into_response(),
        Err(error)
            if error.kind() == std::io::ErrorKind::InvalidInput
                || error.kind() == std::io::ErrorKind::NotFound =>
        {
            not_found("File not found")
        }
        Err(error) => storage_error(error),
    }
}

fn required_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
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
fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
        .into_response()
}
fn bad_request(code: &str, message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, code, message)
}
fn not_found(message: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", message)
}
fn conflict(message: &str) -> Response {
    error_response(StatusCode::CONFLICT, "conflict", message)
}
fn server_error(message: &str) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "storage_error", message)
}
fn storage_error(error: std::io::Error) -> Response {
    match error.kind() {
        std::io::ErrorKind::NotFound => not_found("Resource not found"),
        std::io::ErrorKind::InvalidInput => bad_request("invalid_request", "Invalid request"),
        _ => server_error("Storage operation failed"),
    }
}
