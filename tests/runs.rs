mod common;

use axum::Router;
use axum::http::StatusCode;
use common::{
    app_at, assert_error_envelope, create_case_in, create_named, create_project, create_suite,
    delete, fixture_home, get, json_request, raw_json_request, send_json, test_app, xml_request,
};
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn test_runs_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/test_runs")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
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

    // The run stores the `testRunId` the create contract copied from the body,
    // so a client putting back what it read restates that identity rather than
    // naming the document by its address.
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/nightly.json",
            &json!({"testRunId": "R-001", "name": "nightly", "timestamp": "2026-09-03T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "restating the stored identity: {body}"
    );

    let (_, updated) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(updated["testRunId"], "R-001");
    assert_eq!(updated["timestamp"], "2026-09-03T00:00:00Z");

    let (status, _) = send_json(&app, delete("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_run_created_from_a_name_alone_reads_back_and_records_results() {
    let (directory, app) = test_app();

    // The body names the run and nothing else: no `testRunId`, no `timestamp`.
    let home = fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({"name": "nightly"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");
    assert_eq!(created["id"], "nightly.json");

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["testRunId"], "nightly.json");
    assert_eq!(stored["name"], "nightly");
    let recorded_at = stored["timestamp"]
        .as_str()
        .expect("a stored run records when it was created");
    assert!(
        recorded_at.parse::<u64>().is_ok(),
        "the recorded timestamp is Unix seconds as a string, got {recorded_at}"
    );

    // The stored document itself satisfies the model, not just the response.
    let marker = directory.path().join(format!(
        "projects/{}/test_runs/nightly.json",
        common::project_folder(&home)
    ));
    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&marker).expect("run readable"))
            .expect("run is valid JSON");
    assert_eq!(on_disk["testRunId"], "nightly.json");
    assert_eq!(on_disk["timestamp"], stored["timestamp"]);

    // A value the body supplied is kept rather than replaced.
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({"name": "morning", "timestamp": "2026-09-02T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, morning) = send_json(&app, get("/test_runs/morning.json")).await;
    assert_eq!(morning["testRunId"], "morning.json");
    assert_eq!(morning["timestamp"], "2026-09-02T00:00:00Z");

    // Every route that deserialises the run answers instead of failing.
    let project = home;
    create_suite(&app, &project, "smoke").await;
    create_case_in(
        &app,
        &format!("/projects/{project}/test_cases"),
        "TC-001.json",
    )
    .await;

    for (route, payload) in [
        (
            "/test_runs/nightly.json/test_suites",
            json!({"suiteId": "smoke.json"}),
        ),
        (
            "/test_runs/nightly.json/test_cases",
            json!({"testCaseId": "TC-001.json"}),
        ),
        (
            "/test_runs/nightly.json/results",
            json!({"testCaseId": "TC-001.json", "status": "Passed", "timestamp": "1"}),
        ),
    ] {
        let (status, body) = send_json(&app, json_request("POST", route, &payload)).await;
        assert_eq!(status, StatusCode::OK, "POST {route}: {body}");
    }

    let (_, recorded) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(recorded["results"][0]["status"], "Passed");
    assert_eq!(recorded["testSuites"][0]["suiteId"], "smoke.json");
}

#[tokio::test]
async fn creating_a_test_run_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    for payload in [json!({"testRunId": "R-001"}), json!({"name": ""})] {
        let (status, body) = send_json(
            &app,
            json_request("POST", &format!("/projects/{home}/test_runs"), &payload),
        )
        .await;
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

    let home = fixture_home(&app).await;
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({"name": "nightly"}),
        ),
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
    let home = fixture_home(&app).await;
    send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({"testRunId": "RUN-1.json", "name": "run1", "timestamp": "2026-09-04T00:00:00Z"}),
        ),
    )
    .await;

    send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({"testRunId": "RUN-2.json", "name": "run2", "timestamp": "2026-09-04T00:00:00Z"}),
        ),
    )
    .await;

    let project = home;
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

    // 5. Give RUN-2.json the same case, then record a different result for it
    // (Run isolation)
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/run2.json/test_cases",
            &json!({"testCaseId": "TC-001.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

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

#[tokio::test]
async fn a_partial_update_keeps_the_fields_the_body_leaves_out() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({
                "testRunId": "R-001",
                "name": "nightly",
                "timestamp": "2026-09-02T00:00:00Z",
                "tags": ["ci"],
                "testCases": [
                    {"testCaseId": "TC-001.json", "title": "Login", "expectedResult": "Stored"}
                ],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(
        &app,
        json_request("PUT", "/test_runs/nightly.json", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "updating: {body}");

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["testRunId"], "R-001");
    assert_eq!(stored["name"], "nightly");
    assert_eq!(stored["timestamp"], "2026-09-02T00:00:00Z");
    assert_eq!(stored["tags"], json!(["ci"]));
    assert_eq!(stored["testCases"][0]["testCaseId"], "TC-001.json");

    // The run still reads back as its model, so recording a result works.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-001.json", "status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording: {body}");

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["results"][0]["status"], "Passed");
    assert_eq!(stored["name"], "nightly");
}

#[tokio::test]
async fn a_re_recorded_result_keeps_what_the_request_leaves_out() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;

    // A result that describes itself fully, with a defect linked to it.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({
                "testCaseId": "TC-1",
                "status": "Failed",
                "timestamp": "2026-09-04T12:00:00Z",
                "notes": "card declined",
                "durationMs": 1200,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording: {body}");

    let (status, linked) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results/TC-1/defects",
            &json!({
                "defectId": "BUG-1",
                "defectUrl": "https://tracker.example/BUG-1",
                "trackerType": "custom",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "linking: {linked}");

    // Re-recording names what it changes and no more: every field the body
    // leaves out keeps the value the stored result carries...
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-1", "status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "re-recording: {body}");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    let result = &run["results"][0];
    assert_eq!(result["status"], "Passed");
    // `status` and `timestamp` are the two fields a recording always describes,
    // so an omitted `timestamp` is the current time rather than the stored one.
    let recorded_at = result["timestamp"]
        .as_str()
        .expect("a recorded result carries a timestamp");
    assert!(
        recorded_at.parse::<u64>().is_ok(),
        "an omitted timestamp falls back to Unix seconds, got {recorded_at}"
    );
    assert_eq!(result["notes"], "card declined");
    assert_eq!(result["durationMs"], 1200);
    // ...including the defect links, which a recording request cannot describe
    // at all: they survive a re-recording because the body never mentions them.
    assert_eq!(result["defectLinks"][0]["defectId"], "BUG-1");

    // An explicit `null` is how a request clears a field it no longer carries,
    // and a value it supplies is written over the stored one.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({
                "testCaseId": "TC-1",
                "status": "Passed",
                "timestamp": "2026-09-04T13:00:00Z",
                "notes": null,
                "durationMs": 900,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "clearing: {body}");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    let result = &run["results"][0];
    assert_eq!(result["timestamp"], "2026-09-04T13:00:00Z");
    assert!(
        result.get("notes").is_none(),
        "an explicit null clears the stored notes: {result}"
    );
    assert_eq!(result["durationMs"], 900);
    assert_eq!(result["defectLinks"][0]["defectId"], "BUG-1");
}

