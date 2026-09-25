mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{
    ROLE_CHECKED_WRITE_OPERATIONS, app_at, assert_error_envelope, content_type, create_case_in,
    create_named, create_suite, fixture_home, get, json_request, raw_json_request, send, send_full,
    send_json, test_app,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tempfile::TempDir;
use tucano_test::api;

#[tokio::test]
async fn health_reports_filesystem_storage() {
    let (_directory, app) = test_app();
    let (status, body) = send_json(&app, get("/health")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["storage"], "filesystem");
}

#[tokio::test]
async fn ready_reports_the_store_behind_a_live_process() {
    let (_directory, app) = test_app();
    let (status, body) = send_json(&app, get("/ready")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready");
    assert_eq!(body["storage"], "filesystem");
}

#[tokio::test]
async fn diagnostics_reports_the_probe_and_names_no_path() {
    let (directory, app) = test_app();
    let (status, bytes) = send(&app, get("/diagnostics")).await;
    let body: Value = serde_json::from_slice(&bytes).expect("diagnostics body");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["storage"], "filesystem");
    assert_eq!(body["ready"], true);
    assert_eq!(body["exists"], true);
    assert_eq!(body["writable"], true);
    assert_eq!(body["lockable"], true);
    assert_eq!(body["lockHeld"], false);
    assert!(body["lastWriteUnix"].is_number());

    // The storage-security rule keeps deployment layout out of responses: the
    // probe describes the store without ever naming where it is.
    let root = directory.path().to_string_lossy().into_owned();
    let rendered = String::from_utf8(bytes).expect("utf-8 body");
    assert!(
        !rendered.contains(&root),
        "the probe leaked the data directory: {rendered}"
    );
}

#[tokio::test]
async fn ready_answers_503_when_the_store_cannot_take_writes() {
    let directory = TempDir::new().expect("temp dir");
    let app = app_at(directory.path());
    std::fs::remove_dir_all(directory.path()).expect("remove the data directory");

    // Liveness is not readiness: the process still serves, so `/health` is
    // untouched by a store that is gone.
    let (status, _) = send_json(&app, get("/health")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send_json(&app, get("/ready")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_error_envelope(&body, "not_ready");
    let message = body["error"]["message"]
        .as_str()
        .expect("readiness message");
    assert!(
        message.contains("missing"),
        "the message must say which check failed: {message}"
    );
    assert!(
        !message.contains(&directory.path().to_string_lossy().into_owned()),
        "the message leaked the data directory: {message}"
    );

    // The report is the operator's half: it answers `200` with the failing
    // checks visible, which is what a `503` from `/ready` cannot say.
    let (status, body) = send_json(&app, get("/diagnostics")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ready"], false);
    assert_eq!(body["exists"], false);
    assert_eq!(body["writable"], false);
    assert_eq!(body["lockable"], false);
    assert_eq!(body["lastWriteUnix"], Value::Null);
}

#[tokio::test]
async fn openapi_document_matches_the_registered_routes() {
    let (_directory, app) = test_app();
    let (status, body) = send_json(&app, get("/openapi.json")).await;

    assert_eq!(status, StatusCode::OK);
    let documented: BTreeSet<String> = body["paths"]
        .as_object()
        .expect("paths object")
        .keys()
        .cloned()
        .collect();

    for route in api::UNDOCUMENTED_ROUTES {
        assert!(
            api::ROUTES.contains(route),
            "exempt route is not registered: {route}"
        );
    }

    let expected: BTreeSet<String> = api::ROUTES
        .iter()
        .copied()
        .filter(|route| !api::UNDOCUMENTED_ROUTES.contains(route))
        .map(str::to_owned)
        .collect();

    assert_eq!(
        documented, expected,
        "openapi.json no longer matches the registered routes"
    );
}

#[tokio::test]
async fn router_serves_every_declared_route() {
    let (_directory, app) = test_app();

    // No route accepts PATCH, so a registered path answers 405 and an
    // unregistered one answers 404 — which is exactly the distinction to test.
    for route in api::ROUTES {
        let (status, _) = send(&app, raw_json_request("PATCH", route, "")).await;
        assert_eq!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{route} is declared in api::ROUTES but the router does not serve it"
        );
    }

    let (status, _) = send(&app, raw_json_request("PATCH", "/not-a-route", "")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_and_update_reject_unknown_fields() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"name": "checkout", "unknownField": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    let (status, listed) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed, json!([]), "a rejected body must not be stored");

    let id = common::create_named(&app, "/projects", "checkout").await;
    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            &format!("/projects/{id}"),
            &json!({"name": "renamed", "unknownField": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
}

#[tokio::test]
async fn swagger_ui_is_served_with_and_without_trailing_slash() {
    let (_directory, app) = test_app();

    for uri in ["/api-docs", "/api-docs/"] {
        let (status, bytes) = send(&app, get(uri)).await;
        let html = String::from_utf8(bytes).expect("utf8 html");

        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains("SwaggerUIBundle"), "{uri}");
        assert!(html.contains("/openapi.json"), "{uri}");
    }
}

#[tokio::test]
async fn malformed_request_bodies_are_rejected() {
    let (_directory, app) = test_app();

    let (status, _) = send(&app, raw_json_request("POST", "/projects", "{ not json")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn identifiers_cannot_escape_the_storage_root() {
    let (_directory, app) = test_app();

    for uri in [
        "/projects/..%2F..%2Fescape.json",
        "/projects/nested%2Fchild.json",
        "/test_cases/..%2Fescape",
        "/test_cases/TC-001/attachments/..%2F..%2Fescape.txt",
        "/test_cases/TC-001/steps/0/attachments/..%2F..%2Fescape.txt",
    ] {
        let (status, _) = send(&app, get(uri)).await;
        assert!(
            status.is_client_error(),
            "traversal attempt should be rejected: {uri} returned {status}"
        );
    }
}

#[tokio::test]
async fn stored_documents_survive_a_repository_restart() {
    let directory = TempDir::new().expect("temp dir");

    {
        let app = app_at(directory.path());
        let (status, _) = send_json(
            &app,
            json_request("POST", "/projects", &json!({"name": "checkout"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let app = app_at(directory.path());
    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], "checkout");
}

#[tokio::test]
async fn the_tree_is_stored_as_folders_that_mirror_the_hierarchy() {
    let directory = TempDir::new().expect("temp dir");
    let app = app_at(directory.path());

    let project = common::create_project(&app, "checkout").await;
    let suite = common::create_suite(&app, &project, "smoke").await;
    let direct =
        common::create_case_in(&app, &format!("/projects/{project}/test_cases"), "TC-001").await;
    let nested =
        common::create_case_in(&app, &format!("/test_suites/{suite}/test_cases"), "TC-002").await;

    // The disk mirrors the conceptual organisation: a project folder holds its
    // suites and its own cases, and a suite folder holds its cases.
    let root = directory.path();
    assert!(root.join("projects/checkout/project.json").is_file());
    assert!(root.join("projects/checkout/smoke/suite.json").is_file());
    assert!(
        root.join(format!("projects/checkout/{direct}/test-case.json"))
            .is_file()
    );
    assert!(
        root.join(format!("projects/checkout/smoke/{nested}/test-case.json"))
            .is_file()
    );

    // Membership lives in the folders, so the markers keep their child arrays
    // empty — and a name-only creation still records the identity its folder
    // name stands for, so a stored document reads back as its typed model.
    let suite_marker = read_json(&root.join("projects/checkout/smoke/suite.json"));
    assert_eq!(suite_marker["suiteId"], suite);
    assert_eq!(suite_marker["testCases"], json!([]));

    let project_marker = read_json(&root.join("projects/checkout/project.json"));
    assert_eq!(project_marker["projectId"], project);
    assert_eq!(project_marker["testSuites"], json!([]));

    // Reads assemble what the folders hold rather than serving the markers.
    let (_, assembled) = send_json(&app, get(&format!("/projects/{project}"))).await;
    assert_eq!(assembled["testSuites"][0]["suiteId"], suite);
    assert_eq!(
        assembled["testSuites"][0]["testCases"][0]["testCaseId"],
        nested
    );
    assert_eq!(assembled["testCases"][0]["testCaseId"], direct);

    // Deleting a project cascades: no folder below it survives.
    let (status, _) = send_json(&app, common::delete(&format!("/projects/{project}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !root.join("projects/checkout").exists(),
        "a cascade delete leaves no folder behind"
    );
    let (status, _) = send_json(&app, get(&format!("/test_suites/{suite}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The folder that holds a resource is what scopes its identifier: a name is
/// taken inside its own project, and the same name is free in another one.
#[tokio::test]
async fn an_identifier_is_unique_inside_its_project_and_free_across_projects() {
    let (_directory, app) = test_app();

    let alpha = common::create_project(&app, "alpha").await;
    let beta = common::create_project(&app, "beta").await;

    let resources = [
        ("test_suites", json!({"name": "smoke"})),
        (
            "test_cases",
            json!({"testCaseId": "TC-1", "title": "Login", "expectedResult": "Stored"}),
        ),
        (
            "test_runs",
            json!({"name": "nightly", "timestamp": "2026-09-04T00:00:00Z"}),
        ),
        ("milestones", json!({"name": "sprint-42"})),
        ("configurations", json!({"name": "chrome-linux"})),
    ];

    for (collection, body) in &resources {
        let mut created = Vec::new();
        for project in [&alpha, &beta] {
            let uri = format!("/projects/{project}/{collection}");
            let (status, answer) = send_json(&app, json_request("POST", &uri, body)).await;
            assert_eq!(status, StatusCode::CREATED, "POST {uri}: {answer}");
            created.push(answer["id"].as_str().expect("created id").to_owned());
        }
        assert_eq!(
            created[0], created[1],
            "the same name derived a different identifier in each project: {collection}"
        );

        // The identifier is taken once the project holds it: the second
        // creation in the same project is the conflict.
        let uri = format!("/projects/{alpha}/{collection}");
        let (status, answer) = send_json(&app, json_request("POST", &uri, body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "POST {uri}: {answer}");
        assert_error_envelope(&answer, "conflict");

        // Reads stay global, so both homes answer the one identifier.
        let (status, listed) = send_json(&app, get(&format!("/{collection}"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed, json!([created[0]]), "{collection}");
    }
}

/// The body the router's request-size limit answers with.
const LENGTH_LIMIT_BODY: &str = "length limit exceeded";

#[tokio::test]
async fn an_unusable_path_identifier_is_answered_with_invalid_id() {
    let (_directory, app) = test_app();

    // These operations resolve the identifier in the path against a `.json`
    // document, so an identifier that is not a single such component is a bad
    // identifier rather than a missing document — every one of them documents
    // the `InvalidId` response in openapi.json.
    //
    // The composition routes reach that lookup only through one of two arms: a
    // creation body goes to `require_parent`, a placement body reads the
    // identifier it names from the body first and checks the parent afterwards.
    // An empty body would take neither, so these carry a creation payload; the
    // order is recorded in `docs/contracts/api-compatibility.md`.
    let suite_creation = json!({"name": "copy of smoke"});
    let case_creation = json!({"testCaseId": "TC-1", "title": "t", "expectedResult": "e"});
    // The run routes check the fields their body requires before they read the
    // run, so those bodies carry them.
    let attaching = json!({"suiteId": "nope.json"});
    let recording = json!({"testCaseId": "nope", "status": "Passed"});
    for (method, uri, body) in [
        ("GET", "/projects/nope", json!({})),
        ("PUT", "/projects/nope", json!({})),
        ("DELETE", "/projects/nope", json!({})),
        ("GET", "/projects/nope/test_suites", json!({})),
        ("POST", "/projects/nope/test_suites", suite_creation),
        ("DELETE", "/projects/nope/test_suites/nope", json!({})),
        ("GET", "/projects/nope/test_cases", json!({})),
        ("POST", "/projects/nope/test_cases", case_creation.clone()),
        ("DELETE", "/projects/nope/test_cases/nope", json!({})),
        ("GET", "/projects/nope/test_runs", json!({})),
        ("GET", "/projects/nope/milestones", json!({})),
        ("GET", "/projects/nope/configurations", json!({})),
        ("GET", "/test_suites/nope", json!({})),
        ("PUT", "/test_suites/nope", json!({})),
        ("DELETE", "/test_suites/nope", json!({})),
        ("GET", "/test_suites/nope/test_cases", json!({})),
        (
            "POST",
            "/test_suites/nope/test_cases",
            case_creation.clone(),
        ),
        ("DELETE", "/test_suites/nope/test_cases/nope", json!({})),
        ("GET", "/test_runs/nope", json!({})),
        ("PUT", "/test_runs/nope", json!({})),
        ("DELETE", "/test_runs/nope", json!({})),
        ("POST", "/test_runs/nope/test_suites", attaching),
        (
            "POST",
            "/test_runs/nope/test_cases",
            json!({"testCaseId": "nope"}),
        ),
        ("POST", "/test_runs/nope/results", recording),
        ("GET", "/milestones/nope", json!({})),
        ("PUT", "/milestones/nope", json!({})),
        ("DELETE", "/milestones/nope", json!({})),
        ("GET", "/milestones/nope/progress", json!({})),
        ("GET", "/configurations/nope", json!({})),
        ("PUT", "/configurations/nope", json!({})),
        ("DELETE", "/configurations/nope", json!({})),
    ] {
        let (status, body) = send_json(&app, json_request(method, uri, &body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}");
        assert_error_envelope(&body, "invalid_id");
    }

    // The run routes also take the identifier of the resource they attach from
    // the body, and refuse an unusable one there the same way. The run is named
    // only, so reaching the identifier at all depends on the identity fields the
    // API records for a run created from a name alone. A run lives inside a
    // project, so the creation names one.
    let home = common::fixture_home(&app).await;
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/projects/{home}/test_runs"),
            &json!({"name": "nightly"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating the run: {created}");
    let run = created["id"].as_str().expect("created id");
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/test_runs/{run}/test_suites"),
            &json!({"suiteId": "nope"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_id");
}

#[tokio::test]
async fn a_test_case_identifier_is_addressed_verbatim() {
    let (_directory, app) = test_app();

    // A case is a folder rather than a document, so its identifier carries no
    // suffix and an unusable one is simply a case that does not exist — never
    // `invalid_id`.
    for (method, uri) in [
        ("GET", "/test_cases/nope"),
        ("PUT", "/test_cases/nope"),
        ("DELETE", "/test_cases/nope"),
        ("POST", "/test_cases/nope/duplicate"),
        ("GET", "/test_cases/nope/attachments/missing.txt"),
        ("DELETE", "/test_cases/nope/attachments/missing.txt"),
        ("GET", "/test_cases/nope/steps/0/attachments"),
        ("GET", "/test_cases/nope/steps/0/attachments/missing.txt"),
        ("DELETE", "/test_cases/nope/steps/0/attachments/missing.txt"),
    ] {
        let (status, body) = send_json(&app, json_request(method, uri, &json!({}))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert_error_envelope(&body, "not_found");
    }

    let (status, body) = send_json(
        &app,
        common::multipart_request("/test_cases/nope/attachments", "note.txt", b"hello"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");

    let (status, body) = send_json(
        &app,
        common::multipart_request("/test_cases/nope/steps/0/attachments", "note.txt", b"hello"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn a_parent_scoped_attachment_route_reads_its_parent_from_the_path() {
    let (_directory, app) = test_app();

    // The bare routes above address a case by its identifier alone, which
    // carries no suffix, so an unusable one is simply a case that does not
    // exist. A parent-scoped route instead resolves the case from the parent in
    // its path, and both names are read as identifiers of stored documents —
    // the parent a `project.json` or `suite.json` folder, the case the folder
    // that parent holds — so an unusable parent is `invalid_id`, the answer the
    // parent-scoped descriptions in openapi.json record. `TC-1` is a legal case
    // folder name, so the refusal can only come from the parent.
    for (method, uri) in [
        (
            "GET",
            "/projects/nope/test_cases/TC-1/attachments/missing.txt",
        ),
        (
            "DELETE",
            "/projects/nope/test_cases/TC-1/attachments/missing.txt",
        ),
        ("GET", "/projects/nope/test_cases/TC-1/steps/0/attachments"),
        (
            "DELETE",
            "/projects/nope/test_cases/TC-1/steps/0/attachments/missing.txt",
        ),
        (
            "GET",
            "/test_suites/nope/test_cases/TC-1/attachments/missing.txt",
        ),
        (
            "DELETE",
            "/test_suites/nope/test_cases/TC-1/attachments/missing.txt",
        ),
        (
            "GET",
            "/test_suites/nope/test_cases/TC-1/steps/0/attachments",
        ),
        (
            "DELETE",
            "/test_suites/nope/test_cases/TC-1/steps/0/attachments/missing.txt",
        ),
    ] {
        let (status, body) = send_json(&app, json_request(method, uri, &json!({}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}");
        assert_error_envelope(&body, "invalid_id");
    }

    // An upload reaches the same refusal, but only once its multipart framing is
    // usable to begin with: a request the extractor cannot start on is answered
    // in plain text before the handler reads the path at all.
    for uri in [
        "/projects/nope/test_cases/TC-1/attachments",
        "/projects/nope/test_cases/TC-1/steps/0/attachments",
        "/test_suites/nope/test_cases/TC-1/attachments",
        "/test_suites/nope/test_cases/TC-1/steps/0/attachments",
    ] {
        let request = common::multipart_request(uri, "note.txt", b"hello");
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "POST {uri}");
        assert_error_envelope(&body, "invalid_id");
    }
}

#[tokio::test]
async fn duplicate_routes_report_an_unusable_identifier_as_their_own_description_says() {
    let (_directory, app) = test_app();

    // A project reads the identifier as a body field before it reads the path,
    // so an unusable path identifier reaches it as a bad request.
    let (status, body) = send_json(
        &app,
        json_request("POST", "/projects/nope/duplicate", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "/projects/nope/duplicate");
    assert_error_envelope(&body, "invalid_request");

    // The routes that read a document inside a project must find its home
    // before they can read it, and locating an identifier refuses an unusable
    // one — the same answer every other bare-identifier document route gives.
    for uri in ["/test_runs/nope/duplicate", "/milestones/nope/duplicate"] {
        let (status, body) = send_json(&app, json_request("POST", uri, &json!({}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        assert_error_envelope(&body, "invalid_id");
    }

    // The suite route reads the path identifier first, like the rest of the
    // suite tree, and answers `invalid_id`.
    let (status, body) = send_json(
        &app,
        json_request("POST", "/test_suites/nope/duplicate", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_id");
}

#[tokio::test]
async fn duplicate_routes_refuse_a_body_new_id_the_store_cannot_file() {
    let (_directory, app) = test_app();
    let home = fixture_home(&app).await;
    let suite = create_suite(&app, &home, "smoke").await;
    let run = create_named(&app, "/test_runs", "nightly").await;
    let milestone = create_named(&app, "/milestones", "sprint").await;
    let case = create_case_in(&app, &format!("/projects/{home}/test_cases"), "TC-1").await;

    // Every duplicate route validates the `newId` it is given, so an explicit
    // identifier the store cannot file is refused rather than silently replaced
    // by the derived copy name. A project, suite, run and milestone identifier
    // is a `.json` component, so a bare value is unusable here.
    for uri in [
        format!("/projects/{home}/duplicate"),
        format!("/test_suites/{suite}/duplicate"),
        format!("/test_runs/{run}/duplicate"),
        format!("/milestones/{milestone}/duplicate"),
    ] {
        for new_id in ["probe-moved", "team/copy.json", "", ".."] {
            let (status, body) =
                send_json(&app, json_request("POST", &uri, &json!({"newId": new_id}))).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "POST {uri} newId={new_id:?}: {body}"
            );
            assert_error_envelope(&body, "invalid_id");
        }
    }

    // A case identifier is addressed verbatim and carries no `.json` suffix, so
    // the same values split: nested, empty and climbing identifiers are refused
    // while a bare one is usable as it stands.
    for new_id in ["team/copy", "", ".."] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/test_cases/{case}/duplicate"),
                &json!({"newId": new_id}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "POST /test_cases/{case}/duplicate newId={new_id:?}: {body}"
        );
        assert_error_envelope(&body, "invalid_id");
    }

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/test_cases/{case}/duplicate"),
            &json!({"newId": "TC-2"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["id"], "TC-2");

    // The case route addresses the path identifier verbatim, so an unusable one
    // is a `404` rather than the `invalid_id` the other duplicate routes answer.
    let (status, body) = send_json(
        &app,
        json_request("POST", "/test_cases/nope/duplicate", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn an_oversized_body_is_rejected_in_plain_text_before_the_handler_runs() {
    let (_directory, app) = test_app();
    common::create_test_case(&app, "TC-001").await;

    // The router caps every request body at the attachment limit, so the JSON
    // `payload_too_large` envelope the per-handler checks could produce is
    // unreachable for attachments and this plain-text 413 is the only
    // observable payload-too-large answer.
    for uri in [
        "/projects",
        "/test_cases/TC-001/attachments",
        "/test_runs/nope/results",
    ] {
        let (status, headers, bytes) = send_full(&app, oversized_request(uri)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{uri}");
        assert_eq!(
            content_type(&headers),
            Some("text/plain; charset=utf-8"),
            "{uri} must not answer the error envelope"
        );
        assert_eq!(String::from_utf8_lossy(&bytes), LENGTH_LIMIT_BODY, "{uri}");
    }
}

#[tokio::test]
async fn the_upload_route_answers_plain_text_only_when_multipart_framing_is_unusable() {
    let (_directory, app) = test_app();
    common::create_test_case(&app, "TC-001").await;

    // A request the multipart extractor cannot even start on is rejected before
    // the handler runs, and that answer is plain text rather than the envelope.
    let (status, headers, bytes) = send_full(
        &app,
        raw_json_request("POST", "/test_cases/TC-001/attachments", "{}"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(content_type(&headers), Some("text/plain; charset=utf-8"));
    assert!(
        String::from_utf8_lossy(&bytes).contains("boundary"),
        "the extractor names the missing boundary"
    );

    // Once the framing parses, the rejections are the documented envelope.
    let (status, body) = send_json(
        &app,
        common::multipart_without_file("/test_cases/TC-001/attachments"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "missing_file");

    // And an accepted part answers the 201 body openapi.json documents.
    let (status, body) = send_json(
        &app,
        common::multipart_request("/test_cases/TC-001/attachments", "note.txt", b"hello"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["message"], "File uploaded successfully");
    assert_eq!(body["originalName"], "note.txt");
    assert_eq!(body["size"], 5);
    assert!(body["filename"].is_string(), "the stored name: {body}");
}

#[tokio::test]
async fn openapi_documents_the_error_contract_of_every_operation() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    // Every `$ref` resolves, so no operation silently loses its responses.
    let mut references = Vec::new();
    collect_references(&document, &mut references);
    assert!(!references.is_empty());
    for reference in &references {
        let pointer = reference
            .strip_prefix("#/")
            .unwrap_or_else(|| panic!("not a local reference: {reference}"));
        let mut target = &document;
        for segment in pointer.split('/') {
            target = target
                .get(segment)
                .unwrap_or_else(|| panic!("unresolved reference: {reference}"));
        }
    }

    let schemas = document["components"]["schemas"]
        .as_object()
        .expect("schemas object");

    let operations = documented_operations(&document);
    assert!(operations.len() > 20, "the document lost its operations");

    // Every 2xx that carries a payload declares a schema for each media type it
    // can answer with, so a generated client is never left untyped. Only the
    // three endpoints that serve the document itself answer with no schema.
    const UNTYPED_2XX: [&str; 3] = ["get /health", "get /openapi.json", "get /api-docs"];
    for (label, operation) in &operations {
        let responses = operation["responses"]
            .as_object()
            .unwrap_or_else(|| panic!("{label} has no responses"));

        for (status, response) in responses {
            if !status.starts_with('2') || UNTYPED_2XX.contains(&label.as_str()) {
                continue;
            }
            let response = dereference(&document, response);
            let content = response["content"]
                .as_object()
                .unwrap_or_else(|| panic!("{label} {status} declares no content"));
            assert!(!content.is_empty(), "{label} {status} has no media type");
            for (media, body) in content {
                assert!(
                    body.get("schema").is_some(),
                    "{label} {status} {media} has no schema"
                );
            }
        }

        // Every operation that reads a body documents the router's limit.
        if operation.get("requestBody").is_some() {
            let too_large = responses
                .get("413")
                .unwrap_or_else(|| panic!("{label} accepts a body but does not document 413"));
            let too_large = dereference(&document, too_large);
            let content = too_large["content"]
                .as_object()
                .unwrap_or_else(|| panic!("{label} documents 413 without a body"));
            assert_eq!(content.len(), 1, "{label} 413 content");
            assert!(
                content.contains_key("text/plain"),
                "{label} must document the plain-text 413"
            );
        }

        // Every 400 names the code it produces, so a client can tell them apart.
        if let Some(response) = responses.get("400") {
            let response = dereference(&document, response);
            let description = response["description"]
                .as_str()
                .unwrap_or_else(|| panic!("{label} 400 has no description"));
            assert!(
                ERROR_CODES.iter().any(|code| description.contains(code)),
                "{label} 400 names no error code: {description}"
            );
        }
    }

    // The result body is the run's own record, not the stored document. A
    // replacement is addressed by path, so it publishes its own body shape
    // rather than the one that requires the case to be named again.
    assert_eq!(
        document["paths"]["/test_runs/{id}/results"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/TestResultRequest"
    );
    assert_eq!(
        document["paths"]["/test_runs/{id}/results/{case_id}"]["put"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/TestResultReplaceRequest"
    );

    // The upload route documents both of its 400 shapes.
    let upload = &document["paths"]["/test_cases/{id}/attachments"]["post"];
    assert!(
        upload["requestBody"]["content"]
            .get("multipart/form-data")
            .is_some(),
        "the upload route takes multipart"
    );
    let rejected = dereference(&document, &upload["responses"]["400"]);
    let content = rejected["content"].as_object().expect("400 content");
    assert!(content.contains_key("application/json"), "the envelope");
    assert!(content.contains_key("text/plain"), "the extractor's answer");

    // Each write route names the schema for its body; the schema lists the
    // fields the domain layer accepts, the fields it requires to derive the id,
    // and rejects unknown fields the way `validate_payload` does.
    const NO_REQUIRED: &[&str] = &[];
    const NAME_REQUIRED: &[&str] = &["name"];
    // A replacement is addressed by path, so the case is not the body's to
    // name: only the status the replacement records is required.
    const STATUS_REQUIRED: &[&str] = &["status"];
    for (path, method, name, required) in [
        ("/projects", "post", "ProjectCreateRequest", NAME_REQUIRED),
        ("/projects/{id}", "put", "ProjectUpdateRequest", NO_REQUIRED),
        (
            "/test_suites/{id}",
            "put",
            "TestSuiteUpdateRequest",
            NO_REQUIRED,
        ),
        (
            "/projects/{id}/test_runs",
            "post",
            "TestRunCreateRequest",
            NAME_REQUIRED,
        ),
        (
            "/test_runs/{id}",
            "put",
            "TestRunUpdateRequest",
            NO_REQUIRED,
        ),
        (
            "/test_runs/{id}/results/{case_id}",
            "put",
            "TestResultReplaceRequest",
            STATUS_REQUIRED,
        ),
        (
            "/test_cases/{id}",
            "put",
            "TestCaseUpdateRequest",
            NO_REQUIRED,
        ),
        (
            "/projects/{id}/milestones",
            "post",
            "MilestoneCreateRequest",
            NAME_REQUIRED,
        ),
        (
            "/milestones/{id}",
            "put",
            "MilestoneUpdateRequest",
            NO_REQUIRED,
        ),
        (
            "/projects/{id}/configurations",
            "post",
            "TestConfigurationCreateRequest",
            NAME_REQUIRED,
        ),
        (
            "/configurations/{id}",
            "put",
            "TestConfigurationUpdateRequest",
            NO_REQUIRED,
        ),
    ] {
        let body = &document["paths"][path][method]["requestBody"]["content"]["application/json"]["schema"];
        assert_eq!(
            body["$ref"],
            json!(format!("#/components/schemas/{name}")),
            "{method} {path}"
        );
        let schema = &schemas[name];
        assert!(schema["properties"].is_object(), "{name} has properties");
        assert_eq!(schema["required"], json!(required), "{name} required");
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{name} rejects unknown fields"
        );
    }
}

/// Every operation the document publishes, paired with the test that proves it
/// answers successfully.
///
/// The table is written out rather than derived, because a covering test is one
/// whose body drives the operation *and* asserts its success — a property no
/// scanner reads out of the source with confidence. Writing the pair down keeps
/// the check honest in both directions: a new operation fails the build until it
/// names a test, and a table entry whose operation disappeared fails too, since
/// the check compares the two label sets as sets.
///
/// A row may not cite one of `GENERIC_MATRIX_TESTS`. Those tests walk a list of
/// routes to prove a *shared* refusal — the 4xx every guarded or malformed call
/// produces — which says nothing about whether an operation answers at all.
/// Letting them serve as evidence once kept the check green while the success
/// paths of six operations had no test of their own.
///
/// This table is the declaration half of the guarantee: it says which test
/// *should* drive each operation. `tests/route_coverage.rs`, run through
/// `scripts/coverage-check.sh`, is the observed half — the router records every
/// request it served and that check demands a successful answer for each
/// documented operation, so a route renamed out from under a row fails, as does
/// an operation nothing reached or one that only ever answered an error.
///
/// Neither half subsumes the other. The table names a test but cannot see
/// whether it reaches the route; the recording sees a success but cannot say
/// which test produced it, only that one in the run did. Keep both.
const CONTRACT_COVERAGE: [(&str, &str); 91] = [
    ("get /health", "health_reports_filesystem_storage"),
    (
        "get /openapi.json",
        "openapi_declares_the_security_posture_of_every_operation",
    ),
    (
        "get /api-docs",
        "swagger_ui_is_served_with_and_without_trailing_slash",
    ),
    (
        "get /ready",
        "ready_reports_the_store_behind_a_live_process",
    ),
    (
        "get /diagnostics",
        "diagnostics_reports_the_probe_and_names_no_path",
    ),
    ("get /metrics", "metrics_report_what_the_deployment_served"),
    ("get /projects", "projects_support_the_full_crud_lifecycle"),
    ("post /projects", "projects_support_the_full_crud_lifecycle"),
    (
        "get /projects/{id}",
        "projects_support_the_full_crud_lifecycle",
    ),
    (
        "put /projects/{id}",
        "projects_support_the_full_crud_lifecycle",
    ),
    (
        "delete /projects/{id}",
        "projects_support_the_full_crud_lifecycle",
    ),
    (
        "post /projects/{id}/duplicate",
        "a_duplicate_without_a_new_id_derives_an_addressable_identifier",
    ),
    (
        "get /projects/{id}/test_suites",
        "test_suites_support_the_full_crud_lifecycle",
    ),
    (
        "post /projects/{id}/test_suites",
        "a_suite_can_be_placed_into_another_project",
    ),
    (
        "delete /projects/{id}/test_suites/{suite_id}",
        "deleting_a_suite_through_its_project_removes_it",
    ),
    (
        "get /projects/{id}/test_cases",
        "deleting_a_case_through_a_project_resolves_that_project",
    ),
    (
        "post /projects/{id}/test_cases",
        "a_copied_case_carries_the_revision_snapshots_of_its_source",
    ),
    (
        "delete /projects/{id}/test_cases/{case_id}",
        "deleting_a_case_through_a_project_resolves_that_project",
    ),
    (
        "post /projects/{id}/test_cases/{case_id}/attachments",
        "a_parent_scoped_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "get /projects/{id}/test_cases/{case_id}/attachments/{filename}",
        "a_parent_scoped_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "delete /projects/{id}/test_cases/{case_id}/attachments/{filename}",
        "a_parent_scoped_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "get /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
        "a_parent_scoped_step_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "post /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
        "a_parent_scoped_step_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "delete /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}",
        "a_parent_scoped_step_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "get /projects/{id}/test_runs",
        "deleting_a_run_through_a_project_resolves_that_project",
    ),
    (
        "post /projects/{id}/test_runs",
        "deleting_a_run_through_a_project_resolves_that_project",
    ),
    (
        "delete /projects/{id}/test_runs/{run_id}",
        "deleting_a_run_through_a_project_resolves_that_project",
    ),
    (
        "get /projects/{id}/milestones",
        "deleting_a_milestone_through_a_project_resolves_that_project",
    ),
    (
        "post /projects/{id}/milestones",
        "deleting_a_milestone_through_a_project_resolves_that_project",
    ),
    (
        "delete /projects/{id}/milestones/{milestone_id}",
        "deleting_a_milestone_through_a_project_resolves_that_project",
    ),
    (
        "get /projects/{id}/configurations",
        "deleting_a_configuration_through_a_project_resolves_that_project",
    ),
    (
        "post /projects/{id}/configurations",
        "deleting_a_configuration_through_a_project_resolves_that_project",
    ),
    (
        "delete /projects/{id}/configurations/{config_id}",
        "deleting_a_configuration_through_a_project_resolves_that_project",
    ),
    (
        "get /test_suites/{id}",
        "duplicating_a_test_suite_copies_it_into_the_source_project",
    ),
    (
        "put /test_suites/{id}",
        "test_suites_support_the_full_crud_lifecycle",
    ),
    (
        "delete /test_suites/{id}",
        "duplicating_a_test_suite_copies_it_into_the_source_project",
    ),
    (
        "post /test_suites/{id}/duplicate",
        "duplicating_a_test_suite_copies_it_into_the_source_project",
    ),
    (
        "get /test_suites/{id}/test_cases",
        "test_suites_support_incremental_case_composition",
    ),
    (
        "post /test_suites/{id}/test_cases",
        "test_suites_support_incremental_case_composition",
    ),
    (
        "delete /test_suites/{id}/test_cases/{case_id}",
        "test_suites_support_incremental_case_composition",
    ),
    (
        "post /test_suites/{id}/test_cases/{case_id}/attachments",
        "a_parent_scoped_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "get /test_suites/{id}/test_cases/{case_id}/attachments/{filename}",
        "a_parent_scoped_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "delete /test_suites/{id}/test_cases/{case_id}/attachments/{filename}",
        "a_parent_scoped_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "get /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
        "a_parent_scoped_step_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "post /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments",
        "a_parent_scoped_step_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "delete /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}",
        "a_parent_scoped_step_attachment_route_reaches_the_occurrence_it_names",
    ),
    (
        "get /test_runs/{id}",
        "a_partial_update_keeps_the_fields_the_body_leaves_out",
    ),
    (
        "put /test_runs/{id}",
        "test_runs_support_the_full_crud_lifecycle",
    ),
    (
        "delete /test_runs/{id}",
        "test_runs_support_the_full_crud_lifecycle",
    ),
    (
        "post /test_runs/{id}/duplicate",
        "duplicating_a_run_preserves_its_configuration_links",
    ),
    (
        "post /test_runs/{id}/test_suites",
        "test_runs_support_composition_execution_and_isolation",
    ),
    (
        "post /test_runs/{id}/test_cases",
        "test_runs_support_composition_execution_and_isolation",
    ),
    (
        "post /test_runs/{id}/results",
        "test_runs_support_composition_execution_and_isolation",
    ),
    (
        "put /test_runs/{id}/results/{case_id}",
        "replacing_a_result_keeps_the_defects_it_cannot_describe",
    ),
    (
        "delete /test_runs/{id}/results/{case_id}",
        "removing_a_result_takes_it_out_of_the_run",
    ),
    (
        "get /test_runs/{id}/results/{case_id}/defects",
        "listing_defects_returns_the_links_a_result_carries",
    ),
    (
        "post /test_runs/{id}/results/{case_id}/defects",
        "a_defect_link_can_be_removed_and_is_then_gone",
    ),
    (
        "delete /test_runs/{id}/results/{case_id}/defects/{link_id}",
        "a_defect_link_can_be_removed_and_is_then_gone",
    ),
    (
        "post /test_runs/{id}/import/junit",
        "a_junit_report_counts_duplicates_and_leaves_them_alone",
    ),
    (
        "post /test_runs/{id}/import/json",
        "a_json_import_counts_duplicates_and_leaves_them_alone",
    ),
    (
        "post /test_runs/{id}/configurations",
        "a_run_links_and_unlinks_a_top_level_configuration",
    ),
    (
        "delete /test_runs/{id}/configurations/{config_id}",
        "a_run_links_and_unlinks_a_top_level_configuration",
    ),
    (
        "get /test_cases/{id}",
        "a_new_test_case_is_stamped_with_its_first_version",
    ),
    (
        "put /test_cases/{id}",
        "duplicating_a_test_case_copies_it_into_the_source_home",
    ),
    (
        "delete /test_cases/{id}",
        "duplicating_a_test_case_copies_it_into_the_source_home",
    ),
    (
        "post /test_cases/{id}/duplicate",
        "duplicating_a_test_case_copies_it_into_the_source_home",
    ),
    (
        "post /test_cases/{id}/attachments",
        "attachments_are_removed_with_their_test_case",
    ),
    (
        "get /test_cases/{id}/attachments/{filename}",
        "attachment_downloads_are_opaque_and_named_for_the_client",
    ),
    (
        "delete /test_cases/{id}/attachments/{filename}",
        "attachments_support_upload_download_and_delete",
    ),
    (
        "get /test_cases/{id}/steps/{step_index}/attachments",
        "a_step_attachment_name_may_not_traverse",
    ),
    (
        "post /test_cases/{id}/steps/{step_index}/attachments",
        "a_step_attachment_name_may_not_traverse",
    ),
    (
        "get /test_cases/{id}/steps/{step_index}/attachments/{filename}",
        "a_step_attachment_downloads_opaquely_and_named_for_the_client",
    ),
    (
        "delete /test_cases/{id}/steps/{step_index}/attachments/{filename}",
        "step_attachments_do_not_collide_with_case_attachments",
    ),
    (
        "get /test_cases/{id}/history",
        "a_case_without_qualifying_updates_has_an_empty_history",
    ),
    (
        "get /test_cases/{id}/history/{version}",
        "a_recorded_revision_is_returned_verbatim_and_the_live_version_is_not_a_snapshot",
    ),
    (
        "get /milestones/{id}",
        "a_milestone_created_from_a_name_alone_reads_back_and_reports_progress",
    ),
    (
        "put /milestones/{id}",
        "duplicating_a_milestone_copies_it_into_an_independent_document",
    ),
    (
        "delete /milestones/{id}",
        "deleting_a_milestone_through_a_project_resolves_that_project",
    ),
    (
        "post /milestones/{id}/duplicate",
        "duplicating_a_milestone_copies_it_into_an_independent_document",
    ),
    (
        "get /milestones/{id}/progress",
        "a_milestone_created_from_a_name_alone_reads_back_and_reports_progress",
    ),
    (
        "get /reports/coverage",
        "a_global_report_sums_every_project",
    ),
    (
        "get /reports/summary",
        "an_empty_tree_reports_an_all_zero_summary",
    ),
    (
        "get /releases",
        "release_names_are_distinct_sorted_and_scoped_to_the_caller",
    ),
    (
        "get /configurations/{id}",
        "a_configuration_created_from_a_name_alone_reads_back_as_its_model",
    ),
    (
        "put /configurations/{id}",
        "a_partial_update_keeps_the_fields_the_body_leaves_out",
    ),
    (
        "delete /configurations/{id}",
        "configurations_support_the_full_crud_lifecycle",
    ),
    (
        "get /environments",
        "environment_names_are_distinct_sorted_and_scoped_to_the_caller",
    ),
    (
        "post /auth/login",
        "signing_in_answers_a_session_the_api_accepts",
    ),
    (
        "post /auth/refresh",
        "a_refresh_token_is_spent_by_the_exchange_that_uses_it",
    ),
    (
        "post /auth/logout",
        "signing_out_revokes_the_refresh_token_it_names",
    ),
    (
        "get /auth/me",
        "signing_in_answers_a_session_the_api_accepts",
    ),
];

/// Tests that assert one shared refusal across a matrix of operations.
///
/// They are the API's role and malformation sweeps: each drives many routes and
/// asserts the status they must all refuse with, and none of them asserts that a
/// route answers correctly. They are listed here so `CONTRACT_COVERAGE` cannot
/// quietly fall back on one of them for an operation's only evidence.
const GENERIC_MATRIX_TESTS: [&str; 8] = [
    "every_guarded_operation_refuses_an_anonymous_caller",
    "a_viewer_reads_the_projects_it_reaches_and_cannot_write",
    "an_editor_writes_content_but_not_projects_or_milestones",
    "a_caller_with_no_grant_sees_nothing",
    "a_body_the_endpoint_does_not_understand_is_an_invalid_request",
    "an_unusable_path_identifier_is_answered_with_invalid_id",
    "a_parent_scoped_attachment_route_requires_the_named_parent_to_hold_the_case",
    "test_cross_resource_lock_contention",
];

/// The names of every top-level `#[test]` and `#[tokio::test]` function below
/// `tests/`.
///
/// Only column-zero declarations count, so a test nested in a module — those are
/// unit-style tests over the storage layer — is not mistaken for an integration
/// test, and `tests/common/` is skipped because a directory is not a source file
/// and its helpers are not tests.
fn declared_tests() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
    for entry in std::fs::read_dir(directory).expect("tests/ is readable") {
        let path = entry.expect("a tests/ entry").path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a test file is readable");
        let mut annotated = false;
        for line in source.lines() {
            if line.starts_with("#[") {
                annotated =
                    annotated || line.starts_with("#[test]") || line.starts_with("#[tokio::test");
                continue;
            }
            if let Some(rest) = line
                .strip_prefix("async fn ")
                .or_else(|| line.strip_prefix("fn "))
            {
                if annotated {
                    let name = rest.split(['(', ' ', '<']).next().unwrap_or_default();
                    names.insert(name.to_owned());
                }
                annotated = false;
            } else if !line.trim().is_empty() {
                annotated = false;
            }
        }
    }
    names
}

/// Every operation the API documents is proven by a test of its own.
///
/// The document is the contract, so an operation added to `openapi.json` without
/// a test that asserts its success fails here rather than shipping unverified.
#[tokio::test]
async fn every_documented_operation_has_a_covering_test() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    let documented: BTreeSet<String> = documented_operations(&document)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    let covered: BTreeSet<String> = CONTRACT_COVERAGE
        .iter()
        .map(|(label, _)| (*label).to_owned())
        .collect();

    let uncovered: Vec<&String> = documented.difference(&covered).collect();
    assert!(
        uncovered.is_empty(),
        "documented operations with no covering test: {uncovered:?}"
    );

    let unknown: Vec<&String> = covered.difference(&documented).collect();
    assert!(
        unknown.is_empty(),
        "table entries that name no documented operation: {unknown:?}"
    );

    let declared = declared_tests();
    for generic in GENERIC_MATRIX_TESTS {
        assert!(
            declared.contains(generic),
            "{generic} is listed as a shared matrix test but does not exist"
        );
    }
    for (label, test) in CONTRACT_COVERAGE {
        assert!(
            declared.contains(test),
            "{label} is covered by {test}, which is not a test in tests/"
        );
        assert!(
            !GENERIC_MATRIX_TESTS.contains(&test),
            "{label} is covered only by the shared matrix test {test}"
        );
    }
}

/// The four attachment downloads are the document's only binary answers, and
/// each is typed as bytes and names the file for the client.
///
/// The route used to declare `application/octet-stream` while answering with a
/// content type derived from the stored name, so a client that picks its decoder
/// from the response type read a text attachment as a `string` and never handed
/// the browser a blob (Issue #291). The invariant is derived from the document,
/// not from a list: an operation that publishes a binary body publishes exactly
/// that media type and the `Content-Disposition` header that names the file.
#[tokio::test]
async fn openapi_types_and_names_every_binary_download() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    let mut binary = Vec::new();
    for (label, operation) in documented_operations(&document) {
        let Some(answer) = operation["responses"].get("200") else {
            continue;
        };
        let response = dereference(&document, answer);
        let content = match response["content"].as_object() {
            Some(content) => content,
            None => continue,
        };
        if !content.contains_key("application/octet-stream") {
            continue;
        }
        binary.push(label.clone());
        assert_eq!(
            content.len(),
            1,
            "{label} publishes more than the binary body"
        );
        assert_eq!(
            content["application/octet-stream"]["schema"],
            json!({"type": "string", "format": "binary"}),
            "{label} binary schema"
        );
        let headers = response["headers"]
            .as_object()
            .unwrap_or_else(|| panic!("{label} names no file"));
        let disposition = headers
            .get("Content-Disposition")
            .unwrap_or_else(|| panic!("{label} declares no Content-Disposition"));
        let disposition = dereference(&document, disposition);
        assert!(
            disposition["description"].is_string(),
            "{label} Content-Disposition has no description"
        );
        assert!(
            disposition["schema"].is_object(),
            "{label} Content-Disposition has no schema"
        );
    }

    binary.sort();
    assert_eq!(
        binary,
        [
            "get /projects/{id}/test_cases/{case_id}/attachments/{filename}",
            "get /test_cases/{id}/attachments/{filename}",
            "get /test_cases/{id}/steps/{step_index}/attachments/{filename}",
            "get /test_suites/{id}/test_cases/{case_id}/attachments/{filename}",
        ],
        "the binary downloads are exactly these four"
    );
}

/// The document states the posture the router enforces: every operation takes a
/// bearer token except the five a caller reaches before it holds one, and every
/// operation that checks a project role can answer 403.
#[tokio::test]
async fn openapi_declares_the_security_posture_of_every_operation() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    let bearer = json!([{ "bearerAuth": [] }]);
    assert_eq!(
        document["security"], bearer,
        "the document no longer defaults to bearer authentication"
    );
    let scheme = &document["components"]["securitySchemes"]["bearerAuth"];
    assert_eq!(scheme["type"], json!("http"));
    assert_eq!(scheme["scheme"], json!("bearer"));

    // The endpoints that exist before a caller does, and the two it signs in
    // through: none of them names a token, and the sign-in routes answer their
    // own documented failure instead. The readiness, diagnostics and counters
    // endpoints are unguarded by the same argument as `/health`: an orchestrator
    // that cannot authenticate is exactly the caller that has to ask whether the
    // process is ready, and none of them reports anything a caller may not see.
    const UNGUARDED: [&str; 8] = [
        "get /health",
        "get /ready",
        "get /diagnostics",
        "get /metrics",
        "get /openapi.json",
        "get /api-docs",
        "post /auth/login",
        "post /auth/refresh",
    ];

    let operations = documented_operations(&document);
    assert_eq!(operations.len(), 91, "the documented surface changed");

    for (label, operation) in &operations {
        let responses = operation["responses"].as_object().expect("responses");
        let security = operation.get("security").unwrap_or(&document["security"]);

        if UNGUARDED.contains(&label.as_str()) {
            assert_eq!(security, &json!([]), "{label} must not require a token");
            continue;
        }

        assert_eq!(security, &bearer, "{label} must require a bearer token");
        assert!(
            responses.contains_key("401"),
            "{label} requires a token but documents no 401"
        );

        // A 403 is the answer to a caller that holds a token and still may not
        // touch the resource; where it is documented it names the code a client
        // switches on.
        if let Some(forbidden) = responses.get("403") {
            let forbidden = dereference(&document, forbidden);
            let description = forbidden["description"]
                .as_str()
                .unwrap_or_else(|| panic!("{label} 403 has no description"));
            assert!(
                description.contains("forbidden"),
                "{label} 403 names no error code: {description}"
            );
            assert!(
                forbidden["content"]["application/json"]["schema"].is_object(),
                "{label} 403 has no error envelope"
            );
        }
    }

    // The reverse direction: every route the shared surface lists as
    // role-checked must document the 403 its guard can answer. A route that
    // gains a guard without a matching change to the document fails here, which
    // is the failure the three create routes used to slip through.
    for label in ROLE_CHECKED_WRITE_OPERATIONS {
        let (_, operation) = operations
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("{label} is role-checked but not documented"));
        assert!(
            operation["responses"].get("403").is_some(),
            "{label} is role-checked but documents no 403"
        );
    }

    // The role-checked surface, frozen so a route that silently loses its check
    // fails here rather than in review.
    let refuses = operations
        .iter()
        .filter(|(_, operation)| operation["responses"].get("403").is_some())
        .count();
    assert_eq!(
        refuses, 80,
        "the 403 surface changed; update this count with it"
    );
}

#[tokio::test]
async fn openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
    let schemas = document["components"]["schemas"]
        .as_object()
        .expect("schemas object");

    // These describe documents whose fields the API checks against a model
    // carrying `deny_unknown_fields`, or — for a body a handler reads field by
    // field — against the explicit list of fields that handler accepts. An
    // extra field is a 400 either way, and the schema says so.
    for name in [
        "Attachment",
        "Project",
        "TestSuite",
        "TestCase",
        "TestStep",
        "StepAttachment",
        "TestCaseResult",
        "DefectLink",
        "DefectLinkRequest",
        "Milestone",
        "TestConfiguration",
        "ImportEntry",
        "TestRun",
        "TestResultRequest",
        "TestResultReplaceRequest",
        "ProjectCreateRequest",
        "ProjectUpdateRequest",
        "TestSuiteUpdateRequest",
        "TestRunCreateRequest",
        "TestRunUpdateRequest",
        "TestCaseUpdateRequest",
        "MilestoneCreateRequest",
        "MilestoneUpdateRequest",
        "TestConfigurationCreateRequest",
        "TestConfigurationUpdateRequest",
    ] {
        assert_eq!(
            schemas[name]["additionalProperties"],
            json!(false),
            "{name} rejects unknown fields"
        );
    }

    // These describe bodies a handler reads field by field, where the branch it
    // is on decides what it knows, so an extra field is ignored and the schema
    // must not claim a strictness the API does not enforce.
    for name in [
        "AttachmentUpload",
        "CompositionRequest",
        "DuplicateRequest",
        "DuplicateCaseRequest",
        "DuplicateRunRequest",
        "MilestoneProgress",
        "CoverageReport",
        "SuiteCoverage",
        "SummaryReport",
        "CompositionResponse",
        "CaseHistoryEntry",
        "Error",
        "CreateResponse",
        "MessageResponse",
        "UploadResponse",
    ] {
        assert!(
            schemas[name].get("additionalProperties").is_none(),
            "{name} ignores unknown fields"
        );
    }

    // The three response components publish the wire shapes the success paths
    // return, so a client can read the assigned id, the message, and the upload
    // metadata without guessing.
    assert_eq!(
        schemas["CreateResponse"]["required"],
        json!(["message", "id"])
    );
    assert_eq!(
        schemas["CreateResponse"]["properties"]["id"]["type"],
        json!("string")
    );
    assert_eq!(schemas["MessageResponse"]["required"], json!(["message"]));
    assert_eq!(
        schemas["UploadResponse"]["required"],
        json!(["message", "filename", "originalName", "size"])
    );
    assert_eq!(
        schemas["UploadResponse"]["properties"]["size"]["type"],
        json!("number")
    );

    // A client can switch exhaustively on the error code: the document's enum
    // is the full set the API can emit, and the `400` descriptions it declares
    // each name at least one of those codes.
    assert_eq!(
        schemas["Error"]["properties"]["error"]["properties"]["code"]["enum"],
        json!(ERROR_CODES)
    );

    // A case carries the attachments the upload route stores.
    assert_eq!(
        schemas["TestCase"]["properties"]["attachments"]["items"]["$ref"],
        "#/components/schemas/Attachment"
    );
    assert!(schemas["TestCase"]["properties"]["tags"].is_object());

    // A structured step carries the attachments the step-upload route stores;
    // a plain string step is the other arm of the same `oneOf`.
    assert_eq!(
        schemas["TestCase"]["properties"]["steps"]["items"]["oneOf"][1]["$ref"],
        "#/components/schemas/TestStep"
    );
    assert_eq!(
        schemas["TestStep"]["properties"]["attachments"]["items"]["$ref"],
        "#/components/schemas/StepAttachment"
    );

    // The recorded timestamp is Unix seconds rendered as a string, as this API
    // writes it — not an ISO-8601 date, so no format may be claimed.
    let timestamp = &schemas["TestCaseResult"]["properties"]["timestamp"];
    assert!(timestamp.get("format").is_none(), "no date-time format");
    assert!(
        timestamp["description"].is_string(),
        "the shape is documented"
    );

    // A run snapshot embeds a copy of the documents it recorded, so the schema
    // publishes the tags and configurations the list filters read.
    let test_run = &schemas["TestRun"];
    assert!(test_run["properties"]["tags"].is_object());
    assert_eq!(
        test_run["properties"]["configurations"]["items"]["$ref"],
        "#/components/schemas/TestConfiguration"
    );

    // The run read response is where that snapshot's shape is published.
    assert_eq!(
        document["paths"]["/test_runs/{id}"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/TestRun"
    );
}

#[tokio::test]
async fn openapi_operations_carry_stable_ids_and_resource_tags() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    // The document declares the resource families once, so Swagger UI and a
    // generated client can group by them.
    let declared: Vec<&str> = document["tags"]
        .as_array()
        .expect("document tags")
        .iter()
        .map(|tag| tag["name"].as_str().expect("tag name"))
        .collect();
    assert_eq!(
        declared,
        vec![
            "Service",
            "Projects",
            "TestSuites",
            "TestCases",
            "TestRuns",
            "Milestones",
            "Configurations",
            "Reports",
            "Auth",
        ],
        "the document declares one tag per resource family"
    );

    // Every operation carries a unique id a generated client can reference
    // stably, and exactly one tag drawn from the declared set.
    let mut ids = BTreeSet::new();
    for (label, operation) in documented_operations(&document) {
        let id = operation["operationId"]
            .as_str()
            .unwrap_or_else(|| panic!("{label} has no operationId"));
        assert!(
            id.chars().next().is_some_and(|c| c.is_ascii_alphabetic()),
            "{label} operationId is not a client-safe identifier: {id}"
        );
        assert!(
            ids.insert(id.to_owned()),
            "{label} repeats operationId {id}"
        );

        let tags = operation["tags"]
            .as_array()
            .unwrap_or_else(|| panic!("{label} has no tags"));
        assert_eq!(
            tags.len(),
            1,
            "{label} must carry exactly one tag: {tags:?}"
        );
        let tag = tags[0].as_str().expect("tag name");
        assert!(
            declared.contains(&tag),
            "{label} carries an undeclared tag: {tag}"
        );
    }
    assert_eq!(ids.len(), 91, "every documented operation is named");
}

#[tokio::test]
async fn openapi_inlines_the_duplicate_operations_and_drops_their_fragments() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    // The five duplicate routes are concrete path items, not `$ref`s into a
    // components extension object a strict OpenAPI 3.0 path-item resolver may
    // refuse to load.
    for path in [
        "/projects/{id}/duplicate",
        "/test_suites/{id}/duplicate",
        "/test_cases/{id}/duplicate",
        "/test_runs/{id}/duplicate",
        "/milestones/{id}/duplicate",
    ] {
        let item = document["paths"][path]
            .as_object()
            .unwrap_or_else(|| panic!("{path} is not a concrete path item"));
        assert!(item.get("$ref").is_none(), "{path} is still a reference");
        assert!(
            item["post"]["operationId"].is_string(),
            "{path} lost its operation"
        );
    }

    // Nothing is left under `components` that only the old references used.
    let fragments: Vec<&str> = document["components"]
        .as_object()
        .expect("components")
        .keys()
        .map(String::as_str)
        .filter(|name| name.starts_with("x-"))
        .collect();
    assert_eq!(fragments, Vec::<&str>::new(), "leftover extension objects");
}

#[tokio::test]
async fn openapi_servers_describe_the_deployment_with_variables() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    let servers = document["servers"].as_array().expect("servers");
    assert_eq!(servers.len(), 1, "one deployment template");
    let server = &servers[0];
    let url = server["url"].as_str().expect("server url");
    assert!(url.contains('{'), "the base path is not templated: {url}");

    // Every placeholder the url uses is a declared variable with a default, so
    // a generator that does not substitute still gets a usable base path.
    let variables = server["variables"].as_object().expect("server variables");
    for (name, variable) in variables {
        assert!(
            url.contains(&format!("{{{name}}}")),
            "variable {name} is not used by {url}"
        );
        assert!(
            variable["default"].is_string(),
            "variable {name} declares no default"
        );
    }
    assert!(
        variables.contains_key("host") && variables.contains_key("port"),
        "the deployment host and port are variables"
    );
}

/// The error codes the API can put in the `{ "error": { "code": ... } }`
/// envelope, as `openapi.json` spells them.
const ERROR_CODES: &[&str] = &[
    "invalid_id",
    "invalid_request",
    "invalid_status",
    "invalid_multipart",
    "missing_file",
    "not_found",
    "conflict",
    "storage_error",
    "lock_timeout",
    "missing_token",
    "invalid_token",
    "token_expired",
    "invalid_credentials",
    "invalid_refresh_token",
    "forbidden",
    "not_ready",
];

/// Every `$ref` the document contains, wherever it sits.
fn collect_references(value: &Value, into: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object {
                if key == "$ref"
                    && let Some(reference) = inner.as_str()
                {
                    into.push(reference.to_owned());
                }
                collect_references(inner, into);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_references(item, into);
            }
        }
        _ => {}
    }
}

/// Every operation the document describes, including those the path items and
/// the shared `x-` fragments hold, labelled by method and location.
fn documented_operations(document: &Value) -> Vec<(String, &Value)> {
    const METHODS: [&str; 4] = ["get", "put", "post", "delete"];

    let mut operations = Vec::new();
    for (path, item) in document["paths"].as_object().expect("paths object") {
        let item = dereference(document, item);
        for method in METHODS {
            if let Some(operation) = item.get(method) {
                operations.push((format!("{method} {path}"), operation));
            }
        }
    }
    for (name, fragment) in document["components"].as_object().expect("components") {
        if !name.starts_with("x-") {
            continue;
        }
        for method in METHODS {
            if let Some(operation) = fragment.get(method) {
                operations.push((format!("{method} {name}"), operation));
            }
        }
    }
    operations
}

/// Follows a local `$ref` one level, so a response or path item can be read
/// where it is defined.
fn dereference<'a>(document: &'a Value, node: &'a Value) -> &'a Value {
    let Some(pointer) = node.get("$ref").and_then(Value::as_str) else {
        return node;
    };
    let mut target = document;
    for segment in pointer.trim_start_matches("#/").split('/') {
        target = target
            .get(segment)
            .unwrap_or_else(|| panic!("unresolved reference: {pointer}"));
    }
    target
}

/// A request that only *claims* to exceed the router's limit, which is enough:
/// the limit is enforced from the declared `Content-Length`, without reading
/// the body.
fn oversized_request(uri: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::CONTENT_LENGTH,
            (api::MAX_BODY_BYTES + 1).to_string(),
        )
        .body(Body::empty())
        .expect("request")
}

/// Reads a stored JSON document from the volume for assertions on the layout.
fn read_json(path: &std::path::Path) -> serde_json::Value {
    let bytes = std::fs::read(path).expect("stored document");
    serde_json::from_slice(&bytes).expect("stored document is JSON")
}
