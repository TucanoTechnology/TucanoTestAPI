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
