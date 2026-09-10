//! The reporting endpoints: `GET /reports/coverage` — the case counts per suite
//! and in total, for one project or across every project — and
//! `GET /reports/summary` — how the recorded results in scope split by status,
//! with their pass rate and total duration.

mod common;

use axum::Router;
use axum::http::StatusCode;
use serde_json::{Value, json};

use common::{
    assert_error_envelope, create_named, create_project, create_suite, get, json_request,
    send_json, test_app,
};

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

/// Creates a run from a complete body and returns the identifier the API stored
/// it under.
async fn create_run(app: &Router, body: Value) -> String {
    let (status, created) = send_json(app, json_request("POST", "/test_runs", &body)).await;
    assert_eq!(status, StatusCode::CREATED, "creating run: {created}");
    created["id"].as_str().expect("run id").to_owned()
}

/// Creates a run that already carries its own recorded results.
async fn record_run(app: &Router, name: &str, timestamp: &str, results: Value) -> String {
    create_run(
        app,
        json!({
            "testRunId": name,
            "name": name,
            "timestamp": timestamp,
            "results": results,
        }),
    )
    .await
}

/// One recorded result, so a test can vary just the status and the duration.
fn result(case_id: &str, status: &str, duration_ms: Option<u64>) -> Value {
    let mut result = json!({"testCaseId": case_id, "status": status, "timestamp": "1"});
    if let Some(duration_ms) = duration_ms {
        result["durationMs"] = json!(duration_ms);
    }
    result
}

#[tokio::test]
async fn an_empty_tree_reports_an_all_zero_summary() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(&app, get("/reports/summary")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "total": 0,
            "passed": 0,
            "failed": 0,
            "blocked": 0,
            "untested": 0,
            "passPercentage": 0.0,
            "totalDurationMs": 0,
        })
    );
}

#[tokio::test]
async fn a_summary_buckets_every_status_and_sums_the_durations() {
    let (_directory, app) = test_app();
    record_run(
        &app,
        "RUN-1",
        "2026-09-04T00:00:00Z",
        json!([
            result("TC-1.json", "Passed", Some(1_000)),
            result("TC-2.json", "Failed", Some(250)),
            result("TC-3.json", "Blocked", None),
            result("TC-4.json", "Untested", Some(50)),
            result("TC-5.json", "Retest", Some(500)),
        ]),
    )
    .await;

    let (status, body) = send_json(&app, get("/reports/summary")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 5);
    assert_eq!(body["passed"], 1);
    assert_eq!(body["failed"], 1);
    assert_eq!(body["blocked"], 1);
    assert_eq!(body["untested"], 1);
    // `Retest` counts toward the total, and so toward the pass rate's
    // denominator, without being a pass or a failure.
    assert_eq!(body["passPercentage"], 20.0);
    // A result without a duration contributes nothing to the total.
    assert_eq!(body["totalDurationMs"], 1_800);
}

#[tokio::test]
async fn results_from_every_run_join_an_unfiltered_summary() {
    let (_directory, app) = test_app();
    record_run(
        &app,
        "nightly",
        "2026-09-04T00:00:00Z",
        json!([
            result("TC-1.json", "Passed", Some(400)),
            result("TC-2.json", "Passed", None),
        ]),
    )
    .await;
    record_run(
        &app,
        "weekly",
        "2026-09-05T00:00:00Z",
        json!([result("TC-3.json", "Failed", Some(600))]),
    )
    .await;

    let (status, body) = send_json(&app, get("/reports/summary")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 3);
    assert_eq!(body["passed"], 2);
    assert_eq!(body["failed"], 1);
    assert_eq!(body["passPercentage"], 2.0 / 3.0 * 100.0);
    assert_eq!(body["totalDurationMs"], 1_000);
}

