mod common;

use axum::http::StatusCode;
use common::{assert_error_envelope, delete, get, json_request, send_json, test_app};
use serde_json::json;

#[tokio::test]
async fn projects_support_the_full_crud_lifecycle() {
    let (_directory, app) = test_app();

    let (status, listing) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!([]));

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"projectId": "checkout.json", "name": "checkout", "testSuites": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], "checkout.json");

    let (status, listing) = send_json(&app, get("/projects")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, json!(["checkout.json"]));

    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["projectId"], "checkout.json");

    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/checkout.json",
            &json!({"name": "checkout-v2"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, updated) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(updated["name"], "checkout-v2");
    assert_eq!(updated["projectId"], "checkout.json");

    let (status, _) = send_json(&app, delete("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_supports_case_insensitive_substring_filtering() {
    let (_directory, app) = test_app();

    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "alpha"})),
    )
    .await;
    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "beta"})),
    )
    .await;
    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "alphabet"})),
    )
    .await;

    let (status, body) = send_json(&app, get("/projects?filter=ALPH")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(["alpha.json", "alphabet.json"]));

    let (status, body) = send_json(&app, get("/projects?filter=nonexistent")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn creating_a_project_requires_a_non_empty_name() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"description": "no name"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");

    let (status, body) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": ""})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "invalid_request");
}

#[tokio::test]
async fn duplicate_projects_are_rejected_with_conflict() {
    let (_directory, app) = test_app();
    let payload = json!({"name": "checkout"});

    let (status, _) = send_json(&app, json_request("POST", "/projects", &payload)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = send_json(&app, json_request("POST", "/projects", &payload)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "conflict");
}

#[tokio::test]
async fn a_duplicate_without_a_new_id_derives_an_addressable_identifier() {
    let (_directory, app) = test_app();

    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "checkout"})),
    )
    .await;

    let (status, duplicated) = send_json(
        &app,
        json_request("POST", "/projects/checkout.json/duplicate", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "duplicating: {duplicated}");
    let copy = duplicated["id"].as_str().expect("copy id");
    assert!(
        copy.starts_with("checkout-copy-"),
        "unexpected copy id {copy}"
    );
    assert!(copy.ends_with(".json"), "unexpected copy id {copy}");

    let (status, stored) = send_json(&app, get(&format!("/projects/{copy}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["projectId"], copy);
    assert_eq!(stored["name"], "checkout");
}

#[tokio::test]
async fn missing_projects_return_a_stable_error_envelope() {
    let (_directory, app) = test_app();

    for request in [
        get("/projects/missing.json"),
        delete("/projects/missing.json"),
        json_request("PUT", "/projects/missing.json", &json!({"name": "missing"})),
    ] {
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error_envelope(&body, "not_found");
    }
}

#[tokio::test]
async fn a_partial_update_keeps_the_fields_the_body_leaves_out() {
    let (_directory, app) = test_app();

    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({
                "projectId": "checkout.json",
                "name": "checkout",
                "description": "the shop",
                "tags": ["release"],
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // A body naming nothing keeps the whole stored document, so the project
    // still reads back afterwards.
    let (status, body) = send_json(
        &app,
        json_request("PUT", "/projects/checkout.json", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "updating: {body}");

    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["projectId"], "checkout.json");
    assert_eq!(stored["name"], "checkout");
    assert_eq!(stored["description"], "the shop");
    assert_eq!(stored["tags"], json!(["release"]));

    // A field the body carries is replaced; the others survive it.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/checkout.json",
            &json!({"name": "checkout-v2"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(stored["name"], "checkout-v2");
    assert_eq!(stored["projectId"], "checkout.json");
    assert_eq!(stored["description"], "the shop");
    assert_eq!(stored["tags"], json!(["release"]));

    // A `null` field counts as not supplied, so it can never drop a stored one.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/checkout.json",
            &json!({"description": null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(stored["description"], "the shop");
}

#[tokio::test]
async fn an_update_refuses_a_body_identifier_that_names_another_project() {
    let (_directory, app) = test_app();

    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "checkout"})),
    )
    .await;

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/checkout.json",
            &json!({"projectId": "other.json", "name": "renamed"}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "renaming a project: {body}"
    );
    assert_error_envelope(&body, "invalid_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("immutable")),
        "unexpected message: {body}"
    );

    // The refusal lands before the write, so the document is untouched.
    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["projectId"], "checkout.json");
    assert_eq!(stored["name"], "checkout");
}

#[tokio::test]
async fn an_update_accepts_a_body_identifier_that_restates_the_addressed_one() {
    let (_directory, app) = test_app();

    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "checkout"})),
    )
    .await;

    let (status, body) = send_json(
        &app,
        json_request(
            "PUT",
            "/projects/checkout.json",
            &json!({"projectId": "checkout.json", "name": "checkout-v2"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "restating the identity: {body}");

    let (_, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(stored["name"], "checkout-v2");
    assert_eq!(stored["projectId"], "checkout.json");
}

#[tokio::test]
async fn an_update_refuses_a_body_identifier_the_store_cannot_file() {
    let (_directory, app) = test_app();

    send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "checkout"})),
    )
    .await;

    for supplied in ["probe-moved", "team/copy.json", "", ".", ".."] {
        let (status, body) = send_json(
            &app,
            json_request(
                "PUT",
                "/projects/checkout.json",
                &json!({"projectId": supplied}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "supplying {supplied:?}: {body}"
        );
        assert_error_envelope(&body, "invalid_id");
    }

    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["projectId"], "checkout.json");
}

#[tokio::test]
async fn a_supplied_project_id_names_the_new_project() {
    let (_directory, app) = test_app();

    for supplied in ["probe-bare", "team/copy.json", "", "."] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                "/projects",
                &json!({"projectId": supplied, "name": "checkout"}),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "creating with {supplied:?}: {body}"
        );
        assert_error_envelope(&body, "invalid_request");
    }

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"projectId": "checkout.json", "name": "shop"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {created}");
    assert_eq!(created["id"], "checkout.json");

    let (status, stored) = send_json(&app, get("/projects/checkout.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["name"], "shop");
    assert_eq!(stored["projectId"], "checkout.json");

    let (_, listing) = send_json(&app, get("/projects")).await;
    assert_eq!(listing, json!(["checkout.json"]));

    // A project the body does not name is still filed under its name.
    let (status, derived) = send_json(
        &app,
        json_request("POST", "/projects", &json!({"name": "warehouse"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "creating: {derived}");
    assert_eq!(derived["id"], "warehouse.json");
}
