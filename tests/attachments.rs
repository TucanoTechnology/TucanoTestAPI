mod common;

use axum::http::StatusCode;
use common::{
    assert_error_envelope, content_type, create_test_case, delete, get, json_request,
    multipart_request, multipart_without_file, send, send_full, send_json, test_app,
};
use serde_json::json;

#[tokio::test]
async fn attachments_support_upload_download_and_delete() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (status, uploaded) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "notes.txt", b"evidence"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(uploaded["originalName"], "notes.txt");
    assert_eq!(uploaded["size"], 8);

    let filename = uploaded["filename"].as_str().expect("stored filename");
    assert!(filename.ends_with("-notes.txt"));

    let uri = format!("/test_cases/TC-001/attachments/{filename}");
    let (status, contents) = send(&app, get(&uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(contents, b"evidence");

    let (status, _) = send_json(&app, delete(&uri)).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&app, get(&uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn uploading_requires_an_existing_test_case() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        multipart_request("/test_cases/UNKNOWN/attachments", "notes.txt", b"evidence"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn uploading_requires_a_file_part() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (status, body) = send_json(
        &app,
        multipart_without_file("/test_cases/TC-001/attachments"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "missing_file");
}

#[tokio::test]
async fn missing_attachments_return_not_found() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let uri = "/test_cases/TC-001/attachments/missing.txt";

    let (status, _) = send(&app, get(uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send_json(&app, delete(uri)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn attachment_downloads_use_a_content_type_derived_from_the_extension() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (_, uploaded) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "report.pdf", b"%PDF-1.4"),
    )
    .await;
    let filename = uploaded["filename"].as_str().expect("stored filename");

    let (status, headers, _) = send_full(
        &app,
        get(&format!("/test_cases/TC-001/attachments/{filename}")),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("application/pdf"));
}

#[tokio::test]
async fn attachments_are_removed_with_their_test_case() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (_, uploaded) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "notes.txt", b"evidence"),
    )
    .await;
    let filename = uploaded["filename"].as_str().expect("stored filename");

    let (status, _) = send_json(&app, delete("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app,
        get(&format!("/test_cases/TC-001/attachments/{filename}")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn attachments_cannot_address_an_ambiguous_case() {
    let (_directory, app) = test_app();

    // One id in two parents: the project's own case and a copy inside a suite.
    let project = common::create_project(&app, "checkout").await;
    let suite = common::create_suite(&app, &project, "smoke").await;
    common::create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/test_suites/{suite}/test_cases"),
            &json!({"testCaseId": "TC-001"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The case is ambiguous now, so the attachment routes must not guess a home.
    let (status, body) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "notes.txt", b"evidence"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("/projects/{id}/test_cases"),
        "the conflict must name the parent-scoped routes: {body}"
    );

    let (status, _) = send(&app, get("/test_cases/TC-001/attachments/missing.txt")).await;
    assert_eq!(status, StatusCode::CONFLICT);
}
