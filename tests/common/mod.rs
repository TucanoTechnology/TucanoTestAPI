// Shared helpers for the HTTP integration suites. Each test binary uses a subset,
// so unused-item warnings are expected here.
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::Path;
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
    api::router(repository)
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

/// Creates a flat resource from its `name` and returns the generated identifier.
pub async fn create_named(app: &Router, collection: &str, name: &str) -> String {
    let (status, created) = send_json(
        app,
        json_request("POST", collection, &json!({"name": name})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "creating {name} in {collection}"
    );
    created["id"].as_str().expect("created id").to_owned()
}

pub async fn create_test_case(app: &Router, id: &str) {
    let (status, _) = send_json(
        app,
        json_request(
            "POST",
            "/test_cases",
            &json!({"testCaseId": id, "title": "Upload", "expectedResult": "Stored"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating test case {id}");
}

/// Asserts the stable `{ "error": { "code", "message" } }` envelope.
pub fn assert_error_envelope(body: &Value, code: &str) {
    assert_eq!(body["error"]["code"], code, "unexpected error body: {body}");
    assert!(
        body["error"]["message"].is_string(),
        "error message missing: {body}"
    );
}
