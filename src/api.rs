use crate::models::{Milestone, MilestoneProgress, TestCase, TestCaseResult, TestRun, TestSuite};
use crate::repository::FileRepository;
use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

pub const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

type SharedRepository = Arc<FileRepository>;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListQuery {
    pub filter: Option<String>,
    pub tags: Option<String>,
}

pub fn router(repository: FileRepository) -> Router {
    let state = Arc::new(repository);
    Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi))
        .route("/api-docs", get(swagger_ui))
        .route("/api-docs/", get(swagger_ui))
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
        .route("/test_suites/{id}/test_cases", post(add_case_to_suite))
        .route(
            "/test_suites/{id}/test_cases/{case_id}",
            delete(remove_case_from_suite),
        )
        .route("/test_runs", get(list_runs).post(create_run))
        .route(
            "/test_runs/{id}",
            get(get_run).put(update_run).delete(delete_run),
        )
        .route("/test_runs/{id}/test_suites", post(add_suite_to_run))
        .route("/test_runs/{id}/test_cases", post(add_case_to_run))
        .route("/test_runs/{id}/results", post(record_run_result))
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
        .route("/milestones", get(list_milestones).post(create_milestone))
        .route(
            "/milestones/{id}",
            get(get_milestone)
                .put(update_milestone)
                .delete(delete_milestone),
        )
        .route("/milestones/{id}/progress", get(get_milestone_progress))
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

async fn swagger_ui() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Body::from(include_str!("../swagger.html")),
    )
        .into_response()
}

