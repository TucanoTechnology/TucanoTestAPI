//! Scalar type validation (#121).
//!
//! Every supplied top-level field must deserialise into the field its model
//! declares, so a body whose JSON type contradicts the schema is rejected with
//! `400 invalid_request` naming the field — on create and on update. The
//! deliberately lenient behaviour that survives is pinned here too: partial
//! payloads and omitted optional fields are still accepted, an unknown
//! top-level key is still rejected, and a document persisted before the change
//! is still readable and updatable.

mod common;

use axum::Router;
use axum::http::StatusCode;
use common::{app_at, create_project, get, json_request, send_json, test_app};
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;

/// Identifiers of the stored projects, sorted.
async fn listed_projects(app: &Router) -> Vec<String> {
    let (status, listing) = send_json(app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    let mut ids: Vec<String> = listing
        .as_array()
        .expect("a listing array")
        .iter()
        .map(|value| value.as_str().expect("identifier").to_owned())
        .collect();
    ids.sort();
    ids
}

/// Asserts a rejection with the stable envelope and a message naming `field`.
async fn assert_rejected(app: &Router, method: &str, uri: &str, body: Value, field: &str) {
    let (status, response) = send_json(app, json_request(method, uri, &body)).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{method} {uri} {body}: {response}"
    );
    common::assert_error_envelope(&response, "invalid_request");
    assert_eq!(
        response["error"]["message"],
        json!(format!("Field `{field}` is invalid")),
        "{method} {uri} {body}"
    );
}

#[tokio::test]
async fn wrong_typed_scalars_are_rejected_on_create() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "host").await;

    for (body, field) in [
        (json!({"name": "a", "tags": "smoke"}), "tags"),
        (json!({"name": "b", "tags": {"x": 1}}), "tags"),
        (json!({"name": "c", "tags": [1, 2]}), "tags"),
        (json!({"name": "d", "description": 7}), "description"),
    ] {
        assert_rejected(&app, "POST", "/projects", body, field).await;
    }

    assert_rejected(
        &app,
        "POST",
        "/test_runs",
        json!({"name": "nightly", "timestamp": 123}),
        "timestamp",
    )
    .await;

    assert_rejected(
        &app,
        "POST",
        &format!("/projects/{project}/test_cases"),
        json!({
            "testCaseId": "TC-1",
            "title": "Login",
            "expectedResult": "Stored",
            "exploratory": "yes"
        }),
        "exploratory",
    )
    .await;

    assert_rejected(
        &app,
        "POST",
        "/milestones",
        json!({"name": "v1.0", "testSuiteIds": "S-1"}),
        "testSuiteIds",
    )
    .await;

    assert_rejected(
        &app,
        "POST",
        "/configurations",
        json!({"name": "chrome", "browser": 1}),
        "browser",
    )
    .await;

    assert_eq!(
        listed_projects(&app).await,
        vec![project.clone()],
        "only the host project may be stored"
    );
}

#[tokio::test]
async fn valid_and_omitted_scalars_are_accepted() {
    let (_directory, app) = test_app();

    for body in [
        json!({"name": "alpha"}),
        json!({"name": "beta", "tags": []}),
        json!({"name": "gamma", "tags": ["smoke", "regression"]}),
    ] {
        let (status, created) = send_json(&app, json_request("POST", "/projects", &body)).await;
        assert_eq!(status, StatusCode::CREATED, "{body}: {created}");
    }

    let host = create_project(&app, "host").await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{host}/test_cases"),
            &json!({"testCaseId": "TC-1", "title": "t", "expectedResult": "e"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}

#[tokio::test]
async fn an_unknown_top_level_key_is_still_rejected() {
    let (_directory, app) = test_app();

    let (status, response) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "a", "sneaky": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    common::assert_error_envelope(&response, "invalid_request");
    assert_eq!(
        response["error"]["message"],
        json!("Unknown field `sneaky`")
    );
}

#[tokio::test]
async fn a_partial_update_carrying_only_a_wrong_typed_field_is_rejected() {
    let (_directory, app) = test_app();
    let id = create_project(&app, "checkout").await;

    assert_rejected(
        &app,
        "PUT",
        &format!("/projects/{id}"),
        json!({"tags": "smoke"}),
        "tags",
    )
    .await;

    let (status, stored) = send_json(&app, get(&format!("/projects/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], json!("checkout"));
    assert!(
        stored.get("tags").is_none(),
        "a rejected update must not change the document: {stored}"
    );
}

#[tokio::test]
async fn a_partial_update_of_a_valid_field_keeps_the_omitted_fields() {
    let (_directory, app) = test_app();

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"name": "checkout", "description": "old", "tags": ["smoke"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("created id");

    let (status, updated) = send_json(
        &app,
        json_request(
            "PUT",
            &format!("/projects/{id}"),
            &json!({"description": "new"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let (status, stored) = send_json(&app, get(&format!("/projects/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["description"], json!("new"));
    assert_eq!(stored["name"], json!("checkout"));
    assert_eq!(stored["tags"], json!(["smoke"]));
}

/// A document persisted before the change carried a wrong-typed scalar and was
/// stored verbatim; tightening the write gate must not make it unreadable.
#[tokio::test]
async fn a_document_persisted_before_the_change_stays_readable() {
    let directory = TempDir::new().expect("temp dir");
    let project = directory.path().join("projects/legacy");
    fs::create_dir_all(&project).expect("project folder");
    fs::write(
        project.join("project.json"),
        r#"{"projectId":"legacy.json","name":"legacy","tags":"smoke","testSuites":[]}"#,
    )
    .expect("legacy document");
    let app = app_at(directory.path());

    let (status, stored) = send_json(&app, get("/projects/legacy.json")).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(stored["tags"], json!("smoke"), "the legacy value is served");

    let (status, listed) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed, json!(["legacy.json"]));

    let (status, updated) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/legacy.json",
            &json!({"description": "updated"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let (status, stored) = send_json(&app, get("/projects/legacy.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["description"], json!("updated"));
    assert_eq!(stored["tags"], json!("smoke"));
}
