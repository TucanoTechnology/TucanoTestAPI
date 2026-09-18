// Shared helpers for the HTTP integration suites. Each test binary uses a subset,
// so unused-item warnings are expected here.
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use tower::ServiceExt;
use tucano_test::{api, repository::FileRepository};

pub const BOUNDARY: &str = "tucanotestboundary";

/// Builds a router backed by an isolated temporary data directory.
/// The returned `TempDir` must stay alive for the duration of the test.
pub fn test_app() -> (TempDir, Router) {
    let directory = TempDir::new().expect("temp dir");
    let router = app_at(directory.path());
    (directory, router)
}

pub fn app_at(path: &Path) -> Router {
    let repository = FileRepository::new(path).expect("repository");
    let store = tucano_test::auth::AuthStore::new(path).expect("auth store");
    // Authentication is off by default, which is the behaviour these suites were
    // written against: the resource routes ask no caller for a token. The suites
    // that exercise authentication build their own router.
    let config = tucano_test::auth::AuthConfig {
        required: false,
        jwt_secret: None,
        access_ttl: tucano_test::auth::DEFAULT_ACCESS_TTL,
        refresh_ttl: tucano_test::auth::DEFAULT_REFRESH_TTL,
        bootstrap_username: None,
        bootstrap_password: None,
    };
    api::router(repository, api::auth::AuthState::new(store, config))
}

/// Like [`app_at`], but with a custom advisory-lock timeout for both the
/// repository and the auth store, so tests can force a fast 503 instead of
/// waiting the default five seconds.
pub fn app_at_with_lock_timeout(path: &Path, timeout: Duration) -> Router {
    let repository = FileRepository::new(path)
        .expect("repository")
        .with_lock_timeout(timeout);
    let store = tucano_test::auth::AuthStore::new(path)
        .expect("auth store")
        .with_lock_timeout(timeout);
    let config = tucano_test::auth::AuthConfig {
        required: false,
        jwt_secret: None,
        access_ttl: tucano_test::auth::DEFAULT_ACCESS_TTL,
        refresh_ttl: tucano_test::auth::DEFAULT_REFRESH_TTL,
        bootstrap_username: None,
        bootstrap_password: None,
    };
    api::router(repository, api::auth::AuthState::new(store, config))
}

pub async fn send_full(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    (status, headers, bytes.to_vec())
}

pub async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let (status, _, bytes) = send_full(app, request).await;
    (status, bytes)
}

pub async fn send_json(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let (status, bytes) = send(app, request).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub fn content_type(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::CONTENT_TYPE)?.to_str().ok()
}

pub fn content_disposition(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()
}

pub fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

pub fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

pub fn json_request(method: &str, uri: &str, body: &Value) -> Request<Body> {
    raw_json_request(method, uri, body.to_string())
}

pub fn raw_json_request(method: &str, uri: &str, body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.into())
        .expect("request")
}

/// Posts an XML body, the content type the JUnit import route reads. The body
/// is any bytes, so a test can also send one that is not valid UTF-8.
pub fn xml_request(uri: &str, body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/xml")
        .body(body.into())
        .expect("request")
}

pub fn multipart_request(uri: &str, filename: &str, contents: &[u8]) -> Request<Body> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    body.extend_from_slice(contents);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    multipart_with_body(uri, body)
}

pub fn multipart_without_file(uri: &str) -> Request<Body> {
    multipart_with_body(uri, format!("--{BOUNDARY}--\r\n").into_bytes())
}

