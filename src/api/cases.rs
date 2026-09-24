//! `/test_cases` — a single executable check, with its attachments.
//!
//! A case has no top-level collection: it is created inside a project or a
//! suite, either through the parent-scoped routes or by placing an existing
//! case there. History addresses a case by its bare identifier and only works
//! while one parent owns it; attachments are reachable both ways, from the bare
//! identifier and from the parent a route names.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use crate::{
    domain::{
        ATTACHMENT_MEDIA_TYPE, DomainError, MAX_ATTACHMENT_BYTES, StoredAttachment,
        content_disposition, duplicate,
    },
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
    principal: Principal,
    Path(id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, DomainError> {
    access::require(&service, &principal, &id, Role::Viewer)?;
    let items = service.list_children_matching(&Parent::Project(id), Resource::Cases, &query)?;
    Ok(Json(json!(items)))
}

async fn create_project_case<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    access::guard_composition(
        &service,
        &principal,
        Resource::Cases,
        &id,
        &body,
        Role::Editor,
    )?;
    let composed = service.compose(Resource::Cases, &Parent::Project(id), &body)?;
    Ok(composed_response(&composed, "Test case"))
}

async fn delete_project_case<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    access::require(&service, &principal, &id, Role::Editor)?;
    service.delete_in(Resource::Cases, &Parent::Project(id), &case_id)?;
    Ok(Json(json!({ "message": "Test case deleted" })))
}

// --- attachments ------------------------------------------------------
//
// A case's files are addressed two ways. The routes that name the case by its
// bare identifier resolve it globally, so they answer a conflict while two
// parents hold the identifier; the routes that name the parent as well address
// that occurrence directly and always work. Both styles end in the same
// `*_in` call once the parent is resolved.

async fn upload_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Editor)?;
    upload_case_attachment(&service, &parent, &id, multipart).await
}

async fn upload_project_case_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = case_in_project(&service, &principal, id, &case_id, Role::Editor)?;
    upload_case_attachment(&service, &parent, &case_id, multipart).await
}

async fn upload_suite_case_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id)): Path<(String, String)>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = case_in_suite(&service, &principal, &id, &case_id, Role::Editor)?;
    upload_case_attachment(&service, &parent, &case_id, multipart).await
}

async fn download_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Response, DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Viewer)?;
    attachment_response(&service, &parent, &id, &filename)
}

async fn download_project_case_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, filename)): Path<(String, String, String)>,
) -> Result<Response, DomainError> {
    let parent = case_in_project(&service, &principal, id, &case_id, Role::Viewer)?;
    attachment_response(&service, &parent, &case_id, &filename)
}

async fn download_suite_case_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, filename)): Path<(String, String, String)>,
) -> Result<Response, DomainError> {
    let parent = case_in_suite(&service, &principal, &id, &case_id, Role::Viewer)?;
    attachment_response(&service, &parent, &case_id, &filename)
}

async fn delete_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, filename)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Editor)?;
    delete_case_attachment(&service, &parent, &id, &filename)
}

async fn delete_project_case_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, filename)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = case_in_project(&service, &principal, id, &case_id, Role::Editor)?;
    delete_case_attachment(&service, &parent, &case_id, &filename)
}

async fn delete_suite_case_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, filename)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = case_in_suite(&service, &principal, &id, &case_id, Role::Editor)?;
    delete_case_attachment(&service, &parent, &case_id, &filename)
}

async fn list_step_attachments<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, step_index)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Viewer)?;
    list_step_files(&service, &parent, &id, &step_index)
}

async fn list_project_case_step_attachments<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, step_index)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = case_in_project(&service, &principal, id, &case_id, Role::Viewer)?;
    list_step_files(&service, &parent, &case_id, &step_index)
}

async fn list_suite_case_step_attachments<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, step_index)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = case_in_suite(&service, &principal, &id, &case_id, Role::Viewer)?;
    list_step_files(&service, &parent, &case_id, &step_index)
}