macro_rules! crud_handlers {
    ($list:ident, $get:ident, $create:ident, $update:ident, $delete:ident, $resource:literal) => {
        async fn $list(
            State(repo): State<SharedRepository>,
            Query(query): Query<ListQuery>,
        ) -> Response {
            list_resource(repo, $resource, query)
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
crud_handlers!(
    list_milestones,
    get_milestone,
    create_milestone,
    update_milestone,
    delete_milestone,
    "milestones"
);

fn list_resource(repo: SharedRepository, resource: &str, query: ListQuery) -> Response {
    match repo.list(resource) {
        Ok(mut items) => {
            if let Some(filter) = query.filter {
                let needle = filter.to_lowercase();
                items.retain(|item| item.to_lowercase().contains(&needle));
            }
            if let Some(tags_param) = query.tags {
                let requested_tags: Vec<String> = tags_param
                    .split(',')
                    .map(|t| t.trim().to_lowercase())
                    .collect();
                items.retain(|item| {
                    if let Ok(value) = repo.read(resource, item) {
                        if let Some(tags) = value.get("tags").and_then(|t| t.as_array()) {
                            let item_tags: Vec<String> = tags
                                .iter()
                                .filter_map(|t| t.as_str().map(|s| s.to_lowercase()))
                                .collect();
                            requested_tags.iter().any(|tag| item_tags.contains(tag))
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                });
            }
            Json(items).into_response()
        }
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
    } else if resource == "milestones" {
        required_string(&value, "name").map(|name| {
            let raw_id = required_string(&value, "milestoneId").unwrap_or(name);
            if raw_id.ends_with(".json") {
                raw_id
            } else {
                format!("{raw_id}.json")
            }
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

async fn add_case_to_suite(
    State(repo): State<SharedRepository>,
    Path(id): Path<String>,
    Json(value): Json<Value>,
) -> Response {
    let Some(target_case_id) = required_string(&value, "testCaseId") else {
        return bad_request("invalid_request", "Required field testCaseId is missing");
    };

    let suite_value = match repo.read("test_suites", &id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test suite not found");
        }
        Err(error) => return storage_error(error),
    };

    let mut suite: TestSuite = match serde_json::from_value(suite_value) {
        Ok(s) => s,
        Err(_) => return server_error("Stored suite JSON is invalid"),
    };

    let case_value = match repo.read("test_cases", &target_case_id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test case not found");
        }
        Err(error) => return storage_error(error),
    };

    let test_case: TestCase = match serde_json::from_value(case_value) {
        Ok(c) => c,
        Err(_) => return server_error("Stored case JSON is invalid"),
    };

    if suite
        .test_cases
        .iter()
        .any(|c| c.test_case_id == test_case.test_case_id || c.test_case_id == target_case_id)
    {
        return conflict("Test case is already in suite");
    }

    suite.test_cases.push(test_case);
    let updated_value = match serde_json::to_value(&suite) {
        Ok(v) => v,
        Err(_) => return server_error("Failed to serialize suite"),
    };

    match repo.write("test_suites", &id, &updated_value) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"message": "Test case added to suite"})),
        )
            .into_response(),
        Err(error) => storage_error(error),
    }
}

async fn remove_case_from_suite(
    State(repo): State<SharedRepository>,
    Path((id, case_id)): Path<(String, String)>,
) -> Response {
    let suite_value = match repo.read("test_suites", &id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test suite not found");
        }
        Err(error) => return storage_error(error),
    };

    let mut suite: TestSuite = match serde_json::from_value(suite_value) {
        Ok(s) => s,
        Err(_) => return server_error("Stored suite JSON is invalid"),
    };

    let original_len = suite.test_cases.len();
    suite.test_cases.retain(|c| c.test_case_id != case_id);

    if suite.test_cases.len() == original_len {
        return not_found("Test case not in suite");
    }

    let updated_value = match serde_json::to_value(&suite) {
        Ok(v) => v,
        Err(_) => return server_error("Failed to serialize suite"),
    };

    match repo.write("test_suites", &id, &updated_value) {
        Ok(()) => Json(json!({"message": "Test case removed from suite"})).into_response(),
        Err(error) => storage_error(error),
    }
}

async fn add_suite_to_run(
    State(repo): State<SharedRepository>,
    Path(id): Path<String>,
    Json(value): Json<Value>,
) -> Response {
    let Some(suite_id) = required_string(&value, "suiteId") else {
        return bad_request("invalid_request", "Required field suiteId is missing");
    };

    let run_value = match repo.read("test_runs", &id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test run not found");
        }
        Err(error) => return storage_error(error),
    };

    let mut run: TestRun = match serde_json::from_value(run_value) {
        Ok(r) => r,
        Err(_) => return server_error("Stored run JSON is invalid"),
    };

    let suite_value = match repo.read("test_suites", &suite_id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test suite not found");
        }
        Err(error) => return storage_error(error),
    };

    let suite: TestSuite = match serde_json::from_value(suite_value) {
        Ok(s) => s,
        Err(_) => return server_error("Stored suite JSON is invalid"),
    };

    let suites = run.test_suites.get_or_insert_with(Vec::new);
    if suites
        .iter()
        .any(|s| s.suite_id == suite.suite_id || s.suite_id == suite_id)
    {
        return conflict("Test suite is already in test run");
    }

    suites.push(suite);
    let updated_value = match serde_json::to_value(&run) {
        Ok(v) => v,
        Err(_) => return server_error("Failed to serialize test run"),
    };

    match repo.write("test_runs", &id, &updated_value) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"message": "Test suite added to test run"})),
        )
            .into_response(),
        Err(error) => storage_error(error),
    }
}

async fn add_case_to_run(
    State(repo): State<SharedRepository>,
    Path(id): Path<String>,
    Json(value): Json<Value>,
) -> Response {
    let Some(case_id) = required_string(&value, "testCaseId") else {
        return bad_request("invalid_request", "Required field testCaseId is missing");
    };

    let run_value = match repo.read("test_runs", &id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test run not found");
        }
        Err(error) => return storage_error(error),
    };

    let mut run: TestRun = match serde_json::from_value(run_value) {
        Ok(r) => r,
        Err(_) => return server_error("Stored run JSON is invalid"),
    };

    let case_value = match repo.read("test_cases", &case_id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test case not found");
        }
        Err(error) => return storage_error(error),
    };

    let test_case: TestCase = match serde_json::from_value(case_value) {
        Ok(c) => c,
        Err(_) => return server_error("Stored case JSON is invalid"),
    };

    let cases = run.test_cases.get_or_insert_with(Vec::new);
    if cases
        .iter()
        .any(|c| c.test_case_id == test_case.test_case_id || c.test_case_id == case_id)
    {
        return conflict("Test case is already in test run");
    }

    cases.push(test_case);
    let updated_value = match serde_json::to_value(&run) {
        Ok(v) => v,
        Err(_) => return server_error("Failed to serialize test run"),
    };

    match repo.write("test_runs", &id, &updated_value) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"message": "Test case added to test run"})),
        )
            .into_response(),
        Err(error) => storage_error(error),
    }
}

