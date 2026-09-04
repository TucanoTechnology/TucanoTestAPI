mod common;

use axum::http::StatusCode;
use common::{assert_error_envelope, delete, get, json_request, send_json, test_app};
use serde_json::json;

#[tokio::test]
async fn milestones_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/milestones")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/milestones",
            &json!({
                "milestoneId": "v1.0-RC1.json",
                "name": "v1.0-RC1",
                "startDate": "2026-09-01",
                "targetDate": "2026-09-15",
                "status": "Open"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "v1.0-RC1.json");

    let (status, listing) = send_json(&app, get("/milestones")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["v1.0-RC1.json"]));

    let (status, stored) = send_json(&app, get("/milestones/v1.0-RC1.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], "v1.0-RC1");
    assert_eq!(stored["status"], "Open");

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/milestones/v1.0-RC1.json",
            &json!({
                "milestoneId": "v1.0-RC1.json",
                "name": "v1.0-RC1",
                "status": "Completed"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/milestones/v1.0-RC1.json")).await;
    assert_eq!(updated["status"], "Completed");

    let (status, _) = send_json(&app, delete("/milestones/v1.0-RC1.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/milestones/v1.0-RC1.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn creating_a_milestone_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request("POST", "/milestones", &json!({"description": "no name"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
}

#[tokio::test]
async fn milestone_progress_aggregates_linked_test_runs() {
    let (_directory, app) = test_app();

    // 1. Create test run RUN-1.json with results
    send_json(
        &app,
        json_request(
            "POST",
            "/test_runs",
            &json!({
                "testRunId": "RUN-1.json",
                "name": "RUN-1",
                "timestamp": "2026-09-04T00:00:00Z",
                "results": [
                    {"testCaseId": "TC-1.json", "status": "Passed", "timestamp": "2026-09-04T12:00:00Z"},
                    {"testCaseId": "TC-2.json", "status": "Failed", "timestamp": "2026-09-04T12:01:00Z"},
                    {"testCaseId": "TC-3.json", "status": "Blocked", "timestamp": "2026-09-04T12:02:00Z"}
                ]
            }),
        ),
    )
    .await;

    // 2. Create milestone M-1.json linking RUN-1.json
    send_json(
        &app,
        json_request(
            "POST",
            "/milestones",
            &json!({
                "milestoneId": "M-1.json",
                "name": "Sprint 42",
                "testRunIds": ["RUN-1.json"]
            }),
        ),
    )
    .await;

    // 3. Fetch progress for M-1.json
    let (status, progress) = send_json(&app, get("/milestones/M-1.json/progress")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(progress["milestoneId"], "M-1.json");
    assert_eq!(progress["totalCases"], 3);
    assert_eq!(progress["passed"], 1);
    assert_eq!(progress["failed"], 1);
    assert_eq!(progress["blocked"], 1);
    assert_eq!(progress["passPercentage"], 33.33333333333333);
}

#[tokio::test]
async fn progress_for_missing_milestone_returns_not_found() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(&app, get("/milestones/missing.json/progress")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}
