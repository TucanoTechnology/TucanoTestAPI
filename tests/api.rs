use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use tucano_test::{api, repository::FileRepository};

const BOUNDARY: &str = "tucanotestboundary";

fn test_app() -> (TempDir, Router) {
    let directory = TempDir::new().expect("temp dir");
    let repository = FileRepository::new(directory.path()).expect("repository");
    (directory, api::router(repository))
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    (status, bytes.to_vec())
}

async fn send_json(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let (status, bytes) = send(app, request).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

fn json_request(method: &str, uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

fn multipart_request(uri: &str, filename: &str, contents: &[u8]) -> Request<Body> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    body.extend_from_slice(contents);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

    Request::builder()
        .method("POST")
        .uri(uri)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .expect("request")
}

async fn create_test_case(app: &Router, id: &str) {
    let (status, _) = send_json(
        app,
        json_request(
            "POST",
            "/test_cases",
            &json!({"testCaseId": id, "title": "Upload", "expectedResult": "Stored"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn health_reports_filesystem_storage() {
    let (_directory, app) = test_app();
    let (status, body) = send_json(&app, get("/health")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["storage"], "filesystem");
}

#[tokio::test]
async fn openapi_document_describes_every_route() {
    let (_directory, app) = test_app();
    let (status, body) = send_json(&app, get("/openapi.json")).await;

    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"].as_object().expect("paths object");
    for route in [
        "/projects",
        "/projects/{id}",
        "/test_suites",
        "/test_suites/{id}",
        "/test_runs",
        "/test_runs/{id}",
        "/test_cases",
        "/test_cases/{id}",
        "/test_cases/{id}/attachments",
        "/test_cases/{id}/attachments/{filename}",
        "/health",
    ] {
        assert!(
            paths.contains_key(route),
            "missing documented route: {route}"
        );
    }
}

#[tokio::test]
async fn swagger_ui_is_served_with_and_without_trailing_slash() {
    let (_directory, app) = test_app();

    for uri in ["/api-docs", "/api-docs/"] {
        let (status, bytes) = send(&app, get(uri)).await;
        let html = String::from_utf8(bytes).expect("utf8 html");

        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains("SwaggerUIBundle"), "{uri}");
        assert!(html.contains("/openapi.json"), "{uri}");
    }
}

#[tokio::test]
async fn projects_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"projectId": "P-001", "name": "checkout", "testSuites": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "checkout.json");

    let (status, listing) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["checkout.json"]));

    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["projectId"], "P-001");

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/checkout.json",
            &json!({"projectId": "P-002", "name": "checkout", "testSuites": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(updated["projectId"], "P-002");

    let (status, _) = send_json(&app, delete("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_suites_and_runs_support_the_crud_lifecycle() {
    let (_directory, app) = test_app();

    for (collection, id) in [("/test_suites", "regression"), ("/test_runs", "nightly")] {
        let (status, created) =
            send_json(&app, json_request("POST", collection, &json!({"name": id}))).await;
        assert_eq!(status, StatusCode::CREATED, "{collection}");
        assert_eq!(created["id"], format!("{id}.json"), "{collection}");

        let item = format!("{collection}/{id}.json");
        let (status, stored) = send_json(&app, get(&item)).await;
        assert_eq!(status, StatusCode::OK, "{collection}");
        assert_eq!(stored["name"], id, "{collection}");

        let (status, _) = send_json(&app, delete(&item)).await;
        assert_eq!(status, StatusCode::OK, "{collection}");
    }
}

#[tokio::test]
async fn test_cases_are_created_from_their_identifier() {
    let (_directory, app) = test_app();

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/test_cases",
            &json!({"testCaseId": "TC-001", "title": "Login", "expectedResult": "Authenticated"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "TC-001");

    let (status, listing) = send_json(&app, get("/test_cases")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["TC-001"]));

    let (status, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["title"], "Login");

    let (status, _) = send_json(&app, delete("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn creating_resources_requires_the_documented_fields() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"description": "no name"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_cases",
            &json!({"testCaseId": "TC-001", "title": "No result"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": ""})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn duplicate_resources_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    let payload = json!({"name": "checkout"});

    let (status, _) = send_json(&app, json_request("POST", "/projects", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(&app, json_request("POST", "/projects", &payload)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn missing_resources_return_a_stable_error_envelope() {
    let (_directory, app) = test_app();

    for request in [
        get("/projects/missing.json"),
        delete("/projects/missing.json"),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
        assert!(body["error"]["message"].is_string());
    }

    let (status, _) = send_json(
        &app,
        json_request("PUT", "/projects/missing.json", &json!({"name": "missing"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn malformed_request_bodies_are_rejected() {
    let (_directory, app) = test_app();

    let request = Request::builder()
        .method("POST")
        .uri("/projects")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{ not json"))
        .expect("request");

    let (status, _) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn identifiers_cannot_escape_the_storage_root() {
    let (_directory, app) = test_app();

    for uri in [
        "/projects/..%2F..%2Fescape.json",
        "/projects/nested%2Fchild.json",
        "/test_cases/..%2Fescape",
    ] {
        let (status, _) = send(&app, get(uri)).await;
        assert!(
            status.is_client_error(),
            "traversal attempt should be rejected: {uri} returned {status}"
        );
    }
}

#[tokio::test]
async fn attachments_support_upload_download_and_delete() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (status, uploaded) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "notes.txt", b"evidence"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(uploaded["originalName"], "notes.txt");
    assert_eq!(uploaded["size"], 8);

    let filename = uploaded["filename"].as_str().expect("stored filename");
    assert!(filename.ends_with("-notes.txt"));

    let uri = format!("/test_cases/TC-001/attachments/{filename}");
    let (status, contents) = send(&app, get(&uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(contents, b"evidence");

    let (status, _) = send_json(&app, delete(&uri)).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&app, get(&uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn attachments_require_an_existing_test_case_and_a_file() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        multipart_request("/test_cases/UNKNOWN/attachments", "notes.txt", b"evidence"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    create_test_case(&app, "TC-001").await;

    let empty = Request::builder()
        .method("POST")
        .uri("/test_cases/TC-001/attachments")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(format!("--{BOUNDARY}--\r\n")))
        .expect("request");

    let (status, body) = send_json(&app, empty).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "missing_file");
}

#[tokio::test]
async fn missing_attachments_return_not_found() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let uri = "/test_cases/TC-001/attachments/missing.txt";

    let (status, _) = send(&app, get(uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send_json(&app, delete(uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn attachment_downloads_use_a_content_type_derived_from_the_extension() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (_, uploaded) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "report.pdf", b"%PDF-1.4"),
    )
    .await;
    let filename = uploaded["filename"].as_str().expect("stored filename");

    let response = app
        .clone()
        .oneshot(get(&format!("/test_cases/TC-001/attachments/{filename}")))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/pdf")
    );
}

#[tokio::test]
async fn stored_documents_survive_a_repository_restart() {
    let directory = TempDir::new().expect("temp dir");

    {
        let repository = FileRepository::new(directory.path()).expect("repository");
        let app = api::router(repository);
        let (status, _) = send_json(
            &app,
            json_request("POST", "/projects", &json!({"name": "checkout"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let repository = FileRepository::new(directory.path()).expect("reopened repository");
    let app = api::router(repository);
    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], "checkout");
}