#[tokio::test]
async fn the_filters_combine_and_each_restricts_the_runs_that_contribute() {
    let (_directory, app) = test_app();
    let checkout = create_project(&app, "checkout").await;
    let billing = create_project(&app, "billing").await;
    let chrome = create_named(&app, "/configurations", "chrome-linux").await;

    let nightly = create_run(
        &app,
        json!({
            "testRunId": "nightly",
            "name": "nightly",
            "timestamp": "2026-09-04T00:00:00Z",
            "projects": [{"projectId": checkout, "name": "checkout", "testSuites": []}],
            "results": [result("TC-1.json", "Passed", None)],
        }),
    )
    .await;
    // A run of the same project that links no configuration, so the
    // configuration filter has something to exclude.
    let _weekly = create_run(
        &app,
        json!({
            "testRunId": "weekly",
            "name": "weekly",
            "timestamp": "2026-09-05T00:00:00Z",
            "projects": [{"projectId": checkout, "name": "checkout", "testSuites": []}],
            "results": [result("TC-2.json", "Failed", None)],
        }),
    )
    .await;
    // A run of another project, so the project filter has something to exclude.
    let daily = create_run(
        &app,
        json!({
            "testRunId": "daily",
            "name": "daily",
            "timestamp": "2026-09-06T00:00:00Z",
            "projects": [{"projectId": billing, "name": "billing", "testSuites": []}],
            "results": [
                result("TC-3.json", "Passed", None),
                result("TC-4.json", "Passed", None),
            ],
        }),
    )
    .await;
    for run in [&nightly, &daily] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/test_runs/{run}/configurations"),
                &json!({"configId": chrome}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "linking {run}: {body}");
    }

    let (_, body) = send_json(&app, get(&format!("/reports/summary?projectId={checkout}"))).await;
    assert_eq!(body["total"], 2, "every checkout run: {body}");
    assert_eq!(body["passed"], 1);
    assert_eq!(body["failed"], 1);

    let (_, body) = send_json(
        &app,
        get(&format!("/reports/summary?configurationId={chrome}")),
    )
    .await;
    assert_eq!(body["total"], 3, "every run linking chrome: {body}");
    assert_eq!(body["passed"], 3);
    assert_eq!(body["failed"], 0);

    // Supplied together the filters intersect, keeping only the run that
    // satisfies each of them.
    let (_, body) = send_json(
        &app,
        get(&format!(
            "/reports/summary?projectId={checkout}&configurationId={chrome}"
        )),
    )
    .await;
    assert_eq!(body["total"], 1, "checkout runs linking chrome: {body}");
    assert_eq!(body["passed"], 1);
    assert_eq!(body["failed"], 0);
}

#[tokio::test]
async fn a_milestone_filter_keeps_only_the_runs_it_references() {
    let (_directory, app) = test_app();
    let nightly = record_run(
        &app,
        "nightly",
        "2026-09-04T00:00:00Z",
        json!([result("TC-1.json", "Passed", None)]),
    )
    .await;
    record_run(
        &app,
        "weekly",
        "2026-09-05T00:00:00Z",
        json!([
            result("TC-2.json", "Failed", None),
            result("TC-3.json", "Blocked", None),
        ]),
    )
    .await;

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/milestones",
            &json!({
                "milestoneId": "v1.0.json",
                "name": "v1.0",
                "testRunIds": [nightly],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating milestone: {body}");

    let (status, body) = send_json(&app, get("/reports/summary?milestoneId=v1.0.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(body["passed"], 1);
    assert_eq!(body["failed"], 0);
}

#[tokio::test]
async fn the_date_bounds_are_inclusive_and_leave_out_uncomparable_runs() {
    let (_directory, app) = test_app();
    for (name, timestamp) in [
        ("first", "2026-09-10T00:00:00Z"),
        ("last", "2026-09-12T23:59:59Z"),
        ("before", "2026-09-09T23:59:59Z"),
        ("after", "2026-09-13T00:00:00Z"),
        ("odd", "yesterday"),
    ] {
        record_run(
            &app,
            name,
            timestamp,
            json!([result("TC-1.json", "Passed", None)]),
        )
        .await;
    }

    let (status, body) =
        send_json(&app, get("/reports/summary?from=2026-09-10&to=2026-09-12")).await;

    assert_eq!(status, StatusCode::OK);
    // Both bounds are inclusive: the first and the last day match, the days on
    // either side do not, and the run whose timestamp is not a date at all is
    // left out while a bound is set.
    assert_eq!(body["total"], 2, "bounded report: {body}");

    // Without a bound the run with an unusable timestamp contributes again.
    let (_, body) = send_json(&app, get("/reports/summary")).await;
    assert_eq!(body["total"], 5);
}

#[tokio::test]
async fn an_unknown_project_or_milestone_is_not_found() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(&app, get("/reports/summary?projectId=missing.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");

    let (status, body) = send_json(&app, get("/reports/summary?milestoneId=missing.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
    assert_eq!(body["error"]["message"], "Milestone not found");
}

#[tokio::test]
async fn an_unknown_configuration_is_not_found() {
    let (_directory, app) = test_app();

    let (status, body) =
        send_json(&app, get("/reports/summary?configurationId=missing.json")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
    // A configuration carries the generic message the rest of the API uses for
    // a missing resource.
    assert_eq!(body["error"]["message"], "Resource not found");
}

#[tokio::test]
async fn an_unusable_filter_identifier_is_invalid_id() {
    let (_directory, app) = test_app();
    create_project(&app, "checkout").await;

    for query in ["projectId=nope", "milestoneId=nope", "configurationId=nope"] {
        let (status, body) = send_json(&app, get(&format!("/reports/summary?{query}"))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_error_envelope(&body, "invalid_id");
    }
}

#[tokio::test]
async fn an_unusable_date_filter_is_invalid_request() {
    let (_directory, app) = test_app();

    for query in ["from=yesterday", "to=2026-9-1"] {
        let (status, body) = send_json(&app, get(&format!("/reports/summary?{query}"))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_error_envelope(&body, "invalid_request");
    }
}
