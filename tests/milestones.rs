mod common;

use axum::http::StatusCode;
use common::{
    assert_error_envelope, case_body, create_project, create_suite, delete, fixture_home, get,
    json_request, send_json, test_app, xml_request,
};
use serde_json::{Value, json};

/// The five buckets a progress report splits its population into.
fn bucket_sum(progress: &Value) -> u64 {
    ["passed", "failed", "blocked", "untested", "retest"]
        .iter()
        .map(|bucket| {
            progress[*bucket]
                .as_u64()
                .unwrap_or_else(|| panic!("{bucket} is not a number: {progress}"))
        })
        .sum()
}

#[tokio::test]
async fn milestones_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/milestones")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
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
    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
            &json!({"name": "M1"}),
        ),
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
    // An empty population has nothing to pass, so the percentage is a defined
    // zero rather than a division by zero.
    assert_eq!(progress["passPercentage"], 0.0);
    assert_eq!(bucket_sum(&progress), 0);
}

#[tokio::test]
async fn creating_a_milestone_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
            &json!({"description": "no name"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
}

#[tokio::test]
async fn milestone_progress_aggregates_linked_test_runs() {
    let (_directory, app) = test_app();

    // 1. Create test run RUN-1.json with results
    let home = fixture_home(&app).await;
    send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
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
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
            &json!({
                "milestoneId": "M-1.json",
                "name": "Sprint 42",
                "testRunIds": ["RUN-1.json"]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");

    // 3. Fetch progress for M-1.json
    let (status, progress) = send_json(&app, get("/milestones/M-1.json/progress")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(progress["milestoneId"], "M-1.json");
    assert_eq!(progress["totalCases"], 3);
    assert_eq!(progress["passed"], 1);
    assert_eq!(progress["failed"], 1);
    assert_eq!(progress["blocked"], 1);
    assert_eq!(progress["passPercentage"], 33.33333333333333);
    assert_eq!(
        bucket_sum(&progress),
        progress["totalCases"].as_u64().unwrap()
    );
}

#[tokio::test]
async fn progress_counts_the_cases_a_linked_suite_embeds() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    let suite = create_suite(&app, &home, "SMOKE").await;
    for case in ["TC-1.json", "TC-2.json", "TC-3.json"] {
        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/test_suites/{suite}/test_cases"),
                &case_body(case),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "creating {case}: {created}");
    }

    // The run's own snapshot names only one of the three cases.
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({
                "testRunId": "RUN-1.json",
                "name": "RUN-1",
                "timestamp": "2026-09-04T00:00:00Z",
                "testCases": [case_body("TC-1.json")]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");

    // The linked suite brings its cases with it, so the run holds all three.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/RUN-1.json/test_suites",
            &json!({"suiteId": suite}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "linking the suite: {body}");

    for (case, status) in [("TC-2.json", "Passed"), ("TC-3.json", "Failed")] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/RUN-1.json/results",
                &json!({"testCaseId": case, "status": status, "timestamp": "2026-09-04T12:00:00Z"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "recording {case}: {body}");
    }

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
            &json!({
                "milestoneId": "M-1.json",
                "name": "Sprint 42",
                "testRunIds": ["RUN-1.json"]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");

    // Every case the run holds is counted once: the declared case, the two the
    // suite embedded alongside it, and the two recorded outcomes.
    let (status, progress) = send_json(&app, get("/milestones/M-1.json/progress")).await;
    assert_eq!(status, StatusCode::OK, "progress: {progress}");
    assert_eq!(progress["totalCases"], 3);
    assert_eq!(progress["passed"], 1);
    assert_eq!(progress["failed"], 1);
    assert_eq!(progress["blocked"], 0);
    assert_eq!(progress["untested"], 1);
    assert_eq!(progress["retest"], 0);
    assert_eq!(progress["passPercentage"], 33.33333333333333);
    assert_eq!(bucket_sum(&progress), 3);
}

#[tokio::test]
async fn progress_counts_recorded_cases_the_snapshot_never_declared() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({
                "testRunId": "RUN-1.json",
                "name": "RUN-1",
                "timestamp": "2026-09-04T00:00:00Z",
                "testCases": [case_body("TC-1.json")]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");

    // The import writes a result for every case the report names, whether or
    // not the run's snapshot declared it.
    let report = r#"<testsuite name="Checkout" timestamp="2026-09-10T12:00:00Z">
        <testcase classname="Checkout" name="pays"/>
        <testcase classname="Checkout" name="declines"><failure message="card declined"/></testcase>
        <testcase classname="Checkout" name="times out"><error message="timeout"/></testcase>
        <testcase classname="Checkout" name="is skipped"><skipped/></testcase>
      </testsuite>"#;
    let (status, summary) = send_json(
        &app,
        xml_request("/test_runs/RUN-1.json/import/junit", report.to_owned()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(summary["imported"], 4);

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
            &json!({
                "milestoneId": "M-1.json",
                "name": "Sprint 42",
                "testRunIds": ["RUN-1.json"]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");

    // The declared case has no result and the four imported ones were never
    // declared: all five are in the population, so the buckets still sum to the
    // total and the percentage stays inside 0..=100.
    let (status, progress) = send_json(&app, get("/milestones/M-1.json/progress")).await;
    assert_eq!(status, StatusCode::OK, "progress: {progress}");
    assert_eq!(progress["totalCases"], 5);
    assert_eq!(progress["passed"], 1);
    assert_eq!(progress["failed"], 2);
    assert_eq!(progress["blocked"], 1);
    assert_eq!(progress["untested"], 1);
    assert_eq!(progress["retest"], 0);
    assert_eq!(progress["passPercentage"], 20.0);
    assert_eq!(bucket_sum(&progress), 5);
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

    let home = fixture_home(&app).await;
    send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
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

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
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
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");

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

    let home = fixture_home(&app).await;
    for id in ["M-1.json", "M-2.json"] {
        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{home}/milestones"),
                &json!({"milestoneId": id, "name": id}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "creating {id}: {created}");
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

    let home = fixture_home(&app).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/milestones"),
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

/// Deleting a milestone through a project resolves the project the route names,
/// so an identifier two projects hold is removed one home at a time and the
/// other home is left alone.
#[tokio::test]
async fn deleting_a_milestone_through_a_project_resolves_that_project() {
    let (_directory, app) = test_app();

    let alpha = create_project(&app, "alpha").await;
    let beta = create_project(&app, "beta").await;
    for project in [&alpha, &beta] {
        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{project}/milestones"),
                &json!({"name": "M-1", "status": "Open"}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "creating in {project}: {created}"
        );
        assert_eq!(created["id"], "M-1.json");
    }

    // While two projects hold the identifier, the global route refuses it: a
    // bare identifier cannot say which occurrence was meant.
    let (status, body) = send_json(&app, delete("/milestones/M-1.json")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_error_envelope(&body, "conflict");

    let (status, body) = send_json(
        &app,
        delete(&format!("/projects/{alpha}/milestones/M-1.json")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Milestone deleted");

    let (status, owned) = send_json(&app, get(&format!("/projects/{alpha}/milestones"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owned, json!([]), "the named project lost the occurrence");
    let (status, owned) = send_json(&app, get(&format!("/projects/{beta}/milestones"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owned, json!(["M-1.json"]), "the other home is untouched");
    let (status, stored) = send_json(&app, get("/milestones/M-1.json")).await;
    assert_eq!(status, StatusCode::OK, "one home resolves again: {stored}");
    assert_eq!(stored["status"], "Open");

    // The occurrence this project owned is gone, so a second delete has nothing
    // left to remove.
    let (status, body) = send_json(
        &app,
        delete(&format!("/projects/{alpha}/milestones/M-1.json")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_error_envelope(&body, "not_found");
}
