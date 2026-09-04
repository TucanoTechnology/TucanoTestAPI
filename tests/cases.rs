mod common;

use axum::http::StatusCode;
use common::{
    assert_error_envelope, create_test_case, delete, get, json_request, send_json, test_app,
};
use serde_json::json;

#[tokio::test]
async fn test_cases_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/test_cases")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/test_cases",
            &json!({"testCaseId": "TC-001", "title": "Login", "expectedResult": "Authenticated"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "TC-001");

    let (status, listing) = send_json(&app, get("/test_cases")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["TC-001"]));

    let (status, stored) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["title"], "Login");

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
}

#[tokio::test]
async fn creating_a_test_case_requires_identifier_title_and_expected_result() {
    let (_directory, app) = test_app();

    for payload in [
        json!({"title": "No identifier", "expectedResult": "Stored"}),
        json!({"testCaseId": "TC-001", "expectedResult": "No title"}),
        json!({"testCaseId": "TC-001", "title": "No expected result"}),
        json!({"testCaseId": "", "title": "Empty identifier", "expectedResult": "Stored"}),
    ] {
        let (status, body) = send_json(&app, json_request("POST", "/test_cases", &payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "payload: {payload}");
        assert_error_envelope(&body, "invalid_request");
    }
}

#[tokio::test]
async fn duplicate_test_cases_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/test_cases",
            &json!({"testCaseId": "TC-001", "title": "Again", "expectedResult": "Stored"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
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

    let (status, created) = send_json(&app, json_request("POST", "/test_cases", &payload)).await;
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
