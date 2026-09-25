mod common;

use axum::{Router, http::StatusCode};
use common::{
    assert_error_envelope, content_disposition, content_type, create_test_case, delete, get,
    json_request, multipart_request, multipart_without_file, send, send_full, send_json, test_app,
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
async fn attachment_downloads_are_opaque_and_named_for_the_client() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    // Whatever the file is, the body is opaque bytes: a client that picks its
    // decoder from the response content type can never turn a text attachment
    // into a string, and non-UTF-8 bytes survive a download unchanged. The
    // stored media type stays in the document instead of reaching the wire.
    for (name, contents, recorded) in [
        ("report.pdf", b"%PDF-1.4".as_slice(), "application/pdf"),
        ("notes.txt", b"evidence", "text/plain"),
    ] {
        let (status, uploaded) = send_json(
            &app,
            multipart_request("/test_cases/TC-001/attachments", name, contents),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let filename = uploaded["filename"].as_str().expect("stored filename");

        let (status, headers, body) = send_full(
            &app,
            get(&format!("/test_cases/TC-001/attachments/{filename}")),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            content_type(&headers),
            Some("application/octet-stream"),
            "{name} is served opaquely"
        );
        assert_eq!(
            content_disposition(&headers),
            Some(format!("attachment; filename=\"{name}\"").as_str()),
            "{name} is named for the client"
        );
        assert_eq!(body, contents, "{name} round-trips its bytes");

        let (status, document) = send_json(&app, get("/test_cases/TC-001")).await;
        assert_eq!(status, StatusCode::OK);
        let recorded_types: Vec<&str> = document["attachments"]
            .as_array()
            .expect("attachments")
            .iter()
            .map(|entry| entry["mimeType"].as_str().expect("mimeType"))
            .collect();
        assert!(
            recorded_types.contains(&recorded),
            "{name} records its media type: {document}"
        );
    }
}

#[tokio::test]
async fn a_non_ascii_file_name_is_named_for_the_client() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (status, uploaded) = send_json(
        &app,
        multipart_request(
            "/test_cases/TC-001/attachments",
            "rapport-généré.txt",
            b"evidence",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let filename = uploaded["filename"].as_str().expect("stored filename");

    let (status, headers, _) = send_full(
        &app,
        get(&format!("/test_cases/TC-001/attachments/{filename}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The ASCII-safe rendering and the exact name travel together, so a client
    // that understands RFC 5987 saves the file under the name that was
    // uploaded and one that does not still gets a usable name.
    assert_eq!(
        content_disposition(&headers),
        Some(
            "attachment; filename=\"rapport-g_n_r_.txt\"; \
             filename*=UTF-8''rapport-g%C3%A9n%C3%A9r%C3%A9.txt"
        )
    );
}

#[tokio::test]
async fn an_upload_records_when_the_file_arrived() {
    let (_directory, app) = test_app();
    create_test_case(&app, "TC-001").await;

    let (status, uploaded) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "notes.txt", b"evidence"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let filename = uploaded["filename"].as_str().expect("stored filename");

    let (status, document) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);
    let stored = &document["attachments"][0];
    assert_eq!(stored["filename"], filename);
    assert_eq!(stored["originalName"], "notes.txt");
    assert_eq!(stored["mimeType"], "text/plain");
    assert_eq!(stored["size"], 8);

    let uploaded_at = stored["uploadedAt"]
        .as_str()
        .unwrap_or_else(|| panic!("`uploadedAt` is recorded: {document}"));
    assert_eq!(
        uploaded_at.len(),
        20,
        "`uploadedAt` is an ISO-8601 UTC timestamp: {uploaded_at}"
    );
    assert!(
        uploaded_at.ends_with('Z') && &uploaded_at[10..11] == "T",
        "`uploadedAt` is an ISO-8601 UTC timestamp: {uploaded_at}"
    );
}

#[tokio::test]
async fn a_step_attachment_records_no_upload_time() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    let (status, _) = send_json(
        &app,
        multipart_request(
            "/test_cases/TC-STEPS/steps/1/attachments",
            "notes.txt",
            b"evidence",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // `StepAttachment` does not declare `uploadedAt`, so the step entry stays
    // as narrow as its schema.
    let (status, listing) = send_json(&app, get("/test_cases/TC-STEPS/steps/1/attachments")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(listing[0].get("uploadedAt").is_none(), "listing: {listing}");
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
    let message = body["error"]["message"].as_str().expect("error message");
    assert!(
        message.contains("/projects/{id}/test_cases"),
        "the conflict must name the parent-scoped routes: {body}"
    );
    assert!(
        message.contains("/projects/{id}/test_cases/{case_id}/attachments"),
        "an attachment request is told which routes address one occurrence: {body}"
    );
    assert!(
        message.contains(
            "2 parents (project checkout.json, suite smoke.json in project checkout.json)"
        ),
        "the homes are counted the way they are listed: {body}"
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
async fn a_step_attachment_downloads_opaquely_and_named_for_the_client() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    let collection = "/test_cases/TC-STEPS/steps/1/attachments";

    // Whatever the file is, the step's bytes reach the wire opaquely and the
    // response names the file the uploader supplied, exactly as the case-level
    // route does.
    for (name, contents) in [
        ("report.pdf", b"%PDF-1.4".as_slice()),
        ("notes.txt", b"evidence"),
    ] {
        let (status, uploaded) =
            send_json(&app, multipart_request(collection, name, contents)).await;
        assert_eq!(status, StatusCode::CREATED);
        let filename = uploaded["filename"].as_str().expect("stored filename");

        let (status, headers, body) =
            send_full(&app, get(&format!("{collection}/{filename}"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            content_type(&headers),
            Some("application/octet-stream"),
            "{name} is served opaquely"
        );
        assert_eq!(
            content_disposition(&headers),
            Some(format!("attachment; filename=\"{name}\"").as_str()),
            "{name} is named for the client"
        );
        assert_eq!(body, contents, "{name} round-trips its bytes");
    }

    // The stored media type stays in the document, and the step entry keeps the
    // narrow `StepAttachment` shape rather than gaining an `uploadedAt`.
    let (status, stored) = send_json(&app, get("/test_cases/TC-STEPS")).await;
    assert_eq!(status, StatusCode::OK);
    let listed = stored["steps"][1]["attachments"]
        .as_array()
        .expect("attachments");
    let recorded: Vec<&str> = listed
        .iter()
        .map(|entry| entry["mimeType"].as_str().expect("mimeType"))
        .collect();
    assert_eq!(recorded, ["application/pdf", "text/plain"], "{stored}");
    assert!(
        listed.iter().all(|entry| entry.get("uploadedAt").is_none()),
        "a step attachment records no upload time: {stored}"
    );
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
async fn a_missing_step_attachment_download_returns_not_found() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    // An unattached filename in a valid step, an index that addresses no step,
    // and an index that addresses the plain string step: each is simply not
    // found — never a served byte and never a bad request.
    for uri in [
        "/test_cases/TC-STEPS/steps/1/attachments/missing.txt",
        "/test_cases/TC-STEPS/steps/9/attachments/missing.txt",
        "/test_cases/TC-STEPS/steps/0/attachments/missing.txt",
    ] {
        let (status, body) = send_json(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "GET {uri}");
        assert_error_envelope(&body, "not_found");
    }

    // An unknown case is refused before the step or the file is considered.
    let (status, body) = send_json(
        &app,
        get("/test_cases/nope/steps/1/attachments/missing.txt"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
    assert_eq!(body["error"]["message"], "Test case not found");
}

#[tokio::test]
async fn a_step_attachment_download_name_may_not_traverse() {
    let (_directory, app) = test_app();
    create_case_with_steps(&app).await;

    // A name that tries to climb out of the step's folder or to name a nested
    // path is refused rather than read, and no byte is served.
    for uri in [
        "/test_cases/TC-STEPS/steps/1/attachments/..%2F..%2Fescape.txt",
        "/test_cases/TC-STEPS/steps/1/attachments/nested%2Fchild.txt",
    ] {
        let (status, body) = send_json(&app, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "GET {uri}");
        assert_error_envelope(&body, "not_found");
    }
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
    // `invalid_id` and not a new error code. The `{filename}` route serves both
    // the download and the delete, and the index is rejected before either.
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
            get("/test_cases/TC-STEPS/steps/nope/attachments/missing.txt"),
            "GET /test_cases/TC-STEPS/steps/nope/attachments/missing.txt",
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

#[tokio::test]
async fn an_over_long_attachment_name_is_a_client_error_not_a_storage_failure() {
    let (_directory, app) = test_app();

    // Two cases in one project: a plain one for the case-level route and one
    // carrying a structured step for the step-level route.
    let project = common::create_project(&app, "checkout").await;
    let cases = format!("/projects/{project}/test_cases");
    common::create_case_in(&app, &cases, "TC-001").await;
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &cases,
            &json!({
                "testCaseId": "TC-STEPS",
                "title": "Order flow",
                "expectedResult": "Confirmation modal shown",
                "steps": ["Open the product page", {"action": "Click Checkout"}],
            }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "creating the step case: {body}"
    );

    // The stored name prefixes the client's own with a unique suffix, so a file
    // name that is acceptable on its own can still compose a name the
    // filesystem cannot hold. Refusing it here keeps the upload a bad request
    // instead of a write that fails with `ENAMETOOLONG`.
    let refused = "n".repeat(255);
    let acceptable = "n".repeat(200);
    let case_uri = "/test_cases/TC-001/attachments";
    let step_uri = "/test_cases/TC-STEPS/steps/1/attachments";

    for uri in [case_uri, step_uri] {
        let (status, body) = send_json(&app, multipart_request(uri, &refused, b"evidence")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "POST {uri}: {body}");
        assert_error_envelope(&body, "invalid_request");
    }

    let (status, uploaded) =
        send_json(&app, multipart_request(case_uri, &acceptable, b"evidence")).await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let filename = uploaded["filename"].as_str().expect("stored filename");
    assert!(
        filename.ends_with(&format!("-{acceptable}")),
        "stored name: {filename}"
    );

    // A refused upload leaves nothing behind.
    let (status, document) = send_json(&app, get("/test_cases/TC-001")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        document["attachments"]
            .as_array()
            .expect("attachments")
            .len(),
        1,
        "only the accepted upload is recorded: {document}"
    );

    // A name the filesystem could not hold is a client error on the file
    // routes too, never a storage failure.
    let over_long = "n".repeat(256);
    let case_file = format!("{case_uri}/{over_long}");

    let (status, body) = send_json(&app, get(&case_file)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "GET {case_file}: {body}");
    assert_error_envelope(&body, "not_found");

    // The step file route serves DELETE only, so the same bound is exercised
    // through the verb it answers.
    for uri in [case_file, format!("{step_uri}/{over_long}")] {
        let (status, body) = send_json(&app, delete(&uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "DELETE {uri}: {body}");
        assert_error_envelope(&body, "not_found");
    }
}

/// One identifier in two folders, each copy carrying a structured step: the
/// project's own case and the copy composed into its suite. This is the shape
/// the bare-identifier routes refuse and the parent-scoped routes exist for.
///
/// Returns the project and the suite, both holding a case named `TC-001` whose
/// second step is structured and whose first is still a plain string.
async fn an_ambiguous_case(app: &Router) -> (String, String) {
    let project = common::create_project(app, "checkout").await;
    let suite = common::create_suite(app, &project, "smoke").await;

    let (status, body) = send_json(
        app,
        json_request(
            "POST",
            &format!("/projects/{project}/test_cases"),
            &json!({
                "testCaseId": "TC-001",
                "title": "Order flow",
                "expectedResult": "Confirmation modal shown",
                "steps": [
                    "Open the product page",
                    {"action": "Click Checkout", "expectedResult": "Payment screen"}
                ]
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating the case: {body}");

    let (status, body) = send_json(
        app,
        json_request(
            "POST",
            &format!("/test_suites/{suite}/test_cases"),
            &json!({"testCaseId": "TC-001"}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "copying the case into the suite: {body}"
    );

    (project, suite)
}

#[tokio::test]
async fn a_parent_scoped_attachment_route_reaches_the_occurrence_it_names() {
    let (_directory, app) = test_app();
    let (project, suite) = an_ambiguous_case(&app).await;

    // The bare identifier names two folders, so it cannot carry a file.
    let (status, body) = send_json(
        &app,
        multipart_request("/test_cases/TC-001/attachments", "notes.txt", b"bare"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");

    // Each parent-scoped route names the occurrence it means.
    let project_uri = format!("/projects/{project}/test_cases/TC-001/attachments");
    let suite_uri = format!("/test_suites/{suite}/test_cases/TC-001/attachments");

    let (status, uploaded) = send_json(
        &app,
        multipart_request(&project_uri, "notes.txt", b"project copy"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "project upload: {uploaded}");
    let project_file = uploaded["filename"]
        .as_str()
        .expect("stored filename")
        .to_owned();
    assert!(
        project_file.ends_with("-notes.txt"),
        "stored name: {project_file}"
    );

    let (status, uploaded) = send_json(
        &app,
        multipart_request(&suite_uri, "notes.txt", b"suite copy"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "suite upload: {uploaded}");
    let suite_file = uploaded["filename"]
        .as_str()
        .expect("stored filename")
        .to_owned();
    assert!(
        suite_file.ends_with("-notes.txt"),
        "stored name: {suite_file}"
    );

    // Each occurrence serves its own bytes, opaquely and named for the client,
    // and holds nothing of the other's.
    let (status, headers, contents) =
        send_full(&app, get(&format!("{project_uri}/{project_file}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("application/octet-stream"));
    assert_eq!(
        content_disposition(&headers),
        Some("attachment; filename=\"notes.txt\"")
    );
    assert_eq!(contents, b"project copy");

    let (status, headers, contents) =
        send_full(&app, get(&format!("{suite_uri}/{suite_file}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("application/octet-stream"));
    assert_eq!(
        content_disposition(&headers),
        Some("attachment; filename=\"notes.txt\"")
    );
    assert_eq!(contents, b"suite copy");

    let (status, _) = send(&app, get(&format!("{project_uri}/{suite_file}"))).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the project does not hold the suite's file"
    );

    let (status, _) = send(&app, get(&format!("{suite_uri}/{project_file}"))).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the suite does not hold the project's file"
    );

    // Deleting one occurrence's file leaves the other's in place.
    let (status, _) = send_json(&app, delete(&format!("{project_uri}/{project_file}"))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&app, get(&format!("{project_uri}/{project_file}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, contents) = send(&app, get(&format!("{suite_uri}/{suite_file}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(contents, b"suite copy");

    // The suite-scoped delete is answered the same way, and the file it names is
    // the only one it removes.
    let (status, _) = send_json(&app, delete(&format!("{suite_uri}/{suite_file}"))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&app, get(&format!("{suite_uri}/{suite_file}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_parent_scoped_step_attachment_route_reaches_the_occurrence_it_names() {
    let (_directory, app) = test_app();
    let (project, suite) = an_ambiguous_case(&app).await;

    let project_uri = format!("/projects/{project}/test_cases/TC-001/steps/1/attachments");
    let suite_uri = format!("/test_suites/{suite}/test_cases/TC-001/steps/1/attachments");

    let (status, listing) = send_json(&app, get(&suite_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]), "a step starts with no attachments");

    let (status, uploaded) = send_json(
        &app,
        multipart_request(&project_uri, "shot.png", b"project step"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "project step: {uploaded}");
    let project_file = uploaded["filename"]
        .as_str()
        .expect("stored filename")
        .to_owned();

    let (status, uploaded) = send_json(
        &app,
        multipart_request(&suite_uri, "shot.png", b"suite step"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "suite step: {uploaded}");
    let suite_file = uploaded["filename"]
        .as_str()
        .expect("stored filename")
        .to_owned();

    // Each copy's step lists its own file only, and the metadata is recorded on
    // the copy the path named.
    let (status, listing) = send_json(&app, get(&project_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing,
        json!([{
            "filename": project_file,
            "originalName": "shot.png",
            "mimeType": "image/png",
            "size": 12,
        }])
    );

    let (status, listing) = send_json(&app, get(&suite_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing,
        json!([{
            "filename": suite_file,
            "originalName": "shot.png",
            "mimeType": "image/png",
            "size": 10,
        }])
    );

    let (status, project_document) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        project_document["testCases"][0]["steps"][1]["attachments"][0]["filename"],
        project_file.as_str()
    );
    assert_eq!(
        project_document["testCases"][0]["steps"][0], "Open the product page",
        "the plain string step keeps its shape"
    );

    let (status, suite_document) = send_json(&app, get(&format!("/test_suites/{suite}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        suite_document["testCases"][0]["steps"][1]["attachments"][0]["filename"],
        suite_file.as_str()
    );

    // Deleting through one parent leaves the other parent's step file alone.
    let (status, _) = send_json(&app, delete(&format!("{project_uri}/{project_file}"))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, listing) = send_json(&app, get(&project_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    // The parent-scoped step surface has no download route, so the suite's copy
    // is checked through the listing that does reach it.
    let (status, listing) = send_json(&app, get(&suite_uri)).await;
    assert_eq!(status, StatusCode::OK, "the suite's step file survives");
    assert_eq!(listing[0]["filename"], suite_file.as_str());

    // The suite-scoped step delete is answered the same way, and the listing
    // that reaches the occurrence reports the file gone.
    let (status, _) = send_json(&app, delete(&format!("{suite_uri}/{suite_file}"))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, listing) = send_json(&app, get(&suite_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));
}

#[tokio::test]
async fn a_parent_scoped_attachment_route_requires_the_named_parent_to_hold_the_case() {
    let (_directory, app) = test_app();
    let holding = common::create_project(&app, "checkout").await;
    let other = common::create_project(&app, "billing").await;
    common::create_case_in(&app, &format!("/projects/{holding}/test_cases"), "TC-001").await;

    // Every shape of the parent-scoped attachment surface, asked of a parent
    // that does not hold the case and of a project that does not exist: the
    // route never falls back to the occurrence the identifier does have.
    for parent in [
        format!("/projects/{other}"),
        "/projects/nowhere.json".to_owned(),
    ] {
        let case = format!("{parent}/test_cases/TC-001");
        for (request, label) in [
            (
                multipart_request(&format!("{case}/attachments"), "notes.txt", b"evidence"),
                "POST attachments",
            ),
            (
                get(&format!("{case}/attachments/notes.txt")),
                "GET attachment",
            ),
            (
                delete(&format!("{case}/attachments/notes.txt")),
                "DELETE attachment",
            ),
            (
                get(&format!("{case}/steps/1/attachments")),
                "GET step attachments",
            ),
            (
                multipart_request(
                    &format!("{case}/steps/1/attachments"),
                    "notes.txt",
                    b"evidence",
                ),
                "POST step attachment",
            ),
            (
                delete(&format!("{case}/steps/1/attachments/notes.txt")),
                "DELETE step attachment",
            ),
        ] {
            let (status, body) = send_json(&app, request).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{label} {case}");
            assert_error_envelope(&body, "not_found");
            assert_eq!(
                body["error"]["message"], "Test case not found",
                "{label} {case}"
            );
        }
    }

    // The parent that does hold the case still answers, so the guard refuses
    // the wrong parent rather than the route.
    let (status, _) = send_json(
        &app,
        multipart_request(
            &format!("/projects/{holding}/test_cases/TC-001/attachments"),
            "notes.txt",
            b"evidence",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}
