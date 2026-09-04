mod common;

use axum::http::StatusCode;
use common::{assert_error_envelope, delete, get, json_request, send_json, test_app};
use serde_json::json;

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
async fn list_supports_case_insensitive_substring_filtering() {
    let (_directory, app) = test_app();

    send_json(&app, json_request("POST", "/projects", &json!({"name": "alpha"}))).await;
    send_json(&app, json_request("POST", "/projects", &json!({"name": "beta"}))).await;
    send_json(&app, json_request("POST", "/projects", &json!({"name": "alphabet"}))).await;

    let (status, body) = send_json(&app, get("/projects?filter=ALPH")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(["alpha.json", "alphabet.json"]));

    let (status, body) = send_json(&app, get("/projects?filter=nonexistent")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn creating_a_project_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"description": "no name"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    let (status, body) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": ""})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
}

#[tokio::test]
async fn duplicate_projects_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    let payload = json!({"name": "checkout"});

    let (status, _) = send_json(&app, json_request("POST", "/projects", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(&app, json_request("POST", "/projects", &payload)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn missing_projects_return_a_stable_error_envelope() {
    let (_directory, app) = test_app();

    for request in [
        get("/projects/missing.json"),
        delete("/projects/missing.json"),
        json_request("PUT", "/projects/missing.json", &json!({"name": "missing"})),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error_envelope(&body, "not_found");
    }
}
