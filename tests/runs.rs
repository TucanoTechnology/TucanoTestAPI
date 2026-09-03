mod common;

use axum::http::StatusCode;
use common::{assert_error_envelope, create_named, delete, get, json_request, send_json, test_app};
use serde_json::json;

#[tokio::test]
async fn test_runs_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/test_runs")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs",
            &json!({"testRunId": "R-001", "name": "nightly", "timestamp": "2026-09-02T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "nightly.json");

    let (status, listing) = send_json(&app, get("/test_runs")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["nightly.json"]));

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["testRunId"], "R-001");

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/nightly.json",
            &json!({"testRunId": "R-002", "name": "nightly", "timestamp": "2026-09-03T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(updated["testRunId"], "R-002");

    let (status, _) = send_json(&app, delete("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn creating_a_test_run_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    for payload in [json!({"testRunId": "R-001"}), json!({"name": ""})] {
        let (status, body) = send_json(&app, json_request("POST", "/test_runs", &payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error_envelope(&body, "invalid_request");
    }
}

#[tokio::test]
async fn duplicate_test_runs_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );

    let (status, body) = send_json(
        &app,
        json_request("POST", "/test_runs", &json!({"name": "nightly"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn missing_test_runs_return_not_found() {
    let (_directory, app) = test_app();

    for request in [
        get("/test_runs/missing.json"),
        delete("/test_runs/missing.json"),
        json_request(
            "PUT",
            "/test_runs/missing.json",
            &json!({"name": "missing"}),
        ),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error_envelope(&body, "not_found");
    }
}