fn multipart_with_body(uri: &str, body: Vec<u8>) -> Request<Body> {
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

/// Creates a project, suite, or flat resource from its `name` and returns the
/// generated identifier.
///
/// A run, a milestone and a configuration are stored inside the project that
/// owns them, so naming one of those collections by its retired flat path
/// creates the fixture project first and posts to the project-scoped route.
pub async fn create_named(app: &Router, collection: &str, name: &str) -> String {
    let collection = scoped(app, collection).await;
    created_id(
        app,
        &collection,
        &json!({"name": name}),
        &format!("{name} in {collection}"),
    )
    .await
}

/// The project the fixtures store their project-scoped resources in.
///
/// Created on first use and returned unchanged afterwards, so a test — and every
/// helper it calls — may ask for it as often as it likes and always gets the one
/// home the tree already has.
pub async fn fixture_home(app: &Router) -> String {
    let (status, body) = send_json(
        app,
        json_request("POST", "/projects", &json!({"name": "checkout"})),
    )
    .await;
    match status {
        StatusCode::CREATED => body["id"].as_str().expect("project id").to_owned(),
        StatusCode::CONFLICT => "checkout.json".to_owned(),
        other => panic!("creating the fixture project answered {other}: {body}"),
    }
}

/// The folder that stores a project's project-scoped resources.
///
/// A project is addressed by its identifier (`checkout.json`) but stored as a
/// folder named after it with the document suffix removed (`projects/checkout`).
pub fn project_folder(home: &str) -> &str {
    home.strip_suffix(".json")
        .expect("a project identifier addresses a document")
}

/// Rewrites a retired flat collection path to the project-scoped route that
/// replaced it, creating the home the resource needs. Any other path — a
/// project, or a collection that is already parent-scoped — is returned as it
/// is.
pub async fn scoped(app: &Router, collection: &str) -> String {
    match collection {
        "/test_runs" | "/milestones" | "/configurations" => format!(
            "/projects/{}/{}",
            fixture_home(app).await,
            collection.trim_start_matches('/')
        ),
        other => other.to_owned(),
    }
}

/// Creates a project at the root of the tree.
pub async fn create_project(app: &Router, name: &str) -> String {
    create_named(app, "/projects", name).await
}

/// Creates a suite inside a project and returns the generated identifier.
pub async fn create_suite(app: &Router, project: &str, name: &str) -> String {
    created_id(
        app,
        &format!("/projects/{project}/test_suites"),
        &json!({"name": name}),
        &format!("suite {name} in {project}"),
    )
    .await
}

/// A minimal payload that creates a test case.
pub fn case_body(id: &str) -> Value {
    json!({"testCaseId": id, "title": "Login", "expectedResult": "Stored"})
}

/// Creates a test case under an existing parent collection, given as a path
/// such as `/projects/checkout.json/test_cases`.
pub async fn create_case_in(app: &Router, collection: &str, id: &str) -> String {
    created_id(
        app,
        collection,
        &case_body(id),
        &format!("test case {id} in {collection}"),
    )
    .await
}

/// Creates a test case in a project of its own, so that the case has exactly
/// one home and every document-level route resolves it.
pub async fn create_test_case(app: &Router, id: &str) {
    let project = create_project(app, "checkout").await;
    create_case_in(app, &format!("/projects/{project}/test_cases"), id).await;
}

async fn created_id(app: &Router, collection: &str, body: &Value, what: &str) -> String {
    let (status, created) = send_json(app, json_request("POST", collection, body)).await;
    assert_eq!(status, StatusCode::CREATED, "creating {what}: {created}");
    created["id"].as_str().expect("created id").to_owned()
}

/// Asserts the stable `{ "error": { "code", "message" } }` envelope.
pub fn assert_error_envelope(body: &Value, code: &str) {
    assert_eq!(body["error"]["code"], code, "unexpected error body: {body}");
    assert!(
        body["error"]["message"].is_string(),
        "error message missing: {body}"
    );
}

/// The write operations a project-scoped role can refuse, labelled by the same
/// `method path` convention `openapi::documented_operations` uses for the
/// served document.
///
/// This single list is what both suites agree on: `auth` drives its matrix of
/// `403` requests from it, so every listed route is proven to answer a caller
/// that lacks the role, and `service` asserts each listed route documents a
/// `403` in `openapi.json`. A route that gains or loses its guard therefore
/// breaks one side or the other instead of quietly drifting out of the
/// contract.
pub const ROLE_CHECKED_WRITE_OPERATIONS: [&str; 39] = [
    "post /projects",
    "put /projects/{id}",
    "delete /projects/{id}",
    "post /projects/{id}/duplicate",
    "put /test_suites/{id}",
    "delete /test_suites/{id}",
    "post /test_suites/{id}/duplicate",
    "post /projects/{id}/test_suites",
    "post /test_suites/{id}/test_cases",
    "delete /test_suites/{id}/test_cases/{case_id}",
    "delete /projects/{id}/test_suites/{suite_id}",
    "put /test_cases/{id}",
    "delete /test_cases/{id}",
    "post /test_cases/{id}/duplicate",
    "post /projects/{id}/test_cases",
    "delete /projects/{id}/test_cases/{case_id}",
    "post /projects/{id}/test_runs",
    "delete /projects/{id}/test_runs/{run_id}",
    "put /test_runs/{id}",
    "delete /test_runs/{id}",
    "post /test_runs/{id}/duplicate",
    "post /test_runs/{id}/test_suites",
    "post /test_runs/{id}/test_cases",
    "post /test_runs/{id}/results",
    "post /test_runs/{id}/results/{case_id}/defects",
    "delete /test_runs/{id}/results/{case_id}/defects/{link_id}",
    "post /test_runs/{id}/import/junit",
    "post /test_runs/{id}/import/json",
    "post /test_runs/{id}/configurations",
    "delete /test_runs/{id}/configurations/{config_id}",
    "post /projects/{id}/milestones",
    "delete /projects/{id}/milestones/{milestone_id}",
    "put /milestones/{id}",
    "delete /milestones/{id}",
    "post /milestones/{id}/duplicate",
    "post /projects/{id}/configurations",
    "delete /projects/{id}/configurations/{config_id}",
    "put /configurations/{id}",
    "delete /configurations/{id}",
];

/// The request a role matrix sends for one entry of
/// [`ROLE_CHECKED_WRITE_OPERATIONS`]. The identifiers come from the seeded
/// tree; a payload that the route would accept is used so the request reaches
/// the role check rather than failing an extractor first.
pub fn role_checked_write(
    label: &str,
    project: &str,
    suite: &str,
    case: &str,
    run: &str,
    milestone: &str,
    configuration: &str,
) -> (&'static str, String, Option<Value>) {
    let method = match label.split_once(' ').expect("label").0 {
        "get" => "GET",
        "put" => "PUT",
        "post" => "POST",
        "delete" => "DELETE",
        other => panic!("unknown role-checked method: {other}"),
    };
    let request = match label {
        "post /projects" => ("/projects".to_owned(), Some(json!({"name": "beta"}))),
        "put /projects/{id}" => (format!("/projects/{project}"), Some(json!({}))),
        "delete /projects/{id}" => (format!("/projects/{project}"), None),
        "post /projects/{id}/duplicate" => {
            (format!("/projects/{project}/duplicate"), Some(json!({})))
        }
        "put /test_suites/{id}" => (format!("/test_suites/{suite}"), Some(json!({}))),
        "delete /test_suites/{id}" => (format!("/test_suites/{suite}"), None),
        "post /test_suites/{id}/duplicate" => {
            (format!("/test_suites/{suite}/duplicate"), Some(json!({})))
        }
        "post /projects/{id}/test_suites" => (
            format!("/projects/{project}/test_suites"),
            Some(json!({"name": "extra"})),
        ),
        "post /test_suites/{id}/test_cases" => (
            format!("/test_suites/{suite}/test_cases"),
            Some(case_body("TC-X")),
        ),
        "delete /test_suites/{id}/test_cases/{case_id}" => {
            (format!("/test_suites/{suite}/test_cases/{case}"), None)
        }
        "delete /projects/{id}/test_suites/{suite_id}" => {
            (format!("/projects/{project}/test_suites/{suite}"), None)
        }
        "put /test_cases/{id}" => (format!("/test_cases/{case}"), Some(json!({}))),
        "delete /test_cases/{id}" => (format!("/test_cases/{case}"), None),
        "post /test_cases/{id}/duplicate" => {
            (format!("/test_cases/{case}/duplicate"), Some(json!({})))
        }
        "post /projects/{id}/test_cases" => (
            format!("/projects/{project}/test_cases"),
            Some(case_body("TC-Y")),
        ),
        "delete /projects/{id}/test_cases/{case_id}" => {
            (format!("/projects/{project}/test_cases/{case}"), None)
        }
        "post /projects/{id}/test_runs" => (
            format!("/projects/{project}/test_runs"),
            Some(json!({
                "name": "extra",
                "timestamp": "2026-09-04T00:00:00Z",
                "projects": [{"projectId": project, "name": "alpha", "testSuites": []}],
            })),
        ),
        "delete /projects/{id}/test_runs/{run_id}" => {
            (format!("/projects/{project}/test_runs/{run}"), None)
        }
        "put /test_runs/{id}" => (format!("/test_runs/{run}"), Some(json!({}))),
        "delete /test_runs/{id}" => (format!("/test_runs/{run}"), None),
        "post /test_runs/{id}/duplicate" => {
            (format!("/test_runs/{run}/duplicate"), Some(json!({})))
        }
        "post /test_runs/{id}/test_suites" => (
            format!("/test_runs/{run}/test_suites"),
            Some(json!({"suiteId": suite})),
        ),
        "post /test_runs/{id}/test_cases" => (
            format!("/test_runs/{run}/test_cases"),
            Some(json!({"testCaseId": case})),
        ),
        "post /test_runs/{id}/results" => (format!("/test_runs/{run}/results"), Some(json!({}))),
        "post /test_runs/{id}/results/{case_id}/defects" => (
            format!("/test_runs/{run}/results/{case}/defects"),
            Some(json!({})),
        ),
        "delete /test_runs/{id}/results/{case_id}/defects/{link_id}" => (
            format!("/test_runs/{run}/results/{case}/defects/link-1"),
            None,
        ),
        "post /test_runs/{id}/import/junit" => (format!("/test_runs/{run}/import/junit"), None),
        "post /test_runs/{id}/import/json" => (format!("/test_runs/{run}/import/json"), None),
        "post /test_runs/{id}/configurations" => {
            (format!("/test_runs/{run}/configurations"), Some(json!({})))
        }
        "delete /test_runs/{id}/configurations/{config_id}" => {
            (format!("/test_runs/{run}/configurations/config-1"), None)
        }
        "post /projects/{id}/milestones" => (
            format!("/projects/{project}/milestones"),
            Some(json!({"name": "linked", "testRunIds": [run]})),
        ),
        "delete /projects/{id}/milestones/{milestone_id}" => {
            (format!("/projects/{project}/milestones/{milestone}"), None)
        }
        "put /milestones/{id}" => (format!("/milestones/{milestone}"), Some(json!({}))),
        "delete /milestones/{id}" => (format!("/milestones/{milestone}"), None),
        "post /milestones/{id}/duplicate" => (
            format!("/milestones/{milestone}/duplicate"),
            Some(json!({})),
        ),
        "post /projects/{id}/configurations" => (
            format!("/projects/{project}/configurations"),
            Some(json!({"name": "extra"})),
        ),
        "delete /projects/{id}/configurations/{config_id}" => (
            format!("/projects/{project}/configurations/{configuration}"),
            None,
        ),
        "put /configurations/{id}" => (format!("/configurations/{configuration}"), Some(json!({}))),
        "delete /configurations/{id}" => (format!("/configurations/{configuration}"), None),
        other => panic!("unknown role-checked operation: {other}"),
    };
    (method, request.0, request.1)
}
