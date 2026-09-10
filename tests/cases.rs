mod common;

use axum::http::StatusCode;
use common::{
    assert_error_envelope, case_body, create_case_in, create_project, create_suite, delete, get,
    json_request, send_json, test_app,
};
use serde_json::json;

#[tokio::test]
async fn test_cases_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;

    let (status, listing) = send_json(&app, get("/test_cases")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_cases"),
            &json!({"testCaseId": "TC-001", "title": "Login", "expectedResult": "Authenticated"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "TC-001");
    assert_eq!(created["message"], "Test case created");

    let (status, listing) = send_json(&app, get("/test_cases")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["TC-001"]));

    let (status, owned) = send_json(&app, get(&format!("/projects/{project}/test_cases"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owned, json!(["TC-001"]));

    let (status, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["title"], "Login");

    let (_, assembled) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(assembled["testCases"][0]["testCaseId"], "TC-001");

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"testCaseId": "TC-001", "title": "Login twice", "expectedResult": "Authenticated"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(updated["title"], "Login twice");

    let (status, _) = send_json(&app, delete("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, owned) = send_json(&app, get(&format!("/projects/{project}/test_cases"))).await;
    assert_eq!(owned, json!([]), "the project no longer owns the case");
}

#[tokio::test]
async fn creating_a_test_case_requires_a_real_parent() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request("POST", "/test_cases", &case_body("TC-001")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("/projects/{id}/test_cases"),
        "the explainer names the replacement routes: {body}"
    );

    for parent in ["/projects/missing.json", "/test_suites/missing.json"] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("{parent}/test_cases"),
                &case_body("TC-001"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "parent: {parent}");
        assert_error_envelope(&body, "not_found");
    }
}

#[tokio::test]
async fn creating_a_test_case_requires_identifier_title_and_expected_result() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let collection = format!("/projects/{project}/test_cases");

    // A body carrying a `title` asks for creation, so its missing creation
    // fields are reported as a bad request rather than read as a placement.
    for payload in [
        json!({"title": "No identifier", "expectedResult": "Stored"}),
        json!({"testCaseId": "TC-001", "title": "No expected result"}),
        json!({"testCaseId": "", "title": "Empty identifier", "expectedResult": "Stored"}),
    ] {
        let (status, body) = send_json(&app, json_request("POST", &collection, &payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "payload: {payload}");
        assert_error_envelope(&body, "invalid_request");
    }

    // Without a title the request reads as a placement, which needs one identifier field
    let (status, body) = send_json(
        &app,
        json_request("POST", &collection, &json!({"note": "neither"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
}

#[tokio::test]
async fn duplicate_test_cases_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_cases"),
            &case_body("TC-001"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn cases_created_inside_a_suite_live_in_the_suite() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;

    create_case_in(&app, &format!("/test_suites/{suite}/test_cases"), "TC-001").await;

    let (_, owned) = send_json(&app, get(&format!("/test_suites/{suite}/test_cases"))).await;
    assert_eq!(owned, json!(["TC-001"]));

    let (_, project_document) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(
        project_document["testSuites"][0]["testCases"][0]["testCaseId"], "TC-001",
        "the assembled project carries the suite's own cases"
    );
    assert!(
        project_document.get("testCases").is_none(),
        "the project owns no case directly: {project_document}"
    );

    let (status, _) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "reads stay global and still find a suite's case"
    );
}

#[tokio::test]
async fn an_identifier_in_several_parents_must_be_addressed_through_one() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;
    let composition = format!("/test_suites/{suite}/test_cases");
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    let (status, copied) = send_json(
        &app,
        json_request("POST", &composition, &json!({"testCaseId": "TC-001"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(copied["message"], "Test case copied");

    // The copy makes the bare identifier ambiguous, so the flat routes refuse it
    for request in [
        get("/test_cases/TC-001"),
        delete("/test_cases/TC-001"),
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"testCaseId": "TC-001", "title": "X", "expectedResult": "Y"}),
        ),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_error_envelope(&body, "conflict");
    }

    let (_, body) = send_json(&app, get("/test_cases/TC-001")).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("/projects/{id}/test_cases"),
        "the conflict names the parent-scoped routes: {body}"
    );

    let (_, listing) = send_json(&app, get("/test_cases")).await;
    assert_eq!(
        listing,
        json!(["TC-001"]),
        "a global scan de-duplicates rather than failing"
    );

    // Ambiguity also blocks a placement, which needs one source occurrence
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &composition,
            &json!({"testCaseId": "TC-001", "mode": "move"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn moving_a_case_between_parents_leaves_it_one_home() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;
    let direct = format!("/projects/{project}/test_cases");
    let composition = format!("/test_suites/{suite}/test_cases");
    create_case_in(&app, &direct, "TC-001").await;

    let (status, moved) = send_json(
        &app,
        json_request(
            "POST",
            &composition,
            &json!({"testCaseId": "TC-001", "mode": "move"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(moved["message"], "Test case moved");
    assert_eq!(moved["id"], "TC-001");

    let (_, owned) = send_json(&app, get(&direct)).await;
    assert_eq!(owned, json!([]), "the project no longer owns the case");

    let (_, owned) = send_json(&app, get(&composition)).await;
    assert_eq!(owned, json!(["TC-001"]));

    let (status, document) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a moved document stays globally readable"
    );
    assert_eq!(document["title"], "Login");

    // Moving it back is a plain placement again: one home, in the project
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &direct,
            &json!({"testCaseId": "TC-001", "mode": "move"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, owned) = send_json(&app, get(&direct)).await;
    assert_eq!(owned, json!(["TC-001"]));

    let (_, owned) = send_json(&app, get(&composition)).await;
    assert_eq!(owned, json!([]), "the suite no longer owns the case");
}

#[tokio::test]
async fn placement_grammar_rejects_mixed_fields_and_unknown_modes() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;
    let composition = format!("/test_suites/{suite}/test_cases");

    for payload in [
        json!({"testCaseId": "TC-001", "title": "Mixes creation and placement", "mode": "copy"}),
        json!({"testCaseId": "TC-001", "mode": "teleport"}),
        json!({"mode": "copy"}),
    ] {
        let (status, body) = send_json(&app, json_request("POST", &composition, &payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "payload: {payload}");
        assert_error_envelope(&body, "invalid_request");
    }

    // A well-formed placement of a case that does not exist reports the missing resource
    let (status, body) = send_json(
        &app,
        json_request("POST", &composition, &json!({"testCaseId": "absent.json"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn missing_test_cases_return_not_found() {
    let (_directory, app) = test_app();

    for request in [
        get("/test_cases/UNKNOWN"),
        delete("/test_cases/UNKNOWN"),
        json_request(
            "PUT",
            "/test_cases/UNKNOWN",
            &json!({"testCaseId": "UNKNOWN", "title": "X", "expectedResult": "Y"}),
        ),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error_envelope(&body, "not_found");
    }
}

#[tokio::test]
async fn creating_a_rich_test_case_preserves_preconditions_severity_and_steps() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;

    let payload = json!({
        "testCaseId": "TC-RICH-001.json",
        "title": "Rich Order Verification",
        "preconditions": "User is logged in",
        "priority": "High",
        "severity": "Critical",
        "testType": "Functional",
        "expectedResult": "Order confirmation modal shown",
        "steps": [
            "Open product page",
            {
                "action": "Click Checkout",
                "expectedResult": "Payment screen loaded"
            },
            {
                "action": "Capture the receipt",
                "attachments": [{
                    "filename": "1-receipt.png",
                    "originalName": "receipt.png",
                    "mimeType": "image/png",
                    "size": 4096
                }]
            }
        ]
    });

    let (status, created) = send_json(
        &app,
        json_request("POST", &format!("/projects/{project}/test_cases"), &payload),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "TC-RICH-001.json");

    let (status, stored) = send_json(&app, get("/test_cases/TC-RICH-001.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["preconditions"], "User is logged in");
    assert_eq!(stored["severity"], "Critical");
    assert_eq!(stored["testType"], "Functional");
    assert_eq!(stored["steps"][0], "Open product page");
    assert_eq!(stored["steps"][1]["action"], "Click Checkout");
    assert_eq!(
        stored["steps"][1]["expectedResult"],
        "Payment screen loaded"
    );

    // A structured step round-trips the attachments the body carried, and a
    // step that carried none omits the key rather than recording an empty list.
    assert!(stored["steps"][1].get("attachments").is_none());
    assert_eq!(
        stored["steps"][2]["attachments"],
        json!([{
            "filename": "1-receipt.png",
            "originalName": "receipt.png",
            "mimeType": "image/png",
            "size": 4096,
        }])
    );
}

#[tokio::test]
async fn a_partial_update_keeps_the_fields_the_body_leaves_out() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_cases"),
            &json!({
                "testCaseId": "TC-001.json",
                "title": "Login",
                "expectedResult": "Dashboard",
                "preconditions": "Account exists",
                "priority": "High",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(
        &app,
        json_request("PUT", "/test_cases/TC-001.json", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "updating: {body}");

    let (status, stored) = send_json(&app, get("/test_cases/TC-001.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["testCaseId"], "TC-001.json");
    assert_eq!(stored["title"], "Login");
    assert_eq!(stored["expectedResult"], "Dashboard");
    assert_eq!(stored["preconditions"], "Account exists");
    assert_eq!(stored["priority"], "High");

    // The case still satisfies its model, so the project assembles it.
    let (status, assembled) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(assembled["testCases"][0]["title"], "Login");
    assert_eq!(assembled["testCases"][0]["expectedResult"], "Dashboard");
}

/// Reads a stored JSON document from the volume for assertions on the layout.
fn read_json(path: &std::path::Path) -> serde_json::Value {
    let bytes = std::fs::read(path).expect("stored document");
    serde_json::from_slice(&bytes).expect("stored document is JSON")
}

/// Asserts the documented `lastModified` shape: an ISO-8601 UTC timestamp.
fn assert_iso8601(value: &serde_json::Value) -> &str {
    let stamp = value.as_str().unwrap_or_else(|| {
        panic!("lastModified is a string, got {value}");
    });
    assert_eq!(stamp.len(), 20, "ISO-8601 UTC stamp: {stamp}");
    assert!(stamp.ends_with('Z'), "ISO-8601 UTC stamp: {stamp}");
    assert_eq!(&stamp[4..5], "-", "ISO-8601 UTC stamp: {stamp}");
    assert_eq!(&stamp[10..11], "T", "ISO-8601 UTC stamp: {stamp}");
    stamp
}

#[tokio::test]
async fn a_new_test_case_is_stamped_with_its_first_version() {
    let (directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;

    // The server owns `version` and `lastModified`, so values a client supplies
    // are overwritten rather than stored.
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_cases"),
            &json!({
                "testCaseId": "TC-001",
                "title": "Login",
                "expectedResult": "Authenticated",
                "version": 7,
                "lastModified": "1999-12-31T23:59:59Z",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (status, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["version"], json!(1));
    let stamped = assert_iso8601(&stored["lastModified"]).to_owned();

    let marker = "projects/checkout/TC-001/test-case.json";
    let on_disk = read_json(&directory.path().join(marker));
    assert_eq!(on_disk["version"], json!(1), "the marker carries version 1");
    assert_eq!(on_disk["lastModified"], json!(stamped));
    assert!(
        !directory
            .path()
            .join("projects/checkout/TC-001/revisions")
            .exists(),
        "a fresh case has no history yet"
    );

    // A case created inside a suite is stamped the same way.
    let (status, nested) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/test_suites/{suite}/test_cases"),
            &case_body("TC-002"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{nested}");
    let nested_marker = read_json(
        &directory
            .path()
            .join("projects/checkout/smoke/TC-002/test-case.json"),
    );
    assert_eq!(nested_marker["version"], json!(1));
}

#[tokio::test]
async fn a_qualifying_update_snapshots_the_previous_version() {
    let (directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    let (_, created) = send_json(&app, get("/test_cases/TC-001")).await;
    let first_stamp = assert_iso8601(&created["lastModified"]).to_owned();
    let case = directory.path().join("projects/checkout/TC-001");

    // `title` is a qualifying field: the pre-update document is snapshotted as
    // version 1 and the live document advances to version 2.
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

    let (_, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(stored["version"], json!(2));
    assert_eq!(stored["title"], "Login twice");
    let second_stamp = assert_iso8601(&stored["lastModified"]).to_owned();

    let first_snapshot = read_json(&case.join("revisions/v1.json"));
    assert_eq!(first_snapshot["version"], json!(1));
    assert_eq!(first_snapshot["title"], "Login");
    assert_eq!(first_snapshot["expectedResult"], "Stored");
    assert_eq!(first_snapshot["lastModified"], json!(first_stamp));
    assert_eq!(
        read_json(&case.join("test-case.json"))["lastModified"],
        json!(second_stamp),
        "the live document carries the refreshed stamp"
    );

    // `expectedResult` qualifies too, and the next snapshot is the version 2
    // document rather than a re-serialisation of version 1.
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"expectedResult": "Authenticated"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(stored["version"], json!(3));

    let second_snapshot = read_json(&case.join("revisions/v2.json"));
    assert_eq!(second_snapshot["version"], json!(2));
    assert_eq!(second_snapshot["title"], "Login twice");
    assert_eq!(second_snapshot["expectedResult"], "Stored");
    assert_eq!(second_snapshot["lastModified"], json!(second_stamp));

    // Snapshots are immutable: the earliest one still records the original text.
    assert_eq!(read_json(&case.join("revisions/v1.json"))["title"], "Login");
}

#[tokio::test]
async fn a_non_qualifying_update_keeps_the_version_and_writes_no_snapshot() {
    let (directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    let (_, created) = send_json(&app, get("/test_cases/TC-001")).await;
    let created_stamp = assert_iso8601(&created["lastModified"]).to_owned();

    // Only metadata changes, and the body tries to shove the bookkeeping along:
    // none of it may move.
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({
                "description": "Rewritten",
                "priority": "High",
                "severity": "Critical",
                "testType": "Functional",
                "exploratory": true,
                "tags": ["smoke", "login"],
                "version": 99,
                "lastModified": "1999-12-31T23:59:59Z",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(stored["version"], json!(1), "the version is untouched");
    assert_eq!(
        stored["lastModified"],
        json!(created_stamp),
        "the stamp is untouched"
    );
    assert_eq!(stored["priority"], "High", "the metadata landed");
    assert_eq!(stored["tags"], json!(["smoke", "login"]));

    let case = directory.path().join("projects/checkout/TC-001");
    assert!(
        !case.join("revisions").exists(),
        "a non-qualifying update records no snapshot"
    );

    // A body carrying only the bookkeeping is not a qualifying change either.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"version": 42, "lastModified": "2020-01-01T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(stored["version"], json!(1));
    assert_eq!(stored["lastModified"], json!(created_stamp));
}

#[tokio::test]
async fn a_case_persisted_before_versioning_stays_readable_and_versions_on_demand() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let case = directory.path().join("projects/legacy/TC-001");
    std::fs::create_dir_all(&case).expect("case folder");
    std::fs::write(
        directory.path().join("projects/legacy/project.json"),
        br#"{"projectId":"legacy","name":"legacy","testSuites":[]}"#,
    )
    .expect("legacy project");
    let legacy = json!({
        "testCaseId": "TC-001",
        "title": "Legacy",
        "expectedResult": "Stored",
    });
    std::fs::write(
        case.join("test-case.json"),
        serde_json::to_vec(&legacy).expect("legacy document"),
    )
    .expect("legacy document");
    let app = common::app_at(directory.path());

    // A document persisted before the fields existed still deserialises, and is
    // served in the shape it was stored in — no new keys, no rewrite.
    let (status, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(stored["title"], "Legacy");
    assert!(
        stored.get("version").is_none() && stored.get("lastModified").is_none(),
        "a legacy document gains no bookkeeping by being read: {stored}"
    );

    // A non-qualifying update keeps that on-disk shape.
    let (status, _) = send_json(
        &app,
        json_request("PUT", "/test_cases/TC-001", &json!({"priority": "Low"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(stored["priority"], "Low");
    assert!(stored.get("version").is_none() && stored.get("lastModified").is_none());

    // The first qualifying update treats the stored document as version 1: the
    // legacy text is snapshotted verbatim and the live document becomes 2.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"title": "Legacy renamed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(stored["version"], json!(2));
    assert_eq!(stored["title"], "Legacy renamed");
    assert_iso8601(&stored["lastModified"]);
    assert_eq!(
        read_json(&case.join("revisions/v1.json")),
        json!({
            "testCaseId": "TC-001",
            "title": "Legacy",
            "expectedResult": "Stored",
            "priority": "Low",
        }),
        "the snapshot is the pre-update document, verbatim"
    );
}

#[tokio::test]
async fn a_copied_case_carries_the_revision_snapshots_of_its_source() {
    let (directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;
    create_case_in(&app, &format!("/test_suites/{suite}/test_cases"), "TC-001").await;

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

    let source = directory.path().join("projects/checkout/smoke/TC-001");
    let snapshot = read_json(&source.join("revisions/v1.json"));
    assert_eq!(snapshot["title"], "Login");

    // A folder copy duplicates the case's history with it, so the copy's next
    // qualifying update continues from the version its live document carries.
    let (status, copied) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_cases"),
            &json!({"testCaseId": "TC-001"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{copied}");

    let copy = directory.path().join("projects/checkout/TC-001");
    assert_eq!(read_json(&copy.join("revisions/v1.json")), snapshot);
    assert_eq!(read_json(&copy.join("test-case.json"))["version"], json!(2));

    // Two homes for one identifier: the copy is addressed through the routes
    // that name its parent, and the flat routes refuse the ambiguity.
    let (status, body) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_error_envelope(&body, "conflict");

    let (_, owned) = send_json(&app, get(&format!("/projects/{project}/test_cases"))).await;
    assert_eq!(owned, json!(["TC-001"]));
}

#[tokio::test]
async fn a_case_without_qualifying_updates_has_an_empty_history() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    // A non-qualifying update leaves the version alone and records nothing.
    let (status, _) = send_json(
        &app,
        json_request("PUT", "/test_cases/TC-001", &json!({"priority": "High"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, history) = send_json(&app, get("/test_cases/TC-001/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history, json!([]), "a case with no snapshots lists nothing");
}

#[tokio::test]
async fn case_history_lists_its_snapshots_oldest_first_with_the_fields_they_changed() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    let (_, created) = send_json(&app, get("/test_cases/TC-001")).await;
    let first_stamp = assert_iso8601(&created["lastModified"]).to_owned();

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
    let (_, updated) = send_json(&app, get("/test_cases/TC-001")).await;
    let second_stamp = assert_iso8601(&updated["lastModified"]).to_owned();

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"preconditions": "Account exists"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Two qualifying updates recorded versions 1 and 2; the live document is
    // version 3. Each entry names the qualifying fields the *next* version
    // changed, so the newest snapshot is measured against the live document.
    let (status, history) = send_json(&app, get("/test_cases/TC-001/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(
        history,
        json!([
            {"version": 1, "lastModified": first_stamp, "changedFields": ["title"]},
            {"version": 2, "lastModified": second_stamp, "changedFields": ["preconditions"]},
        ])
    );

    // The live version is not a snapshot, so it never appears in the listing.
    let live = history.as_array().expect("history array");
    assert!(
        live.iter().all(|entry| entry["version"] != json!(3)),
        "the live version is not a snapshot: {history}"
    );
}

#[tokio::test]
async fn a_recorded_revision_is_returned_verbatim_and_the_live_version_is_not_a_snapshot() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    let (_, created) = send_json(&app, get("/test_cases/TC-001")).await;
    let first_stamp = assert_iso8601(&created["lastModified"]).to_owned();

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

    let (status, snapshot) = send_json(&app, get("/test_cases/TC-001/history/1")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(
        snapshot,
        json!({
            "testCaseId": "TC-001",
            "title": "Login",
            "expectedResult": "Stored",
            "version": 1,
            "lastModified": first_stamp,
        }),
        "the pre-update document, verbatim"
    );

    // The live document is read through the case route; the revision route
    // refuses the current version because it has no snapshot.
    let (_, live) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(live["version"], json!(2));
    let (status, body) = send_json(&app, get("/test_cases/TC-001/history/2")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_error_envelope(&body, "not_found");

    // A version the case never recorded is a 404 as well.
    let (status, body) = send_json(&app, get("/test_cases/TC-001/history/99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn history_resolves_the_case_before_it_validates_the_version() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;

    // The listing route answers a case that is not there with a 404.
    let (status, body) = send_json(&app, get("/test_cases/TC-404/history")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_error_envelope(&body, "not_found");

    // The revision route resolves the case first, so an unknown case is a 404
    // even when the version could never be parsed.
    for version in ["abc", "0", "99"] {
        let (status, body) =
            send_json(&app, get(&format!("/test_cases/TC-404/history/{version}"))).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "unknown case with version `{version}`: {body}"
        );
        assert_error_envelope(&body, "not_found");
    }

    // A known case reports an unusable version as `invalid_request`.
    for version in ["abc", "0", "-1", "1.5"] {
        let (status, body) =
            send_json(&app, get(&format!("/test_cases/TC-001/history/{version}"))).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "unusable version `{version}`: {body}"
        );
        assert_error_envelope(&body, "invalid_request");
    }
}

#[tokio::test]
async fn history_is_available_for_a_case_held_inside_a_suite() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;
    create_case_in(&app, &format!("/test_suites/{suite}/test_cases"), "TC-001").await;

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"expectedResult": "Authenticated"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, history) = send_json(&app, get("/test_cases/TC-001/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history.as_array().expect("history").len(), 1, "{history}");
    assert_eq!(history[0]["version"], json!(1));
    assert_eq!(history[0]["changedFields"], json!(["expectedResult"]));

    let (status, snapshot) = send_json(&app, get("/test_cases/TC-001/history/1")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["expectedResult"], "Stored");
}

#[tokio::test]
async fn a_snapshot_written_before_versioning_lists_without_a_timestamp() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let case = directory.path().join("projects/legacy/TC-001");
    std::fs::create_dir_all(&case).expect("case folder");
    std::fs::write(
        directory.path().join("projects/legacy/project.json"),
        br#"{"projectId":"legacy","name":"legacy","testSuites":[]}"#,
    )
    .expect("legacy project");
    let legacy = json!({
        "testCaseId": "TC-001",
        "title": "Legacy",
        "expectedResult": "Stored",
    });
    std::fs::write(
        case.join("test-case.json"),
        serde_json::to_vec(&legacy).expect("legacy document"),
    )
    .expect("legacy document");
    let app = common::app_at(directory.path());

    // Before any qualifying update the legacy case has no history at all.
    let (status, history) = send_json(&app, get("/test_cases/TC-001/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history, json!([]));

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-001",
            &json!({"title": "Legacy renamed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The snapshot is the stored document, which carries no stamp, so the entry
    // omits `lastModified` rather than inventing one.
    let (status, history) = send_json(&app, get("/test_cases/TC-001/history")).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(
        history,
        json!([{"version": 1, "changedFields": ["title"]}]),
        "the timestamp is absent, not null"
    );
    assert!(history[0].get("lastModified").is_none());

    // The snapshot itself is served as it was stored.
    let (status, snapshot) = send_json(&app, get("/test_cases/TC-001/history/1")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot, legacy);
}
