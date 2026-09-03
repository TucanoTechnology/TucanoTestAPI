mod common;

use axum::http::StatusCode;
use common::{assert_error_envelope, create_named, delete, get, json_request, send_json, test_app};
use serde_json::json;

#[tokio::test]
async fn test_suites_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/test_suites")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/test_suites",
            &json!({"suiteId": "S-001", "name": "regression", "testCases": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "regression.json");

    let (status, listing) = send_json(&app, get("/test_suites")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["regression.json"]));

    let (status, stored) = send_json(&app, get("/test_suites/regression.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["suiteId"], "S-001");

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_suites/regression.json",
            &json!({"suiteId": "S-002", "name": "regression", "testCases": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/test_suites/regression.json")).await;
    assert_eq!(updated["suiteId"], "S-002");

    let (status, _) = send_json(&app, delete("/test_suites/regression.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/test_suites/regression.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn creating_a_test_suite_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    for payload in [json!({"suiteId": "S-001"}), json!({"name": ""})] {
        let (status, body) = send_json(&app, json_request("POST", "/test_suites", &payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error_envelope(&body, "invalid_request");
    }
}

#[tokio::test]
async fn duplicate_test_suites_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_suites", "regression").await,
        "regression.json"
    );

    let (status, body) = send_json(
        &app,
        json_request("POST", "/test_suites", &json!({"name": "regression"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn missing_test_suites_return_not_found() {
    let (_directory, app) = test_app();

    for request in [
        get("/test_suites/missing.json"),
        delete("/test_suites/missing.json"),
        json_request(
            "PUT",
            "/test_suites/missing.json",
            &json!({"name": "missing"}),
        ),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error_envelope(&body, "not_found");
    }
}
