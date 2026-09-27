//! The shallow child listing of project and suite reads (#415):
//! `?children=ids` answers the same document with its children's wire
//! identifiers instead of embedded child documents — the shape a context
//! tree fetches, priced at one folder walk.

mod common;

use axum::{Router, http::StatusCode};
use common::{
    create_case_in, create_project, create_suite, delete, get, json_request, send_json, test_app,
};
use serde_json::json;

async fn seed_tree(app: &Router) -> (String, String) {
    let project = create_project(app, "expansion").await;
    let suite = create_suite(app, &project, "suite-a").await;
    create_case_in(app, &format!("/projects/{project}/test_cases"), "TC-direct").await;
    create_case_in(app, &format!("/test_suites/{suite}/test_cases"), "TC-suite").await;
    (project, suite)
}

#[tokio::test]
async fn a_project_can_list_children_by_identifier_only() {
    let (_directory, app) = test_app();
    let (project, suite) = seed_tree(&app).await;

    // Full (default) shape: embedded objects.
    let (status, full) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        full["testSuites"][0]["suiteId"].is_string(),
        "the default embeds suite documents: {full}"
    );

    // Shallow shape: identifier strings only, same membership.
    let (status, ids) = send_json(&app, get(&format!("/projects/{project}?children=ids"))).await;
    assert_eq!(status, StatusCode::OK);
    let suites: Vec<&str> = ids["testSuites"]
        .as_array()
        .expect("suites")
        .iter()
        .map(|s| s.as_str().unwrap_or_default())
        .collect();
    assert_eq!(suites, [suite.as_str()], "suites answer as wire ids: {ids}");
    let cases: Vec<&str> = ids["testCases"]
        .as_array()
        .expect("direct cases")
        .iter()
        .map(|c| c.as_str().unwrap_or_default())
        .collect();
    assert_eq!(cases, ["TC-direct"], "direct cases answer as ids: {ids}");
    assert_eq!(ids["name"], "expansion", "the document itself is intact");

    // An unknown value is the documented default, not an error.
    let (status, _) = send_json(
        &app,
        get(&format!("/projects/{project}?children=everything")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A suite answers shallow too.
    let (status, suite_ids) =
        send_json(&app, get(&format!("/test_suites/{suite}?children=ids"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        suite_ids["testCases"],
        json!(["TC-suite"]),
        "the suite's cases answer as ids"
    );
}

#[tokio::test]
async fn shallow_reads_follow_writes_and_other_resources_are_untouched() {
    let (_directory, app) = test_app();
    let (project, _suite) = seed_tree(&app).await;
    send_json(&app, delete("/test_cases/TC-direct")).await;
    let (_, ids) = send_json(&app, get(&format!("/projects/{project}?children=ids"))).await;
    assert!(
        ids.get("testCases").is_none(),
        "an emptied direct-case list is omitted, as in the embedded shape: {ids}"
    );

    // A run does not hydrate children; the parameter changes nothing.
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_runs"),
            &json!({"name": "plain"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let run_id = created["id"].as_str().expect("run id").to_owned();
    let (status, _) = send_json(&app, get(&format!("/test_runs/{run_id}?children=ids"))).await;
    assert_eq!(status, StatusCode::OK, "the parameter is ignored elsewhere");
}
