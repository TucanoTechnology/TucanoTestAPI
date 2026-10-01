mod common;

use axum::http::StatusCode;
use common::{
    app_at, assert_error_envelope, create_project, delete, fixture_home, get, json_request,
    project_folder, send_json, test_app,
};
use serde_json::json;

#[tokio::test]
async fn workflows_are_created_inside_a_project() {
    let (_directory, app) = test_app();

    // The retired flat routes still serve (for listing) but creation is refused.
    let (status, listing) = send_json(&app, get("/workflows")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, body) = send_json(
        &app,
        json_request("POST", "/workflows", &json!({"name": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    // Creation through the project-scoped route.
    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/workflows"),
            &json!({
                "name": "release-smoke",
                "steps": [{"testCaseId": "TC-001"}]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "release-smoke.json");

    // Listing through the project.
    let (status, listing) = send_json(&app, get(&format!("/projects/{home}/workflows"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["release-smoke.json"]));
}

#[tokio::test]
async fn workflows_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();
    let home = fixture_home(&app).await;

    // Create
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/workflows"),
            &json!({
                "name": "smoke",
                "steps": [{"testCaseId": "TC-001"}]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();

    // Read
    let (status, stored) = send_json(&app, get(&format!("/workflows/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], "smoke");

    // Update
    let (status, updated) = send_json(
        &app,
        json_request(
            "PUT",
            &format!("/workflows/{id}"),
            &json!({"name": "smoke-v2"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Read again to verify
    let (status, stored) = send_json(&app, get(&format!("/workflows/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], "smoke-v2");

    // Delete through the project-scoped route
    let (status, _) = send_json(
        &app,
        delete(&format!("/projects/{home}/workflows/{id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Gone
    let (status, _) = send_json(&app, get(&format!("/workflows/{id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
