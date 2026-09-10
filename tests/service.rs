mod common;

use axum::http::StatusCode;
use common::{
    app_at, assert_error_envelope, get, json_request, raw_json_request, send, send_json, test_app,
};
use serde_json::json;
use std::collections::BTreeSet;
use tempfile::TempDir;
use tucano_test::api;

#[tokio::test]
async fn health_reports_filesystem_storage() {
    let (_directory, app) = test_app();
    let (status, body) = send_json(&app, get("/health")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["storage"], "filesystem");
}

#[tokio::test]
async fn openapi_document_matches_the_registered_routes() {
    let (_directory, app) = test_app();
    let (status, body) = send_json(&app, get("/openapi.json")).await;

    assert_eq!(status, StatusCode::OK);
    let documented: BTreeSet<String> = body["paths"]
        .as_object()
        .expect("paths object")
        .keys()
        .cloned()
        .collect();

    for route in api::UNDOCUMENTED_ROUTES {
        assert!(
            api::ROUTES.contains(route),
            "exempt route is not registered: {route}"
        );
    }

    let expected: BTreeSet<String> = api::ROUTES
        .iter()
        .copied()
        .filter(|route| !api::UNDOCUMENTED_ROUTES.contains(route))
        .map(str::to_owned)
        .collect();

    assert_eq!(
        documented, expected,
        "openapi.json no longer matches the registered routes"
    );
}

#[tokio::test]
async fn router_serves_every_declared_route() {
    let (_directory, app) = test_app();

    // No route accepts PATCH, so a registered path answers 405 and an
    // unregistered one answers 404 — which is exactly the distinction to test.
    for route in api::ROUTES {
        let (status, _) = send(&app, raw_json_request("PATCH", route, "")).await;
        assert_eq!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{route} is declared in api::ROUTES but the router does not serve it"
        );
    }

    let (status, _) = send(&app, raw_json_request("PATCH", "/not-a-route", "")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_and_update_reject_unknown_fields() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"name": "checkout", "unknownField": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    let (status, listed) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed, json!([]), "a rejected body must not be stored");

    let id = common::create_named(&app, "/projects", "checkout").await;
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            &format!("/projects/{id}"),
            &json!({"name": "renamed", "unknownField": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
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