async fn record_run_result(
    State(repo): State<SharedRepository>,
    Path(id): Path<String>,
    Json(value): Json<Value>,
) -> Response {
    let Some(test_case_id) = required_string(&value, "testCaseId") else {
        return bad_request("invalid_request", "Required field testCaseId is missing");
    };
    let Some(status_str) = required_string(&value, "status") else {
        return bad_request("invalid_request", "Required field status is missing");
    };

    let valid_statuses = ["Passed", "Failed", "Blocked", "Untested", "Retest"];
    if !valid_statuses.contains(&status_str.as_str()) {
        return bad_request(
            "invalid_status",
            "Status must be Passed, Failed, Blocked, Untested, or Retest",
        );
    }

    let run_value = match repo.read("test_runs", &id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Test run not found");
        }
        Err(error) => return storage_error(error),
    };

    let mut run: TestRun = match serde_json::from_value(run_value) {
        Ok(r) => r,
        Err(_) => return server_error("Stored run JSON is invalid"),
    };

    let timestamp = required_string(&value, "timestamp").unwrap_or_else(current_iso_timestamp);
    let notes = required_string(&value, "notes");

    let new_result = TestCaseResult {
        test_case_id: test_case_id.clone(),
        status: status_str,
        timestamp,
        notes,
        attachments: None,
    };

    let results = run.results.get_or_insert_with(Vec::new);
    if let Some(existing) = results.iter_mut().find(|r| r.test_case_id == test_case_id) {
        *existing = new_result;
    } else {
        results.push(new_result);
    }

    let updated_value = match serde_json::to_value(&run) {
        Ok(v) => v,
        Err(_) => return server_error("Failed to serialize test run"),
    };

    match repo.write("test_runs", &id, &updated_value) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"message": "Test result recorded in run"})),
        )
            .into_response(),
        Err(error) => storage_error(error),
    }
}

async fn get_milestone_progress(
    State(repo): State<SharedRepository>,
    Path(id): Path<String>,
) -> Response {
    let milestone_value = match repo.read("milestones", &id) {
        Ok(v) => v,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Milestone not found");
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            return bad_request("invalid_id", "Invalid resource ID");
        }
        Err(error) => return storage_error(error),
    };

    let milestone: Milestone = match serde_json::from_value(milestone_value) {
        Ok(m) => m,
        Err(_) => return server_error("Stored milestone JSON is invalid"),
    };

    let run_ids = milestone.test_run_ids.unwrap_or_default();
    let mut total_cases = 0;
    let mut passed = 0;
    let mut failed = 0;
    let mut blocked = 0;
    let mut untested = 0;
    let mut retest = 0;

    for run_id in run_ids {
        let Ok(run_value) = repo.read("test_runs", &run_id) else {
            continue;
        };
        let Ok(run) = serde_json::from_value::<TestRun>(run_value) else {
            continue;
        };
        if let Some(cases) = run.test_cases {
            total_cases += cases.len();
        }
        if let Some(results) = run.results {
            for res in results {
                match res.status.as_str() {
                    "Passed" => passed += 1,
                    "Failed" => failed += 1,
                    "Blocked" => blocked += 1,
                    "Untested" => untested += 1,
                    "Retest" => retest += 1,
                    _ => {}
                }
            }
        }
    }

    if total_cases == 0 {
        total_cases = passed + failed + blocked + untested + retest;
    }

    let pass_percentage = if total_cases > 0 {
        (passed as f64 / total_cases as f64) * 100.0
    } else {
        0.0
    };

    let progress = MilestoneProgress {
        milestone_id: milestone.milestone_id,
        total_cases,
        passed,
        failed,
        blocked,
        untested,
        retest,
        pass_percentage,
    };

    Json(progress).into_response()
}

fn current_iso_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
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
