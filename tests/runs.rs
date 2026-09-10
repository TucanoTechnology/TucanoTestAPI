mod common;

use axum::Router;
use axum::http::StatusCode;
use common::{
    app_at, assert_error_envelope, create_case_in, create_named, create_project, create_suite,
    delete, get, json_request, raw_json_request, send_json, test_app, xml_request,
};
use serde_json::json;
use tempfile::TempDir;

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
async fn a_run_created_from_a_name_alone_reads_back_and_records_results() {
    let (directory, app) = test_app();

    // The body names the run and nothing else: no `testRunId`, no `timestamp`.
    let (status, created) = send_json(
        &app,
        json_request("POST", "/test_runs", &json!({"name": "nightly"})),
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
    let marker = directory.path().join("test_runs/nightly.json");
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
            "/test_runs",
            &json!({"name": "morning", "timestamp": "2026-09-02T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, morning) = send_json(&app, get("/test_runs/morning.json")).await;
    assert_eq!(morning["testRunId"], "morning.json");
    assert_eq!(morning["timestamp"], "2026-09-02T00:00:00Z");

    // Every route that deserialises the run answers instead of failing.
    let project = create_project(&app, "checkout").await;
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

#[tokio::test]
async fn a_partial_update_keeps_the_fields_the_body_leaves_out() {
    let (_directory, app) = test_app();

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs",
            &json!({
                "testRunId": "R-001",
                "name": "nightly",
                "timestamp": "2026-09-02T00:00:00Z",
                "tags": ["ci"],
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

    // The run still reads back as its model, so recording a result works.
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/nightly.json/results",
            &json!({"testCaseId": "TC-001.json", "status": "Passed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, stored) = send_json(&app, get("/test_runs/nightly.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["results"][0]["status"], "Passed");
    assert_eq!(stored["name"], "nightly");
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

    for (id, name) in [("R-1", "nightly"), ("R-2", "weekly"), ("R-3", "release")] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                "/test_runs",
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
    let (status, created) = send_json(
        app,
        json_request("POST", "/test_runs", &json!({"name": name})),
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
    create_run(&app, "nightly").await;

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
    create_run(&app, "nightly").await;

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