async fn upload_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, step_index)): Path<(String, String)>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Editor)?;
    upload_step_file(&service, &parent, &id, &step_index, multipart).await
}

async fn upload_project_case_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, step_index)): Path<(String, String, String)>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = case_in_project(&service, &principal, id, &case_id, Role::Editor)?;
    upload_step_file(&service, &parent, &case_id, &step_index, multipart).await
}

async fn upload_suite_case_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, step_index)): Path<(String, String, String)>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let parent = case_in_suite(&service, &principal, &id, &case_id, Role::Editor)?;
    upload_step_file(&service, &parent, &case_id, &step_index, multipart).await
}

async fn delete_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, step_index, filename)): Path<(String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Editor)?;
    delete_step_file(&service, &parent, &id, &step_index, &filename)
}

async fn delete_project_case_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, step_index, filename)): Path<(String, String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = case_in_project(&service, &principal, id, &case_id, Role::Editor)?;
    delete_step_file(&service, &parent, &case_id, &step_index, &filename)
}

async fn delete_suite_case_step_attachment<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, case_id, step_index, filename)): Path<(String, String, String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = case_in_suite(&service, &principal, &id, &case_id, Role::Editor)?;
    delete_step_file(&service, &parent, &case_id, &step_index, &filename)
}

/// The parent a parent-scoped case route names, once the caller's role in that
/// parent's project is checked and the parent is proven to hold the case.
///
/// The case is read from the named parent rather than through the global
/// lookup, so an identifier two parents hold is addressed unambiguously and one
/// the parent does not hold is reported as absent.
fn case_in<R: Repository>(
    service: &AppState<R>,
    principal: &Principal,
    parent: Parent,
    case_id: &str,
    required: Role,
) -> Result<Parent, DomainError> {
    access::require(service, principal, parent.project(), required)?;
    service.document_in(Resource::Cases, &parent, case_id, "Test case not found")?;
    Ok(parent)
}

/// [`case_in`] for the routes that name a project.
fn case_in_project<R: Repository>(
    service: &AppState<R>,
    principal: &Principal,
    project: String,
    case_id: &str,
    required: Role,
) -> Result<Parent, DomainError> {
    case_in(
        service,
        principal,
        Parent::Project(project),
        case_id,
        required,
    )
}

/// [`case_in`] for the routes that name a suite, which the role check needs in
/// order to name the project the suite lives in.
fn case_in_suite<R: Repository>(
    service: &AppState<R>,
    principal: &Principal,
    suite: &str,
    case_id: &str,
    required: Role,
) -> Result<Parent, DomainError> {
    let parent = service.suite_parent(suite)?;
    case_in(service, principal, parent, case_id, required)
}

/// Stores an uploaded file in the addressed case folder.
async fn upload_case_attachment<R: Repository>(
    service: &AppState<R>,
    parent: &Parent,
    case_id: &str,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let (original_name, contents) = uploaded_file(&mut multipart).await?;
    let stored = service.store_attachment(parent, case_id, &original_name, &contents)?;
    Ok(upload_response(stored))
}

/// Stores an uploaded file in the addressed step's folder.
async fn upload_step_file<R: Repository>(
    service: &AppState<R>,
    parent: &Parent,
    case_id: &str,
    step_index: &str,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DomainError> {
    let (original_name, contents) = uploaded_file(&mut multipart).await?;
    let step_index = parse_step_index(step_index)?;
    let stored =
        service.store_step_attachment(parent, case_id, step_index, &original_name, &contents)?;
    Ok(upload_response(stored))
}

/// Answers a download with the stored bytes, served opaquely and named for the
/// client.
///
/// The body is `application/octet-stream` whatever the file is — the stored
/// media type stays in the case document's `mimeType` — and the
/// `Content-Disposition` names the file the uploader supplied, so a download
/// lands under a name a human recognises instead of the stored
/// `<suffix>-<original name>`.
fn attachment_response<R: Repository>(
    service: &AppState<R>,
    parent: &Parent,
    case_id: &str,
    filename: &str,
) -> Result<Response, DomainError> {
    let contents = service.read_attachment(parent, case_id, filename)?;
    let disposition = HeaderValue::from_str(&content_disposition(filename))
        .expect("the disposition is printable ASCII by construction");
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(ATTACHMENT_MEDIA_TYPE),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        contents,
    )
        .into_response())
}

