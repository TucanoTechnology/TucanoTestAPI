mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{
    app_at, assert_error_envelope, content_type, get, json_request, raw_json_request, send,
    send_full, send_json, test_app,
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
    // API records for a run created from a name alone.
    let (status, created) = send_json(
        &app,
        json_request("POST", "/test_runs", &json!({"name": "nightly"})),
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
}

#[tokio::test]
async fn duplicate_routes_report_an_unusable_identifier_as_their_own_description_says() {
    let (_directory, app) = test_app();

    // These three read the identifier as a body field before they read the
    // path, so an unusable path identifier reaches them as a bad request.
    for uri in [
        "/projects/nope/duplicate",
        "/test_runs/nope/duplicate",
        "/milestones/nope/duplicate",
    ] {
        let (status, body) = send_json(&app, json_request("POST", uri, &json!({}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        assert_error_envelope(&body, "invalid_request");
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

    let operations = documented_operations(&document);
    assert!(operations.len() > 20, "the document lost its operations");

    for (label, operation) in &operations {
        let responses = operation["responses"]
            .as_object()
            .unwrap_or_else(|| panic!("{label} has no responses"));

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

    // The result body is the run's own record, not the stored document.
    assert_eq!(
        document["paths"]["/test_runs/{id}/results"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/TestResultRequest"
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
}

#[tokio::test]
async fn openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
    let schemas = document["components"]["schemas"]
        .as_object()
        .expect("schemas object");

    // These describe documents that deserialise into a model carrying
    // `deny_unknown_fields`, so an extra field is a 400 and the schema says so.
    for name in [
        "Attachment",
        "Project",
        "TestSuite",
        "TestCase",
        "TestStep",
        "TestCaseResult",
        "Milestone",
        "TestConfiguration",
        "TestRun",
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
        "CompositionResponse",
        "TestResultRequest",
        "Error",
    ] {
        assert!(
            schemas[name].get("additionalProperties").is_none(),
            "{name} ignores unknown fields"
        );
    }

    // A case carries the attachments the upload route stores.
    assert_eq!(
        schemas["TestCase"]["properties"]["attachments"]["items"]["$ref"],
        "#/components/schemas/Attachment"
    );
    assert!(schemas["TestCase"]["properties"]["tags"].is_object());

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

/// The error codes the API can put in the `{ "error": { "code": ... } }`
/// envelope, as `openapi.json` spells them.
const ERROR_CODES: &[&str] = &[
    "invalid_id",
    "invalid_request",
    "invalid_status",
    "invalid_multipart",
    "missing_file",
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
