//! `GET /reports/coverage` — the case counts per suite and in total, for one
//! project or across every project.

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{assert_error_envelope, create_project, create_suite, get, send_json, test_app};

#[tokio::test]
async fn an_empty_tree_reports_no_cases() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(&app, get("/reports/coverage")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"totalCases": 0, "suites": []}));
    assert!(
        body.get("projectId").is_none(),
        "an unscoped report omits the identifier: {body}"
    );
}

#[tokio::test]
async fn a_scoped_report_counts_the_suites_and_the_projects_own_cases() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;
    common::create_case_in(&app, &format!("/test_suites/{suite}/test_cases"), "TC-001").await;
    common::create_case_in(&app, &format!("/test_suites/{suite}/test_cases"), "TC-002").await;
    // A case held directly by the project belongs to no suite, so it adds to
    // the total without appearing in any suite entry.
    common::create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-003").await;

    let (status, body) =
        send_json(&app, get(&format!("/reports/coverage?projectId={project}"))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["projectId"], project);
    assert_eq!(body["totalCases"], 3);
    assert_eq!(
        body["suites"],
        json!([{"suiteId": suite, "name": "smoke", "caseCount": 2}])
    );
}

#[tokio::test]
async fn a_global_report_sums_every_project() {
    let (_directory, app) = test_app();

    let checkout = create_project(&app, "checkout").await;
    let checkout_suite = create_suite(&app, &checkout, "smoke").await;
    common::create_case_in(
        &app,
        &format!("/test_suites/{checkout_suite}/test_cases"),
        "TC-001",
    )
    .await;
    // A suite with no cases is still reported, with a zero count.
    create_suite(&app, &checkout, "empty").await;

    let billing = create_project(&app, "billing").await;
    let billing_suite = create_suite(&app, &billing, "regression").await;
    common::create_case_in(
        &app,
        &format!("/test_suites/{billing_suite}/test_cases"),
        "TC-001",
    )
    .await;
    common::create_case_in(
        &app,
        &format!("/test_suites/{billing_suite}/test_cases"),
        "TC-002",
    )
    .await;
    common::create_case_in(&app, &format!("/projects/{billing}/test_cases"), "TC-003").await;

    let (status, body) = send_json(&app, get("/reports/coverage")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.get("projectId").is_none(),
        "a global report omits the identifier: {body}"
    );
    assert_eq!(body["totalCases"], 4);
    let suites = body["suites"].as_array().expect("suites array");
    assert_eq!(suites.len(), 3, "every suite of every project: {body}");
    // Projects are listed sorted by folder (`billing` ahead of `checkout`) and
    // so are the suites inside each, which is the order the report keeps.
    assert_eq!(
        suites
            .iter()
            .map(|entry| (
                entry["name"].as_str().unwrap(),
                entry["caseCount"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [("regression", 2), ("empty", 0), ("smoke", 1)]
    );
}

#[tokio::test]
async fn a_project_that_does_not_exist_is_not_found() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(&app, get("/reports/coverage?projectId=missing.json")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn an_unusable_project_identifier_is_invalid_id() {
    let (_directory, app) = test_app();
    create_project(&app, "checkout").await;

    // The identifier is a `.json` document name, so one that cannot be a
    // single path component is a bad identifier rather than a missing project.
    let (status, body) = send_json(&app, get("/reports/coverage?projectId=nope")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_id");
}
