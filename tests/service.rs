mod common;

use axum::http::StatusCode;
use common::{app_at, get, json_request, raw_json_request, send, send_json, test_app};
use serde_json::json;
use tempfile::TempDir;

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
        "/milestones",
        "/milestones/{id}",
        "/milestones/{id}/progress",
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
async fn malformed_request_bodies_are_rejected() {
    let (_directory, app) = test_app();

    let (status, _) = send(&app, raw_json_request("POST", "/projects", "{ not json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn identifiers_cannot_escape_the_storage_root() {
    let (_directory, app) = test_app();

    for uri in [
        "/projects/..%2F..%2Fescape.json",
        "/projects/nested%2Fchild.json",
        "/test_cases/..%2Fescape",
        "/test_cases/TC-001/attachments/..%2F..%2Fescape.txt",
    ] {
        let (status, _) = send(&app, get(uri)).await;
        assert!(
            status.is_client_error(),
            "traversal attempt should be rejected: {uri} returned {status}"
        );
    }
}

#[tokio::test]
async fn stored_documents_survive_a_repository_restart() {
    let directory = TempDir::new().expect("temp dir");

    {
        let app = app_at(directory.path());
        let (status, _) = send_json(
            &app,
            json_request("POST", "/projects", &json!({"name": "checkout"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let app = app_at(directory.path());
    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], "checkout");
}
