mod common;

use axum::http::StatusCode;
use common::{
    assert_error_envelope, create_case_in, create_project, create_suite, delete, get, json_request,
    send_json, test_app,
};
use serde_json::json;

#[tokio::test]
async fn test_suites_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;

    let (status, listing) = send_json(&app, get("/test_suites")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]), "a new tree holds no suites");

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_suites"),
            &json!({"suiteId": "S-001", "name": "regression", "testCases": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "regression.json");
    assert_eq!(created["message"], "Test suite created");

    let (status, listing) = send_json(&app, get("/test_suites")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["regression.json"]));

    let (status, owned) = send_json(&app, get(&format!("/projects/{project}/test_suites"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owned, json!(["regression.json"]));

    let (status, stored) = send_json(&app, get("/test_suites/regression.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["suiteId"], "S-001");
    assert_eq!(stored["testCases"], json!([]), "a suite starts empty");

    let (_, assembled) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(
        assembled["testSuites"][0]["suiteId"], "S-001",
        "reading a project assembles the suites it owns"
    );

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_suites/regression.json",
            &json!({"suiteId": "S-002", "name": "regression", "testCases": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/test_suites/regression.json")).await;
    assert_eq!(updated["suiteId"], "S-002");

    let (status, _) = send_json(&app, delete("/test_suites/regression.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/test_suites/regression.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, owned) = send_json(&app, get(&format!("/projects/{project}/test_suites"))).await;
    assert_eq!(owned, json!([]), "the project no longer owns the suite");
}

#[tokio::test]
async fn creating_a_test_suite_requires_a_parent_and_a_name() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;

    for payload in [json!({"note": "neither"}), json!({"name": ""})] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{project}/test_suites"),
                &payload,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "payload: {payload}");
        assert_error_envelope(&body, "invalid_request");
    }

    // A well-formed placement of a suite that does not exist reports the missing resource
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_suites"),
            &json!({"suiteId": "absent.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/missing.json/test_suites",
            &json!({"name": "smoke"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn the_flat_creation_route_only_explains_the_replacement() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request("POST", "/test_suites", &json!({"name": "regression"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("/projects/{id}/test_suites"),
        "the explainer names the replacement route: {body}"
    );

    let (_, listing) = send_json(&app, get("/test_suites")).await;
    assert_eq!(listing, json!([]), "nothing was stored");
}

#[tokio::test]
async fn duplicate_test_suites_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    assert_eq!(
        create_suite(&app, &project, "regression").await,
        "regression.json"
    );

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_suites"),
            &json!({"name": "regression"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn an_identifier_in_several_projects_must_be_addressed_through_one() {
    let (_directory, app) = test_app();
    let checkout = create_project(&app, "checkout").await;
    let billing = create_project(&app, "billing").await;

    assert_eq!(create_suite(&app, &checkout, "smoke").await, "smoke.json");
    assert_eq!(create_suite(&app, &billing, "smoke").await, "smoke.json");

    let (status, body) = send_json(&app, get("/test_suites/smoke.json")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("/projects/{id}/test_suites"),
        "the conflict names the parent-scoped routes: {body}"
    );

    let (_, listing) = send_json(&app, get("/test_suites")).await;
    assert_eq!(
        listing,
        json!(["smoke.json"]),
        "a global scan de-duplicates rather than failing"
    );

    let (_, owned) = send_json(&app, get(&format!("/projects/{billing}/test_suites"))).await;
    assert_eq!(owned, json!(["smoke.json"]));
}

#[tokio::test]
async fn missing_test_suites_return_not_found() {
    let (_directory, app) = test_app();

    for request in [
        get("/test_suites/missing.json"),
        delete("/test_suites/missing.json"),
        json_request(
            "PUT",
            "/test_suites/missing.json",
            &json!({"name": "missing"}),
        ),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error_envelope(&body, "not_found");
    }
}

#[tokio::test]
async fn deleting_a_suite_through_its_project_removes_it() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;

    let uri = format!("/projects/{project}/test_suites/{suite}");
    let (status, body) = send_json(&app, delete(&uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Test suite deleted");

    let (status, _) = send_json(&app, get(&format!("/test_suites/{suite}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send_json(&app, delete(&uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn test_suites_support_incremental_case_composition() {
    let (_directory, app) = test_app();
    let project = create_project(&app, "checkout").await;
    let suite = create_suite(&app, &project, "smoke").await;
    let cases = format!("/projects/{project}/test_cases");
    let composition = format!("/test_suites/{suite}/test_cases");

    create_case_in(&app, &cases, "TC-001.json").await;
    create_case_in(&app, &cases, "TC-002.json").await;

    // 1. An unknown case cannot join the suite
    let (status, body) = send_json(
        &app,
        json_request("POST", &composition, &json!({"testCaseId": "TC-999.json"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");

    // 2. The default mode copies, so the case keeps its home in the project
    let (status, copied) = send_json(
        &app,
        json_request("POST", &composition, &json!({"testCaseId": "TC-001.json"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(copied["message"], "Test case copied");
    assert_eq!(copied["id"], "TC-001.json");

    // 3. A second copy of the same identifier is rejected
    let (status, body) = send_json(
        &app,
        json_request("POST", &composition, &json!({"testCaseId": "TC-001.json"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");

    // 4. The next case joins and the suite lists both in folder order
    let (status, _) = send_json(
        &app,
        json_request("POST", &composition, &json!({"testCaseId": "TC-002.json"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, owned) = send_json(&app, get(&format!("/test_suites/{suite}/test_cases"))).await;
    assert_eq!(owned, json!(["TC-001.json", "TC-002.json"]));

    let (_, suite_document) = send_json(&app, get(&format!("/test_suites/{suite}"))).await;
    let members = suite_document["testCases"]
        .as_array()
        .expect("assembled cases");
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["testCaseId"], "TC-001.json");
    assert_eq!(members[1]["testCaseId"], "TC-002.json");

    let (_, project_document) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(
        project_document["testCases"]
            .as_array()
            .expect("direct cases")
            .len(),
        2,
        "copying a case into a suite leaves the source in place"
    );

    // 5. Removing the suite's occurrence leaves the project's copy alone
    let (status, _) = send_json(&app, delete(&format!("{composition}/TC-001.json"))).await;
    assert_eq!(status, StatusCode::OK);

    let (_, owned) = send_json(&app, get(&format!("/test_suites/{suite}/test_cases"))).await;
    assert_eq!(owned, json!(["TC-002.json"]));

    let (status, _) = send_json(&app, get("/test_cases/TC-001.json")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the project still owns its own TC-001.json"
    );

    // 6. Removing it again is a not-found
    let (status, body) = send_json(&app, delete(&format!("{composition}/TC-001.json"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn a_suite_can_be_placed_into_another_project() {
    let (_directory, app) = test_app();
    let checkout = create_project(&app, "checkout").await;
    let billing = create_project(&app, "billing").await;
    let suite = create_suite(&app, &checkout, "smoke").await;
    create_case_in(
        &app,
        &format!("/test_suites/{suite}/test_cases"),
        "TC-001.json",
    )
    .await;

    let (status, moved) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{billing}/test_suites"),
            &json!({"suiteId": "smoke.json", "mode": "move"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(moved["message"], "Test suite moved");

    let (_, source) = send_json(&app, get(&format!("/projects/{checkout}/test_suites"))).await;
    assert_eq!(source, json!([]), "the old project lost the suite");

    let (_, target) = send_json(&app, get(&format!("/projects/{billing}/test_suites"))).await;
    assert_eq!(target, json!(["smoke.json"]));

    let (_, suite_document) = send_json(&app, get("/test_suites/smoke.json")).await;
    assert_eq!(
        suite_document["testCases"][0]["testCaseId"], "TC-001.json",
        "a move carries the cases the suite owns"
    );

    // The default mode copies, so the identifier may live in both projects
    let (status, copied) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{checkout}/test_suites"),
            &json!({"suiteId": "smoke.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(copied["message"], "Test suite copied");

    let (_, source) = send_json(&app, get(&format!("/projects/{checkout}/test_suites"))).await;
    assert_eq!(source, json!(["smoke.json"]));

    let (_, target) = send_json(&app, get(&format!("/projects/{billing}/test_suites"))).await;
    assert_eq!(
        target,
        json!(["smoke.json"]),
        "copying never moves the source"
    );

    let (status, _) = send_json(&app, get("/test_suites/smoke.json")).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "two projects now hold a suite named smoke"
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
            &format!("/projects/{project}/test_suites"),
            &json!({
                "suiteId": "S-001",
                "name": "regression",
                "description": "nightly",
                "tags": ["smoke"],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(
        &app,
        json_request("PUT", "/test_suites/regression.json", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "updating: {body}");

    let (status, stored) = send_json(&app, get("/test_suites/regression.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["suiteId"], "S-001");
    assert_eq!(stored["name"], "regression");
    assert_eq!(stored["description"], "nightly");
    assert_eq!(stored["tags"], json!(["smoke"]));
    assert_eq!(
        stored["testCases"],
        json!([]),
        "membership stays in the folders"
    );

    // The project still owns the suite, and reading it assembles the same fields.
    let (status, assembled) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(assembled["testSuites"][0]["suiteId"], "S-001");
    assert_eq!(assembled["testSuites"][0]["description"], "nightly");
}