#[tokio::test]
async fn a_result_body_is_checked_rather_than_read_field_by_field() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;

    let bad_bodies = [
        // Not an object at all.
        json!("TC-1"),
        // Fields a client may know from elsewhere but this route does not take.
        json!({"testCaseId": "TC-1", "status": "Passed", "comment": "looks fine"}),
        json!({"testCaseId": "TC-1", "status": "Passed", "duration": 1200}),
        // A required field left out, or supplied empty.
        json!({"testCaseId": "TC-1"}),
        json!({"status": "Passed"}),
        json!({"testCaseId": "", "status": "Passed"}),
        // Malformed optionals, none of which is quietly dropped.
        json!({"testCaseId": "TC-1", "status": "Passed", "timestamp": 1789735695}),
        json!({"testCaseId": "TC-1", "status": "Passed", "timestamp": ""}),
        json!({"testCaseId": "TC-1", "status": "Passed", "notes": 7}),
        json!({"testCaseId": "TC-1", "status": "Passed", "durationMs": -5}),
        json!({"testCaseId": "TC-1", "status": "Passed", "durationMs": 1.5}),
        json!({"testCaseId": "TC-1", "status": "Passed", "durationMs": "1200"}),
    ];
    for body in bad_bodies {
        let (status, response) = send_json(
            &app,
            json_request("POST", "/test_runs/nightly.json/results", &body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body {body}: {response}");
        assert_error_envelope(&response, "invalid_request");
    }

    // A status the API does not know is its own error, and is the one check the
    // route has always made.
    let (status, response) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-1", "status": "Nope"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&response, "invalid_status");

    // Every rejection happened before the write, so the run records nothing.
    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert!(run["results"].is_null(), "nothing was written: {run}");
}

#[tokio::test]
async fn a_result_is_refused_for_a_case_the_run_does_not_hold() {
    let (_directory, app) = test_app();
    let project = fixture_home(&app).await;
    create_run_holding(&app, "nightly", &["TC-1"]).await;

    // The run declares TC-1 and nothing else, so a result for any other case is
    // a 404: a run that records a case it never picked up is a document no
    // client can render faithfully.
    for case_id in ["TC-NOT-IN-RUN", "TC-2"] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/nightly.json/results",
                &json!({"testCaseId": case_id, "status": "Passed"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "recording {case_id}: {body}");
        assert_error_envelope(&body, "not_found");
    }

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert!(run["results"].is_null(), "nothing was written: {run}");

    // A run holds the cases its own suites declare too, so a case the run picked
    // up through a suite is recordable.
    let suite = create_suite(&app, &project, "smoke").await;
    create_case_in(&app, &format!("/test_suites/{suite}/test_cases"), "TC-002").await;
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/test_suites",
            &json!({"suiteId": suite}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "adding the suite: {body}");

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-002", "status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording through a suite: {body}");
}

#[tokio::test]
async fn replacing_a_result_keeps_the_defects_it_cannot_describe() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;

    // A result that describes itself fully, with a defect linked to it.
    let (status, recorded) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({
                "testCaseId": "TC-1",
                "status": "Failed",
                "timestamp": "2026-09-04T12:00:00Z",
                "notes": "card declined",
                "durationMs": 1200,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording: {recorded}");

    let (status, linked) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results/TC-1/defects",
            &json!({
                "defectId": "BUG-1",
                "defectUrl": "https://tracker.example/BUG-1",
                "trackerType": "custom",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "linking: {linked}");

    // The path names the result, so the body need not: a replacement that
    // carries only the status the case now records is complete.
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/nightly.json/results/TC-1",
            &json!({"status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "replacing: {body}");
    assert_eq!(body, json!({"message": "Test result replaced in run"}));

    // It rewrites the stored result rather than adding one, and keeps what the
    // body cannot describe: the links already hanging off the result survive,
    // and so do the fields the request leaves out.
    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    let results = run["results"].as_array().expect("results array");
    assert_eq!(results.len(), 1, "a replacement does not add one: {run}");
    let result = &results[0];
    assert_eq!(result["testCaseId"], "TC-1");
    assert_eq!(result["status"], "Passed");
    assert_eq!(result["notes"], "card declined");
    assert_eq!(result["durationMs"], 1200);
    assert_eq!(result["defectLinks"][0]["defectId"], "BUG-1");

    // `status` and `timestamp` are the two fields a replacement always
    // describes, exactly as a recording does, so an omitted `timestamp` is the
    // current time rather than the stored one.
    let replaced_at = result["timestamp"]
        .as_str()
        .expect("a replaced result carries a timestamp");
    assert!(
        replaced_at.parse::<u64>().is_ok(),
        "an omitted timestamp falls back to Unix seconds, got {replaced_at}"
    );

    // A body may name the case as long as it agrees with the path, and an
    // explicit `null` clears a stored field.
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/nightly.json/results/TC-1",
            &json!({
                "testCaseId": "TC-1",
                "status": "Blocked",
                "timestamp": "2026-09-04T13:00:00Z",
                "notes": null,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "replacing again: {body}");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    let result = &run["results"][0];
    assert_eq!(result["status"], "Blocked");
    assert_eq!(result["timestamp"], "2026-09-04T13:00:00Z");
    assert!(
        result.get("notes").is_none(),
        "an explicit null clears the stored notes: {result}"
    );
    assert_eq!(result["defectLinks"][0]["defectId"], "BUG-1");

    // A body that points the replacement at another case is refused rather than
    // quietly retargeted, and the mismatch writes nothing.
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/nightly.json/results/TC-1",
            &json!({"testCaseId": "TC-2", "status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "mismatched case: {body}");
    assert_error_envelope(&body, "invalid_request");

    // The replacement validates its body the way a recording does, so a body
    // that names no status is refused before the run is read.
    let (status, body) = send_json(
        &app,
        json_request("PUT", "/test_runs/nightly.json/results/TC-1", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no status: {body}");
    assert_error_envelope(&body, "invalid_request");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    let result = &run["results"][0];
    assert_eq!(result["status"], "Blocked", "nothing was written: {run}");

    // A replacement never creates: a run that stores no result for the case —
    // whether it records other cases or no results at all — answers 404, and so
    // does a run that does not exist.
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/nightly.json/results/TC-2",
            &json!({"status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "absent result: {body}");
    assert_error_envelope(&body, "not_found");

    create_run_holding(&app, "empty", &["TC-9"]).await;
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/empty.json/results/TC-9",
            &json!({"status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no results at all: {body}");
    assert_error_envelope(&body, "not_found");

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/missing.json/results/TC-1",
            &json!({"status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown run: {body}");
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn removing_a_result_takes_it_out_of_the_run() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1", "TC-2"]).await;

    for case_id in ["TC-1", "TC-2"] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/nightly.json/results",
                &json!({"testCaseId": case_id, "status": "Failed"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "recording {case_id}: {body}");
    }

    // The path names the result, and removing one takes out that result alone.
    let (status, body) = send_json(&app, delete("/test_runs/nightly.json/results/TC-1")).await;
    assert_eq!(status, StatusCode::OK, "removing: {body}");
    assert_eq!(body, json!({"message": "Test result removed from run"}));

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    let results = run["results"].as_array().expect("results array");
    assert_eq!(results.len(), 1, "one result is left: {run}");
    assert_eq!(results[0]["testCaseId"], "TC-2");

    // The result is gone as a result, not merely emptied: the routes that hang
    // off it no longer find one.
    let (status, body) = send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "defects of a removed result: {body}"
    );
    assert_error_envelope(&body, "not_found");

    // The same result cannot be removed twice, a case the run records no result
    // for is not found, and neither is a case of a run that records nothing.
    let (status, body) = send_json(&app, delete("/test_runs/nightly.json/results/TC-1")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "removing twice: {body}");
    assert_error_envelope(&body, "not_found");

    let (status, body) = send_json(&app, delete("/test_runs/nightly.json/results/TC-9")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "never recorded: {body}");
    assert_error_envelope(&body, "not_found");

    create_run_holding(&app, "empty", &["TC-9"]).await;
    let (status, body) = send_json(&app, delete("/test_runs/empty.json/results/TC-9")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no results at all: {body}");
    assert_error_envelope(&body, "not_found");

    // A run that does not exist is not found as a run.
    let (status, body) = send_json(&app, delete("/test_runs/missing.json/results/TC-1")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown run: {body}");
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn a_run_links_and_unlinks_a_top_level_configuration() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );
    assert_eq!(
        create_named(&app, "/configurations", "chrome-linux").await,
        "chrome-linux.json"
    );

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/configurations",
            &json!({"configId": "chrome-linux.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "linking: {body}");

    // The run keeps a reference, not a copy of the configuration document.
    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        stored["configurations"][0],
        json!({"configId": "chrome-linux.json", "name": "chrome-linux"})
    );

    // Linking the same configuration twice is a conflict.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/configurations",
            &json!({"configId": "chrome-linux.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");

    let (status, body) = send_json(
        &app,
        delete("/test_runs/nightly.json/configurations/chrome-linux.json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unlinking: {body}");

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(stored["configurations"], json!([]));
}

#[tokio::test]
async fn a_configuration_link_validates_the_run_the_configuration_and_the_body() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );
    assert_eq!(
        create_named(&app, "/configurations", "chrome-linux").await,
        "chrome-linux.json"
    );

    // An unknown run is a 404 even when the configuration exists.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/missing.json/configurations",
            &json!({"configId": "chrome-linux.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");

    // An unknown configuration is a 404 even when the run exists.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/configurations",
            &json!({"configId": "missing.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");

    // The body must name the configuration it links.
    let (status, body) = send_json(
        &app,
        json_request("POST", "/test_runs/nightly.json/configurations", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
}

#[tokio::test]
async fn unlinking_a_configuration_the_run_does_not_reference_is_not_found() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );

    let (status, body) = send_json(
        &app,
        delete("/test_runs/nightly.json/configurations/chrome-linux.json"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");

    let (status, body) = send_json(
        &app,
        delete("/test_runs/missing.json/configurations/chrome-linux.json"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn duplicating_a_run_preserves_its_configuration_links() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );
    assert_eq!(
        create_named(&app, "/configurations", "chrome-linux").await,
        "chrome-linux.json"
    );
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/configurations",
            &json!({"configId": "chrome-linux.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/duplicate",
            &json!({"newId": "copy.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "duplicating: {body}");

    let (_, copy) = send_json(&app, get("/test_runs/copy.json")).await;
    assert_eq!(
        copy["configurations"][0],
        json!({"configId": "chrome-linux.json", "name": "chrome-linux"})
    );

    let (_, original) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(
        original["configurations"][0]["configId"],
        "chrome-linux.json"
    );
}

#[tokio::test]
async fn configuration_links_survive_a_repository_restart() {
    let directory = TempDir::new().expect("temp dir");

    {
        let app = app_at(directory.path());
        assert_eq!(
            create_named(&app, "/test_runs", "nightly").await,
            "nightly.json"
        );
        assert_eq!(
            create_named(&app, "/configurations", "chrome-linux").await,
            "chrome-linux.json"
        );
        let (status, _) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/nightly.json/configurations",
                &json!({"configId": "chrome-linux.json"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let app = app_at(directory.path());
    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        stored["configurations"][0],
        json!({"configId": "chrome-linux.json", "name": "chrome-linux"})
    );
}

#[tokio::test]
async fn listing_runs_filters_by_the_configuration_they_link() {
    let (_directory, app) = test_app();

    let home = fixture_home(&app).await;
    for (id, name) in [("R-1", "nightly"), ("R-2", "weekly"), ("R-3", "release")] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{home}/test_runs"),
                &json!({
                    "testRunId": id,
                    "name": name,
                    "timestamp": "2026-09-02T00:00:00Z",
                    "tags": ["ci"],
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "creating {name}: {body}");
    }
    for name in ["chrome-linux", "firefox-windows"] {
        assert_eq!(
            create_named(&app, "/configurations", name).await,
            format!("{name}.json")
        );
    }
    for (run, configuration) in [("nightly", "chrome-linux"), ("weekly", "firefox-windows")] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/test_runs/{run}.json/configurations"),
                &json!({"configId": format!("{configuration}.json")}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "linking {run}: {body}");
    }

    // Only the runs that link the configuration are listed, so the run that
    // links nothing is excluded.
    let (status, listing) =
        send_json(&app, get("/test_runs?configuration=chrome-linux.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["nightly.json"]));

    let (status, listing) =
        send_json(&app, get("/test_runs?configuration=firefox-windows.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["weekly.json"]));

    // A configuration no run links yields an empty listing, not a 404, matching
    // how the substring and tag filters already behave.
    let (status, listing) =
        send_json(&app, get("/test_runs?configuration=firefox-linux.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    // The configuration filter composes with the substring and tag filters, and
    // the substring filter can still exclude a linked run.
    let (status, listing) = send_json(
        &app,
        get("/test_runs?filter=NIGHT&tags=ci&configuration=chrome-linux.json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["nightly.json"]));

    let (status, listing) = send_json(
        &app,
        get("/test_runs?filter=WEEK&configuration=chrome-linux.json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, listing) = send_json(
        &app,
        get("/test_runs?tags=ci&configuration=chrome-linux.json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["nightly.json"]));

    // Only runs carry configuration references, so the parameter is inert for
    // the other listings rather than emptying them.
    let (status, listing) =
        send_json(&app, get("/configurations?configuration=chrome-linux.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing,
        json!(["chrome-linux.json", "firefox-windows.json"])
    );
}

/// Creates a run to import into and returns the identifier.
async fn create_run(app: &Router, name: &str) -> String {
    let project = fixture_home(app).await;
    let (status, created) = send_json(
        app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_runs"),
            &json!({"name": name}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "creating run {name}: {created}"
    );
    created["id"].as_str().expect("run id").to_owned()
}

/// Creates a run that holds `cases` — each declared as the embedded snapshot
/// the run was built from — and returns the identifier.
///
/// Recording a result is only legal for a case the run holds, so a test that
/// records one without importing it declares the case here rather than relying
/// on the run being a place where anything can be written.
async fn create_run_holding(app: &Router, name: &str, cases: &[&str]) -> String {
    let project = fixture_home(app).await;
    let declared: Vec<serde_json::Value> = cases.iter().map(|id| common::case_body(id)).collect();
    let (status, created) = send_json(
        app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_runs"),
            &json!({"name": name, "testCases": declared}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "creating run {name}: {created}"
    );
    created["id"].as_str().expect("run id").to_owned()
}

#[tokio::test]
async fn a_junit_report_imports_and_maps_statuses() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    let report = r#"<testsuite name="Checkout" timestamp="2026-09-10T12:00:00Z">
        <testcase classname="Checkout" name="pays"/>
        <testcase classname="Checkout" name="declines"><failure message="card declined"/></testcase>
        <testcase classname="Checkout" name="times out"><error message="timeout"/></testcase>
        <testcase classname="Checkout" name="is skipped"><skipped/></testcase>
      </testsuite>"#;
    let (status, summary) = send_json(
        &app,
        xml_request("/test_runs/nightly.json/import/junit", report.to_owned()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(
        summary,
        json!({
            "imported": 4,
            "skipped": 0,
            "errors": 0,
            "duplicates": 0,
            "summary": {"passed": 1, "failed": 2, "blocked": 1},
        })
    );

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    let results = stored["results"].as_array().expect("results recorded");
    let statuses: Vec<(&str, &str)> = results
        .iter()
        .map(|result| {
            (
                result["testCaseId"].as_str().expect("case id"),
                result["status"].as_str().expect("status"),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("Checkout.pays", "Passed"),
            ("Checkout.declines", "Failed"),
            ("Checkout.times out", "Failed"),
            ("Checkout.is skipped", "Blocked"),
        ]
    );

    // The failing case carries the report's message as its notes, and every case
    // records the enclosing suite's timestamp rather than the wall clock.
    assert_eq!(results[1]["notes"], "card declined");
    assert_eq!(results[2]["notes"], "timeout");
    assert_eq!(results[0]["timestamp"], "2026-09-10T12:00:00Z");
    assert_eq!(results[3]["timestamp"], "2026-09-10T12:00:00Z");
}

#[tokio::test]
async fn a_junit_report_counts_duplicates_and_leaves_them_alone() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["Checkout.pays"]).await;

    // A result recorded by hand first, so the import meets an existing one.
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "Checkout.pays", "status": "Failed", "timestamp": "1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The same case appears twice in the report as well as in the run, and only
    // the case the run does not know is written.
    let report = r#"<testsuite>
        <testcase classname="Checkout" name="pays"/>
        <testcase classname="Checkout" name="pays"/>
        <testcase classname="Checkout" name="checks out"/>
      </testsuite>"#;
    let (status, summary) = send_json(
        &app,
        xml_request("/test_runs/nightly.json/import/junit", report.to_owned()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(
        summary,
        json!({
            "imported": 1,
            "skipped": 2,
            "errors": 0,
            "duplicates": 2,
            "summary": {"passed": 1, "failed": 0, "blocked": 0},
        })
    );

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    let results = stored["results"].as_array().expect("results recorded");
    assert_eq!(results.len(), 2);
    // The hand-recorded result survives the import untouched.
    assert_eq!(results[0]["testCaseId"], "Checkout.pays");
    assert_eq!(results[0]["status"], "Failed");
    assert_eq!(results[0]["timestamp"], "1");
    assert_eq!(results[1]["testCaseId"], "Checkout.checks out");
    assert_eq!(results[1]["status"], "Passed");
}

#[tokio::test]
async fn a_junit_report_without_a_suite_timestamp_uses_the_current_time() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    let report = r#"<testsuite><testcase classname="Suite" name="bare"/></testsuite>"#;
    let (status, summary) = send_json(
        &app,
        xml_request("/test_runs/nightly.json/import/junit", report.to_owned()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(summary["imported"], 1);

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    let recorded_at = stored["results"][0]["timestamp"]
        .as_str()
        .expect("a timestamp is always recorded");
    assert!(
        recorded_at.parse::<u64>().is_ok(),
        "a report without a timestamp falls back to Unix seconds, got {recorded_at}"
    );
}

#[tokio::test]
async fn a_testcase_without_a_name_is_counted_as_an_error() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    let report = r#"<testsuite>
        <testcase classname="Suite"/>
        <testcase classname="Suite" name="named"/>
      </testsuite>"#;
    let (status, summary) = send_json(
        &app,
        xml_request("/test_runs/nightly.json/import/junit", report.to_owned()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(
        summary,
        json!({
            "imported": 1,
            "skipped": 1,
            "errors": 1,
            "duplicates": 0,
            "summary": {"passed": 1, "failed": 0, "blocked": 0},
        })
    );

    // The nameless case is not written; the named one is.
    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    let results = stored["results"].as_array().expect("results recorded");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["testCaseId"], "Suite.named");
}

#[tokio::test]
async fn malformed_junit_xml_is_rejected_and_writes_nothing() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    let (status, body) = send_json(
        &app,
        xml_request(
            "/test_runs/nightly.json/import/junit",
            "<testsuite><testcase></testsuite>".to_owned(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    // A body that is not UTF-8 is rejected the same way.
    let (status, body) = send_json(
        &app,
        xml_request("/test_runs/nightly.json/import/junit", vec![0xff, 0xfe]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert!(stored["results"].is_null(), "nothing was written: {stored}");
}

#[tokio::test]
async fn importing_into_an_unknown_run_is_not_found() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        xml_request(
            "/test_runs/missing.json/import/junit",
            "<testsuite/>".to_owned(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn an_over_deep_junit_document_is_rejected_without_aborting_the_server() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    // Deep nesting, not a large body: the audit found that ~3,410 nested
    // elements overflowed the worker stack and aborted the process. The
    // document is refused before the parser recurses, so the request answers a
    // client error and the server keeps serving.
    let mut report = String::new();
    for _ in 0..5_000 {
        report.push_str("<testsuite>");
    }
    for _ in 0..5_000 {
        report.push_str("</testsuite>");
    }

    let (status, body) = send_json(
        &app,
        xml_request("/test_runs/nightly.json/import/junit", report),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "over-deep: {body}");
    assert_error_envelope(&body, "invalid_request");

    // The same process answers the next request, and the refused body wrote
    // nothing into the run.
    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK, "the server survived: {stored}");
    assert!(stored["results"].is_null(), "nothing was written: {stored}");
}

#[tokio::test]
async fn a_json_import_maps_its_fields_and_defaults_the_timestamp() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    let (status, summary) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/import/json",
            &json!([
                {"testCaseId": "TC-1", "status": "Passed", "notes": "looks good", "timestamp": "2026-09-10T12:00:00Z"},
                {"testCaseId": "TC-2", "status": "Failed"},
                {"testCaseId": "TC-3", "status": "Blocked", "notes": "environment down"},
            ]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(
        summary,
        json!({
            "imported": 3,
            "skipped": 0,
            "errors": 0,
            "duplicates": 0,
            "summary": {"passed": 1, "failed": 1, "blocked": 1},
        })
    );

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    let results = stored["results"].as_array().expect("results recorded");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["testCaseId"], "TC-1");
    assert_eq!(results[0]["status"], "Passed");
    assert_eq!(results[0]["notes"], "looks good");
    assert_eq!(results[0]["timestamp"], "2026-09-10T12:00:00Z");
    assert_eq!(results[1]["testCaseId"], "TC-2");
    assert_eq!(results[1]["status"], "Failed");
    assert_eq!(results[2]["testCaseId"], "TC-3");
    assert_eq!(results[2]["status"], "Blocked");
    assert_eq!(results[2]["notes"], "environment down");

    // An entry without a timestamp falls back to Unix seconds, like the
    // single-result route.
    let recorded_at = results[1]["timestamp"]
        .as_str()
        .expect("a timestamp is always recorded");
    assert!(
        recorded_at.parse::<u64>().is_ok(),
        "a missing timestamp falls back to Unix seconds, got {recorded_at}"
    );
}

#[tokio::test]
async fn a_json_import_may_wrap_its_entries_in_a_results_field() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    let (status, summary) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/import/json",
            &json!({"results": [{"testCaseId": "TC-1", "status": "Passed"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(
        summary,
        json!({
            "imported": 1,
            "skipped": 0,
            "errors": 0,
            "duplicates": 0,
            "summary": {"passed": 1, "failed": 0, "blocked": 0},
        })
    );

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(stored["results"][0]["testCaseId"], "TC-1");
    assert_eq!(stored["results"][0]["status"], "Passed");
}

#[tokio::test]
async fn a_json_import_counts_duplicates_and_leaves_them_alone() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;

    // A result recorded by hand first, so the import meets an existing one.
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-1", "status": "Failed", "timestamp": "1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // TC-1 is already recorded and also appears twice in the body; only the case
    // the run does not know is written.
    let (status, summary) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/import/json",
            &json!([
                {"testCaseId": "TC-1", "status": "Passed"},
                {"testCaseId": "TC-1", "status": "Passed"},
                {"testCaseId": "TC-2", "status": "Passed"},
            ]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "importing: {summary}");
    assert_eq!(
        summary,
        json!({
            "imported": 1,
            "skipped": 2,
            "errors": 0,
            "duplicates": 2,
            "summary": {"passed": 1, "failed": 0, "blocked": 0},
        })
    );

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    let results = stored["results"].as_array().expect("results recorded");
    assert_eq!(results.len(), 2);
    // The hand-recorded result survives the import untouched.
    assert_eq!(results[0]["testCaseId"], "TC-1");
    assert_eq!(results[0]["status"], "Failed");
    assert_eq!(results[0]["timestamp"], "1");
    assert_eq!(results[1]["testCaseId"], "TC-2");
    assert_eq!(results[1]["status"], "Passed");
}

#[tokio::test]
async fn a_json_import_rejects_an_unusable_entry_and_writes_nothing() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    for body in [
        // A required field is missing.
        json!([{"status": "Passed"}]),
        // The identifier is present but empty.
        json!([{"testCaseId": "", "status": "Passed"}]),
        json!([{"testCaseId": "TC-1"}]),
        // A field carries the wrong type.
        json!([{"testCaseId": "TC-1", "status": 5}]),
        // An entry is not an object at all.
        json!(["TC-1"]),
        // One bad entry fails the whole body, even after a good one.
        json!([
            {"testCaseId": "TC-1", "status": "Passed"},
            {"testCaseId": "TC-2"},
        ]),
    ] {
        let (status, error) = send_json(
            &app,
            json_request("POST", "/test_runs/nightly.json/import/json", &body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "for {body}: {error}");
        assert_error_envelope(&error, "invalid_request");
    }

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert!(stored["results"].is_null(), "nothing was written: {stored}");
}

#[tokio::test]
async fn a_json_import_rejects_an_unknown_field() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/import/json",
            &json!([{"testCaseId": "TC-1", "status": "Passed", "result": "Passed"}]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert!(stored["results"].is_null(), "nothing was written: {stored}");
}

#[tokio::test]
async fn a_json_import_rejects_a_status_outside_the_three() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    for status in ["Untested", "Retest", "passed", "Unknown", ""] {
        let (code, body) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/nightly.json/import/json",
                &json!([{"testCaseId": "TC-1", "status": status}]),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "for status {status}: {body}");
        assert_error_envelope(&body, "invalid_status");
    }

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert!(stored["results"].is_null(), "nothing was written: {stored}");
}

#[tokio::test]
async fn a_json_import_rejects_a_body_it_cannot_read() {
    let (_directory, app) = test_app();
    create_run(&app, "nightly").await;

    for body in [
        r#"[{"testCaseId": "TC-1", "status": "Passed"}"#,
        r#"{"cases": []}"#,
        r#"{"results": [], "extra": 1}"#,
        r#"{"results": "nope"}"#,
        r#"{"results": {}}"#,
        r#""results""#,
        "null",
        "42",
    ] {
        let (status, error) = send_json(
            &app,
            raw_json_request(
                "POST",
                "/test_runs/nightly.json/import/json",
                body.to_owned(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "for {body}: {error}");
        assert_error_envelope(&error, "invalid_request");
    }

    let (_, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert!(stored["results"].is_null(), "nothing was written: {stored}");
}

#[tokio::test]
async fn importing_json_into_an_unknown_run_is_not_found() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/missing.json/import/json",
            &json!([{"testCaseId": "TC-1", "status": "Passed"}]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn a_result_without_links_lists_no_defects() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-1", "status": "Failed", "timestamp": "1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // An empty list is the answer for a result that exists and links nothing,
    // which is a different answer from "no such result" (asserted below).
    let (status, body) = send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
    assert_eq!(status, StatusCode::OK, "listing: {body}");
    assert_eq!(body, json!({"defects": []}));
}

#[tokio::test]
async fn listing_defects_returns_the_links_a_result_carries() {
    let (directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-1", "status": "Failed", "timestamp": "1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The links belong to the result, not the run, so they are written where the
    // route that will create them stores them — and read back through the same
    // document the API serves. A run lives in its project's folder.
    let home = fixture_home(&app).await;
    let path = directory.path().join(format!(
        "projects/{}/test_runs/nightly.json",
        common::project_folder(&home)
    ));
    let mut run: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("run readable"))
            .expect("run is valid JSON");
    run["results"][0]["defectLinks"] = json!([
        {
            "linkId": "L-1",
            "defectId": "BUG-42",
            "defectUrl": "https://tracker.example/BUG-42",
            "trackerType": "jira",
            "title": "Card is declined twice",
            "status": "Open",
            "linkedAt": "1",
        }
    ]);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&run).expect("run serialises"),
    )
    .expect("run written");

    let (status, body) = send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
    assert_eq!(status, StatusCode::OK, "listing: {body}");
    assert_eq!(
        body,
        json!({
            "defects": [{
                "linkId": "L-1",
                "defectId": "BUG-42",
                "defectUrl": "https://tracker.example/BUG-42",
                "trackerType": "jira",
                "title": "Card is declined twice",
                "status": "Open",
                "linkedAt": "1",
            }]
        })
    );
}

#[tokio::test]
async fn listing_defects_needs_a_run_and_a_result_to_read() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-1", "status": "Passed", "timestamp": "1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // An identifier that is not a single `.json` component is a 400 before the
    // storage is consulted, an unknown run is a 404, and so is a case the run
    // never recorded a result for.
    for (route, expected) in [
        (
            "/test_runs/not-a-document/results/TC-1/defects",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/test_runs/missing.json/results/TC-1/defects",
            StatusCode::NOT_FOUND,
        ),
        (
            "/test_runs/nightly.json/results/TC-2/defects",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (status, body) = send_json(&app, get(route)).await;
        assert_eq!(status, expected, "GET {route}: {body}");
        if expected == StatusCode::BAD_REQUEST {
            assert_error_envelope(&body, "invalid_id");
        } else {
            assert_error_envelope(&body, "not_found");
        }
    }
}

/// Records a failed result for `case_id` so a defect link has a home.
async fn record_failure(app: &Router, case_id: &str) {
    let (status, body) = send_json(
        app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": case_id, "status": "Failed", "timestamp": "1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording {case_id}: {body}");
}

#[tokio::test]
async fn a_result_links_a_defect_of_every_tracker_type() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;
    record_failure(&app, "TC-1").await;

    // One link per tracker the API knows. The URL shape each tracker accepts is
    // the shape the tracker itself uses, so what a client can paste into a
    // browser is what the API stores.
    let defects = [
        (
            "BUG-1",
            "https://acme.atlassian.net/browse/BUG-1",
            "jira",
            None,
        ),
        ("7", "https://github.com/acme/app/issues/7", "github", None),
        (
            "BUG-2",
            "https://gitlab.com/acme/app/-/issues/9",
            "gitlab",
            None,
        ),
        (
            "BUG-3",
            "https://tracker.example/BUG-3",
            "custom",
            Some(("Flickering checkout", "Open")),
        ),
    ];

    for (defect_id, defect_url, tracker_type, extra) in defects {
        let mut body = json!({
            "defectId": defect_id,
            "defectUrl": defect_url,
            "trackerType": tracker_type,
        });
        if let Some((title, status)) = extra {
            body["title"] = json!(title);
            body["status"] = json!(status);
        }

        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/nightly.json/results/TC-1/defects",
                &body,
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "linking {defect_id}: {created}"
        );
        assert_eq!(created["message"], "Defect linked to test result");
        let link_id = created["id"].as_str().expect("the API derives the link id");
        assert!(
            link_id.starts_with("link-"),
            "the derived id is the API's own: {link_id}"
        );

        // The identifier the API returned is the one the read route publishes,
        // so a client never has to guess how to address the link it just made.
        let (status, listed) =
            send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
        assert_eq!(status, StatusCode::OK, "listing: {listed}");
        let listed = listed["defects"].as_array().expect("defects array");
        let newest = listed.last().expect("the link just created");
        assert_eq!(newest["linkId"], link_id);
        assert_eq!(newest["defectId"], defect_id);
        assert_eq!(newest["defectUrl"], defect_url);
        assert_eq!(newest["trackerType"], tracker_type);
        assert!(
            newest["linkedAt"].is_string(),
            "the API dates the link: {newest}"
        );
        match extra {
            Some((title, status)) => {
                assert_eq!(newest["title"], title);
                assert_eq!(newest["status"], status);
            }
            None => {
                assert!(
                    newest.get("title").is_none() && newest.get("status").is_none(),
                    "an omitted optional field is absent, not null: {newest}"
                );
            }
        }
    }

    let (_, listed) = send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
    assert_eq!(
        listed["defects"].as_array().expect("defects array").len(),
        4
    );
}

#[tokio::test]
async fn a_defect_link_rejects_a_body_or_tracker_the_api_cannot_use() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;
    record_failure(&app, "TC-1").await;

    // Every field the client must supply, an unknown field, and the two
    // identifiers the API derives rather than reads. A body naming `linkId` or
    // `linkedAt` is rejected instead of having it silently thrown away.
    let bad_bodies = [
        json!({"defectUrl": "https://tracker.example/BUG", "trackerType": "custom"}),
        json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG"}),
        json!({"defectId": "BUG-1", "trackerType": "custom"}),
        json!({"defectId": "", "defectUrl": "https://tracker.example/BUG", "trackerType": "custom"}),
        json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG", "trackerType": "custom", "sneaky": true}),
        json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG", "trackerType": "custom", "linkId": "L-1"}),
        json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG", "trackerType": "custom", "linkedAt": "1"}),
        json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG", "trackerType": "trello"}),
        json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG", "trackerType": 7}),
        // A URL the named tracker would not use, and one that is not HTTPS.
        json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG-1", "trackerType": "jira"}),
        json!({"defectId": "BUG-1", "defectUrl": "http://tracker.example/BUG-1", "trackerType": "custom"}),
        json!({"defectId": "BUG-1", "defectUrl": "https://github.com/acme/app/pull/7", "trackerType": "github"}),
    ];
    for body in bad_bodies {
        let (status, response) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/nightly.json/results/TC-1/defects",
                &body,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body {body}: {response}");
        assert_error_envelope(&response, "invalid_request");
    }

    // A rejected body writes nothing, so the result still links no defect.
    let (_, listed) = send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
    assert_eq!(listed, json!({"defects": []}));

    let good = json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG-1", "trackerType": "custom"});

    // An unusable run identifier is a 400 before the storage is consulted; an
    // unknown run and a case the run never recorded are both 404.
    for (route, expected) in [
        (
            "/test_runs/not-a-document/results/TC-1/defects",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/test_runs/missing.json/results/TC-1/defects",
            StatusCode::NOT_FOUND,
        ),
        (
            "/test_runs/nightly.json/results/TC-2/defects",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (status, response) = send_json(&app, json_request("POST", route, &good)).await;
        assert_eq!(status, expected, "POST {route}: {response}");
        if expected == StatusCode::BAD_REQUEST {
            assert_error_envelope(&response, "invalid_id");
        } else {
            assert_error_envelope(&response, "not_found");
        }
    }
}

#[tokio::test]
async fn the_same_defect_cannot_be_linked_to_one_result_twice() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1", "TC-2"]).await;
    record_failure(&app, "TC-1").await;

    let body = json!({"defectId": "BUG-1", "defectUrl": "https://tracker.example/BUG-1", "trackerType": "custom"});
    let (status, first) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results/TC-1/defects",
            &body,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "first link: {first}");

    // The duplicate is a conflict, and the link that landed first survives.
    let (status, response) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results/TC-1/defects",
            &body,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&response, "conflict");

    let (_, listed) = send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
    let listed = listed["defects"].as_array().expect("defects array");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["linkId"], first["id"]);

    // A different defect on the same result is not a duplicate.
    let (status, second) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results/TC-1/defects",
            &json!({"defectId": "BUG-2", "defectUrl": "https://tracker.example/BUG-2", "trackerType": "custom"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "second defect: {second}");

    // The identity is per result: the same defect may be linked to another case.
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-2", "status": "Failed", "timestamp": "1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, other) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results/TC-2/defects",
            &body,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "same defect, other case: {other}"
    );
}

#[tokio::test]
async fn a_defect_link_can_be_removed_and_is_then_gone() {
    let (_directory, app) = test_app();
    create_run_holding(&app, "nightly", &["TC-1"]).await;
    record_failure(&app, "TC-1").await;

    let mut ids = Vec::new();
    for defect_id in ["BUG-1", "BUG-2"] {
        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs/nightly.json/results/TC-1/defects",
                &json!({
                    "defectId": defect_id,
                    "defectUrl": format!("https://tracker.example/{defect_id}"),
                    "trackerType": "custom",
                }),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "linking {defect_id}: {created}"
        );
        ids.push(created["id"].as_str().expect("link id").to_owned());
    }

    // Removing one link leaves the other in place.
    let (status, removed) = send_json(
        &app,
        delete(&format!(
            "/test_runs/nightly.json/results/TC-1/defects/{}",
            ids[0]
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unlinking: {removed}");

    let (_, listed) = send_json(&app, get("/test_runs/nightly.json/results/TC-1/defects")).await;
    let listed = listed["defects"].as_array().expect("defects array");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["linkId"], ids[1]);

    // The same link cannot be removed twice, and an identifier the result never
    // carried is not found either.
    for link_id in [ids[0].as_str(), "link-0"] {
        let (status, response) = send_json(
            &app,
            delete(&format!(
                "/test_runs/nightly.json/results/TC-1/defects/{link_id}"
            )),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "unlinking {link_id}: {response}"
        );
        assert_error_envelope(&response, "not_found");
    }

    // The link identifier is opaque, so it is never validated as a document
    // name; the run and result are what the route validates.
    for (route, expected) in [
        (
            "/test_runs/not-a-document/results/TC-1/defects/link-0",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/test_runs/missing.json/results/TC-1/defects/link-0",
            StatusCode::NOT_FOUND,
        ),
        (
            "/test_runs/nightly.json/results/TC-2/defects/link-0",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (status, response) = send_json(&app, delete(route)).await;
        assert_eq!(status, expected, "DELETE {route}: {response}");
        if expected == StatusCode::BAD_REQUEST {
            assert_error_envelope(&response, "invalid_id");
        } else {
            assert_error_envelope(&response, "not_found");
        }
    }
}

#[tokio::test]
async fn a_run_pins_the_case_version_it_snapshotted() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );

    // Adding the case snapshots it at the version it carries; the API stamps 1
    // when it creates a case.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/test_cases",
            &json!({"testCaseId": "TC-001"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "adding the case: {body}");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(run["caseVersions"]["TC-001"], json!(1));
    assert_eq!(run["testCases"][0]["version"], json!(1));

    // A qualifying edit advances the live case to version 2...
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"title": "Login twice"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, live) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(live["version"], json!(2));

    // ...but the run is a snapshot: the version it pinned and the copy of the
    // case it embedded both stay at 1, so the run still describes the case as
    // it was when the run captured it.
    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(run["caseVersions"]["TC-001"], json!(1));
    assert_eq!(run["testCases"][0]["version"], json!(1));
    assert_eq!(run["testCases"][0]["title"], "Login");

    // The first capture wins, so recording a result for the now-version-2 case
    // leaves the pinned version alone too.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-001", "status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording the result: {body}");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(run["caseVersions"]["TC-001"], json!(1));
}

#[tokio::test]
async fn recording_a_result_pins_the_version_of_the_case_the_store_holds() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;
    // The run holds both cases it records results for: TC-001 lives in the
    // store, TC-404 only in the run's own snapshot — the shape a case whose
    // document was removed after the run captured it has.
    assert_eq!(
        create_run_holding(&app, "nightly", &["TC-001", "TC-404"]).await,
        "nightly.json"
    );

    // A run that holds a case the store does not carry records a result for it
    // and pins that case at version 1 rather than leaving it unversioned.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-404", "status": "Failed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording the result: {body}");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(run["caseVersions"]["TC-404"], json!(1));

    // A case the store does hold is pinned at the version that case carries.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"title": "Login twice"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, live) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(live["version"], json!(2));

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-001", "status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "recording the result: {body}");

    let (_, run) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(run["caseVersions"]["TC-001"], json!(2));
    assert_eq!(run["caseVersions"]["TC-404"], json!(1));
}

/// Deleting a run through a project resolves the project the route names, so an
/// identifier two projects hold is removed one home at a time and the other
/// home is left alone.
#[tokio::test]
async fn deleting_a_run_through_a_project_resolves_that_project() {
    let (_directory, app) = test_app();

    let alpha = create_project(&app, "alpha").await;
    let beta = create_project(&app, "beta").await;
    for project in [&alpha, &beta] {
        let (status, created) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{project}/test_runs"),
                &json!({"name": "nightly"}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "creating in {project}: {created}"
        );
        assert_eq!(created["id"], "nightly.json");
    }

    // While two projects hold the identifier, the global route refuses it: a
    // bare identifier cannot say which occurrence was meant.
    let (status, body) = send_json(&app, delete("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_error_envelope(&body, "conflict");

    let (status, body) = send_json(
        &app,
        delete(&format!("/projects/{alpha}/test_runs/nightly.json")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Test run deleted");

    let (status, owned) = send_json(&app, get(&format!("/projects/{alpha}/test_runs"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owned, json!([]), "the named project lost the occurrence");
    let (status, owned) = send_json(&app, get(&format!("/projects/{beta}/test_runs"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        owned,
        json!(["nightly.json"]),
        "the other home is untouched"
    );
    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK, "one home resolves again: {stored}");
    assert_eq!(stored["name"], "nightly");

    // The occurrence this project owned is gone, so a second delete has nothing
    // left to remove.
    let (status, body) = send_json(
        &app,
        delete(&format!("/projects/{alpha}/test_runs/nightly.json")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn an_update_refuses_a_body_identifier_that_names_another_run() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_runs/nightly.json",
            &json!({"testRunId": "other.json", "name": "renamed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "renaming a run: {body}");
    assert_error_envelope(&body, "invalid_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("immutable")),
        "unexpected message: {body}"
    );

    // The refusal lands before the write, so the document is untouched.
    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["testRunId"], "nightly.json");
    assert_eq!(stored["name"], "nightly");

    let (status, body) = send_json(&app, get("/test_runs/other.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nothing was renamed: {body}");
}

#[tokio::test]
async fn an_update_refuses_a_body_identifier_the_store_cannot_file() {
    let (_directory, app) = test_app();
    assert_eq!(
        create_named(&app, "/test_runs", "nightly").await,
        "nightly.json"
    );

    for supplied in ["probe-moved", "team/copy.json", "", ".", ".."] {
        let (status, body) = send_json(
            &app,
            json_request(
                "PUT",
                "/test_runs/nightly.json",
                &json!({"testRunId": supplied}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "supplying {supplied:?}: {body}"
        );
        assert_error_envelope(&body, "invalid_id");
    }

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["testRunId"], "nightly.json");
}
