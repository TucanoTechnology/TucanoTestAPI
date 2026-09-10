mod common;

use axum::{Router, http::StatusCode};
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

/// Creates a case whose `steps` array holds a plain string step, a structured
/// step, and a second structured step, so a step attachment can address one of
/// them while leaving its neighbours untouched.
async fn create_case_with_steps(app: &Router) {
    let project = common::create_project(app, "checkout").await;
    let (status, body) = send_json(
        app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_cases"),
            &json!({
                "testCaseId": "TC-STEPS",
                "title": "Order flow",
                "expectedResult": "Confirmation modal shown",
                "steps": [
                    "Open the product page",
                    { "action": "Click Checkout", "expectedResult": "Payment screen" },
                    { "action": "Click Pay Now" }
                ]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating the case: {body}");
}

#[tokio::test]
async fn step_attachments_support_upload_list_and_delete() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    let collection = "/test_cases/TC-STEPS/steps/1/attachments";
    let (status, uploaded) = send_json(
        &app,
        multipart_request(collection, "notes.txt", b"evidence"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(uploaded["originalName"], "notes.txt");
    assert_eq!(uploaded["size"], 8);
    let filename = uploaded["filename"]
        .as_str()
        .expect("stored filename")
        .to_owned();
    assert!(filename.ends_with("-notes.txt"), "stored name: {filename}");

    let (status, listing) = send_json(&app, get(collection)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing,
        json!([{
            "filename": filename,
            "originalName": "notes.txt",
            "mimeType": "text/plain",
            "size": 8,
        }])
    );

    // The metadata lives on the addressed step, and the neighbouring steps keep
    // their shape: the plain string step is still a string and the other
    // structured step omits the key it never carried.
    let (status, stored) = send_json(&app, get("/test_cases/TC-STEPS")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["steps"][0], "Open the product page");
    assert_eq!(stored["steps"][1]["attachments"][0]["filename"], filename);
    assert!(stored["steps"][2].get("attachments").is_none());

    let (status, _) = send_json(&app, delete(&format!("{collection}/{filename}"))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, listing) = send_json(&app, get(collection)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, body) = send_json(&app, delete(&format!("{collection}/{filename}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn a_step_without_attachments_lists_an_empty_array() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    let (status, listing) = send_json(&app, get("/test_cases/TC-STEPS/steps/2/attachments")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));
}

#[tokio::test]
async fn a_missing_step_attachment_returns_not_found() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    let (status, body) = send_json(
        &app,
        delete("/test_cases/TC-STEPS/steps/1/attachments/missing.txt"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn a_step_upload_requires_a_file_part() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    let (status, body) = send_json(
        &app,
        multipart_without_file("/test_cases/TC-STEPS/steps/1/attachments"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "missing_file");
}

#[tokio::test]
async fn step_attachments_reject_an_unusable_step_index() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    // A value that is not a non-negative integer is a bad request, not an
    // `invalid_id` and not a new error code. The `{filename}` route only
    // serves DELETE, so a bad index there is exercised through DELETE.
    for (request, label) in [
        (
            get("/test_cases/TC-STEPS/steps/nope/attachments"),
            "GET /test_cases/TC-STEPS/steps/nope/attachments",
        ),
        (
            get("/test_cases/TC-STEPS/steps/-1/attachments"),
            "GET /test_cases/TC-STEPS/steps/-1/attachments",
        ),
        (
            delete("/test_cases/TC-STEPS/steps/nope/attachments/missing.txt"),
            "DELETE /test_cases/TC-STEPS/steps/nope/attachments/missing.txt",
        ),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}");
        assert_error_envelope(&body, "invalid_request");
    }

    // An index that is an integer but addresses no step, and one that addresses
    // the plain string step, are bad requests too.
    for uri in [
        "/test_cases/TC-STEPS/steps/9/attachments",
        "/test_cases/TC-STEPS/steps/0/attachments",
    ] {
        let (status, body) =
            send_json(&app, multipart_request(uri, "notes.txt", b"evidence")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "POST {uri}");
        assert_error_envelope(&body, "invalid_request");

        let (status, body) = send_json(&app, get(uri)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "GET {uri}");
        assert_error_envelope(&body, "invalid_request");
    }
}

#[tokio::test]
async fn step_attachments_do_not_collide_with_case_attachments() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    // The same original name in both namespaces: the case-level file and the
    // step-level file are separate occurrences.
    let (status, case_level) = send_json(
        &app,
        multipart_request(
            "/test_cases/TC-STEPS/attachments",
            "shot.png",
            b"case-level",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let case_filename = case_level["filename"].as_str().expect("case filename");
    assert!(case_filename.ends_with("-shot.png"));

    let (status, step_level) = send_json(
        &app,
        multipart_request(
            "/test_cases/TC-STEPS/steps/1/attachments",
            "shot.png",
            b"step-level",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let step_filename = step_level["filename"].as_str().expect("step filename");
    assert!(step_filename.ends_with("-shot.png"));
    assert_ne!(case_filename, step_filename);

    // The case's own attachment is still readable and is not the step's file.
    let (status, contents) = send(
        &app,
        get(&format!("/test_cases/TC-STEPS/attachments/{case_filename}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(contents, b"case-level");

    // Deleting the step attachment leaves the case attachment alone.
    let (status, _) = send_json(
        &app,
        delete(&format!(
            "/test_cases/TC-STEPS/steps/1/attachments/{step_filename}"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, contents) = send(
        &app,
        get(&format!("/test_cases/TC-STEPS/attachments/{case_filename}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(contents, b"case-level");
}

#[tokio::test]
async fn a_step_attachment_name_may_not_traverse() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    let (status, body) = send_json(
        &app,
        multipart_request(
            "/test_cases/TC-STEPS/steps/1/attachments",
            "../escape.txt",
            b"evidence",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "traversing name: {body}");
    assert_error_envelope(&body, "not_found");

    let (status, listing) = send_json(&app, get("/test_cases/TC-STEPS/steps/1/attachments")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));
}
