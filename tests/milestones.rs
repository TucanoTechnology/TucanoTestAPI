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
async fn a_milestone_created_from_a_name_alone_reads_back_and_reports_progress() {
    let (_directory, app) = test_app();

    // The body names the milestone and nothing else: no `milestoneId`.
    let (status, created) = send_json(
        &app,
        json_request("POST", "/milestones", &json!({"name": "M1"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");
    assert_eq!(created["id"], "M1.json");

    let (status, stored) = send_json(&app, get("/milestones/M1.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["milestoneId"], "M1.json");
    assert_eq!(stored["name"], "M1");

    // Progress deserialises the milestone, so it used to answer 500 here.
    let (status, progress) = send_json(&app, get("/milestones/M1.json/progress")).await;
    assert_eq!(status, StatusCode::OK, "progress: {progress}");
    assert_eq!(progress["milestoneId"], "M1.json");
    assert_eq!(progress["totalCases"], 0);
    assert_eq!(progress["passed"], 0);
    assert_eq!(progress["failed"], 0);
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

#[tokio::test]
async fn duplicating_a_milestone_copies_it_into_an_independent_document() {
    let (_directory, app) = test_app();

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
                    {"testCaseId": "TC-2.json", "status": "Failed", "timestamp": "2026-09-04T12:01:00Z"}
                ]
            }),
        ),
    )
    .await;

    send_json(
        &app,
        json_request(
            "POST",
            "/milestones",
            &json!({
                "milestoneId": "M-1.json",
                "name": "Sprint 42",
                "startDate": "2026-09-01",
                "targetDate": "2026-09-15",
                "status": "Open",
                "testRunIds": ["RUN-1.json"]
            }),
        ),
    )
    .await;

    let (status, duplicated) = send_json(
        &app,
        json_request("POST", "/milestones/M-1.json/duplicate", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "duplicating: {duplicated}");
    assert_eq!(duplicated["message"], "Milestone duplicated");
    let copy = duplicated["id"].as_str().expect("copy id").to_owned();
    assert!(copy.starts_with("M-1-copy-"), "unexpected copy id {copy}");
    assert!(
        copy.ends_with(".json"),
        "the derived id must address a document: {copy}"
    );

    let (status, stored) = send_json(&app, get(&format!("/milestones/{copy}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["milestoneId"], copy);
    assert_eq!(stored["name"], "Sprint 42");
    assert_eq!(stored["status"], "Open");
    assert_eq!(stored["startDate"], "2026-09-01");
    assert_eq!(stored["targetDate"], "2026-09-15");
    assert_eq!(stored["testRunIds"], json!(["RUN-1.json"]));

    // Progress is derived, so the copy reports the same progress as its source.
    let (status, progress) = send_json(&app, get(&format!("/milestones/{copy}/progress"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(progress["milestoneId"], copy);
    assert_eq!(progress["totalCases"], 2);
    assert_eq!(progress["passed"], 1);
    assert_eq!(progress["failed"], 1);

    // The two documents are independent: editing the copy leaves the source
    // untouched, and deleting the source leaves the copy in place.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            &format!("/milestones/{copy}"),
            &json!({"milestoneId": copy, "name": "Sprint 42 copy", "status": "Completed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, source) = send_json(&app, get("/milestones/M-1.json")).await;
    assert_eq!(source["name"], "Sprint 42");
    assert_eq!(source["status"], "Open");

    let (status, _) = send_json(&app, delete("/milestones/M-1.json")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_json(&app, get(&format!("/milestones/{copy}"))).await;
    assert_eq!(status, StatusCode::OK, "deleting the source keeps the copy");
}

#[tokio::test]
async fn duplicating_a_milestone_onto_an_existing_identifier_is_a_conflict() {
    let (_directory, app) = test_app();

    for id in ["M-1.json", "M-2.json"] {
        send_json(
            &app,
            json_request(
                "POST",
                "/milestones",
                &json!({"milestoneId": id, "name": id}),
            ),
        )
        .await;
    }

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/milestones/M-1.json/duplicate",
            &json!({"newId": "M-2.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");

    let (status, duplicated) = send_json(
        &app,
        json_request(
            "POST",
            "/milestones/M-1.json/duplicate",
            &json!({"newId": "M-3.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "duplicating: {duplicated}");
    assert_eq!(duplicated["id"], "M-3.json");

    let (status, stored) = send_json(&app, get("/milestones/M-3.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["milestoneId"], "M-3.json");
    assert_eq!(stored["name"], "M-1.json");
}

#[tokio::test]
async fn duplicating_a_missing_milestone_returns_not_found() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request("POST", "/milestones/missing.json/duplicate", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn a_partial_update_keeps_the_fields_the_body_leaves_out() {
    let (_directory, app) = test_app();

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/milestones",
            &json!({
                "milestoneId": "M-1.json",
                "name": "Sprint 42",
                "startDate": "2026-09-01",
                "targetDate": "2026-09-15",
                "status": "Open",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The body this bug is reported with: `{}` used to store a milestone with no
    // fields at all, after which progress answered 500 storage_error.
    let (status, body) = send_json(
        &app,
        json_request("PUT", "/milestones/M-1.json", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "updating: {body}");

    let (status, stored) = send_json(&app, get("/milestones/M-1.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["milestoneId"], "M-1.json");
    assert_eq!(stored["name"], "Sprint 42");
    assert_eq!(stored["startDate"], "2026-09-01");
    assert_eq!(stored["targetDate"], "2026-09-15");
    assert_eq!(stored["status"], "Open");

    let (status, progress) = send_json(&app, get("/milestones/M-1.json/progress")).await;
    assert_eq!(status, StatusCode::OK, "progress: {progress}");
    assert_eq!(progress["milestoneId"], "M-1.json");

    // A field the body carries is still replaced.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/milestones/M-1.json",
            &json!({"status": "Completed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get("/milestones/M-1.json")).await;
    assert_eq!(stored["status"], "Completed");
    assert_eq!(stored["name"], "Sprint 42");
    assert_eq!(stored["targetDate"], "2026-09-15");
}
