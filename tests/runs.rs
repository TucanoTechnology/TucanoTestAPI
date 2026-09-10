mod common;

use axum::http::StatusCode;
use common::{
    assert_error_envelope, create_case_in, create_named, create_project, create_suite, delete, get,
    json_request, send_json, test_app,
};
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

#[tokio::test]
async fn test_runs_support_composition_execution_and_isolation() {
    let (_directory, app) = test_app();

    // 1. Create two test runs, one suite, and one test case
    send_json(
        &app,
        json_request(
            "POST",
            "/test_runs",
            &json!({"testRunId": "RUN-1.json", "name": "run1", "timestamp": "2026-09-04T00:00:00Z"}),
        ),
    )
    .await;

    send_json(
        &app,
        json_request(
            "POST",
            "/test_runs",
            &json!({"testRunId": "RUN-2.json", "name": "run2", "timestamp": "2026-09-04T00:00:00Z"}),
        ),
    )
    .await;

    let project = create_project(&app, "checkout").await;
    create_suite(&app, &project, "smoke").await;
    create_case_in(
        &app,
        &format!("/projects/{project}/test_cases"),
        "TC-001.json",
    )
    .await;

    // 2. Add suite and case to RUN-1.json
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/run1.json/test_suites",
            &json!({"suiteId": "smoke.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/run1.json/test_cases",
            &json!({"testCaseId": "TC-001.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 3. Reject adding duplicate case or suite to run
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/run1.json/test_cases",
            &json!({"testCaseId": "TC-001.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");

    // 4. Record result "Passed" for TC-001.json in RUN-1.json
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/run1.json/results",
            &json!({
                "testCaseId": "TC-001.json",
                "status": "Passed",
                "notes": "Login verified successfully",
                "timestamp": "2026-09-04T12:00:00Z"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 5. Record result "Failed" for TC-001.json in RUN-2.json (Run isolation)
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/run2.json/results",
            &json!({
                "testCaseId": "TC-001.json",
                "status": "Failed",
                "notes": "Login timeout in run2",
                "timestamp": "2026-09-04T12:10:00Z"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 6. Verify run1 results vs run2 results
    let (_, run1) = send_json(&app, get("/test_runs/run1.json")).await;
    assert_eq!(run1["results"][0]["status"], "Passed");
    assert_eq!(run1["results"][0]["notes"], "Login verified successfully");

    let (_, run2) = send_json(&app, get("/test_runs/run2.json")).await;
    assert_eq!(run2["results"][0]["status"], "Failed");
    assert_eq!(run2["results"][0]["notes"], "Login timeout in run2");

    // 7. Verify source test case was unmutated
    let (_, tc001) = send_json(&app, get("/test_cases/TC-001.json")).await;
    assert_eq!(tc001["title"], "Login");
    assert!(tc001.get("results").is_none());
}