/// Deletes one stored file from the addressed case folder.
fn delete_case_attachment<R: Repository>(
    service: &AppState<R>,
    parent: &Parent,
    case_id: &str,
    filename: &str,
) -> Result<Json<Value>, DomainError> {
    service.delete_attachment(parent, case_id, filename)?;
    Ok(deleted_response())
}

/// Lists the attachments one step of the addressed case carries.
fn list_step_files<R: Repository>(
    service: &AppState<R>,
    parent: &Parent,
    case_id: &str,
    step_index: &str,
) -> Result<Json<Value>, DomainError> {
    let step_index = parse_step_index(step_index)?;
    let attachments = service.list_step_attachments(parent, case_id, step_index)?;
    Ok(Json(json!(attachments)))
}

/// Deletes one stored file from the addressed step's folder.
fn delete_step_file<R: Repository>(
    service: &AppState<R>,
    parent: &Parent,
    case_id: &str,
    step_index: &str,
    filename: &str,
) -> Result<Json<Value>, DomainError> {
    let step_index = parse_step_index(step_index)?;
    service.delete_step_attachment(parent, case_id, step_index, filename)?;
    Ok(deleted_response())
}

/// Reads the one file part an upload request carries.
///
/// The part name is not inspected, but the part must carry a filename and a
/// readable body within [`MAX_ATTACHMENT_BYTES`]; each refusal is the one the
/// upload routes have always answered.
async fn uploaded_file(multipart: &mut Multipart) -> Result<(String, Bytes), DomainError> {
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
    Ok((original_name, contents))
}

/// The `201` answer every upload route gives for a stored file.
fn upload_response(stored: StoredAttachment) -> (StatusCode, Json<Value>) {
    (
        StatusCode::CREATED,
        Json(json!({
            "message": "File uploaded successfully",
            "filename": stored.filename,
            "originalName": stored.original_name,
            "size": stored.size,
        })),
    )
}

/// The `200` answer every delete route gives.
fn deleted_response() -> Json<Value> {
    Json(json!({ "message": "File deleted successfully" }))
}

async fn list_case_history<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path(id): Path<String>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Viewer)?;
    let history = service.list_case_history(&parent, &id)?;
    Ok(Json(json!(history)))
}

async fn read_case_revision<R: Repository>(
    State(service): State<AppState<R>>,
    principal: Principal,
    Path((id, version)): Path<(String, String)>,
) -> Result<Json<Value>, DomainError> {
    let parent = service.require_test_case(&id)?;
    access::require(&service, &principal, parent.project(), Role::Viewer)?;
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
        .route(
            "/projects/{id}/test_cases/{case_id}/attachments",
            post(upload_project_case_attachment::<R>),
        )
        .route(
            "/projects/{id}/test_cases/{case_id}/attachments/{filename}",
            get(download_project_case_attachment::<R>).delete(delete_project_case_attachment::<R>),
        )
        .route(
            "/projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
            get(list_project_case_step_attachments::<R>)
                .post(upload_project_case_step_attachment::<R>),
        )
        .route(
            "/projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}",
            delete(delete_project_case_step_attachment::<R>),
        )
        .route(
            "/test_suites/{id}/test_cases/{case_id}/attachments",
            post(upload_suite_case_attachment::<R>),
        )
        .route(
            "/test_suites/{id}/test_cases/{case_id}/attachments/{filename}",
            get(download_suite_case_attachment::<R>).delete(delete_suite_case_attachment::<R>),
        )
        .route(
            "/test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
            get(list_suite_case_step_attachments::<R>).post(upload_suite_case_step_attachment::<R>),
        )
        .route(
            "/test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}",
            delete(delete_suite_case_step_attachment::<R>),
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
