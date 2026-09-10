//! Tags: the optional `tags` array on projects, suites, cases and runs, and the
//! shared `?tags=` OR filter over the list endpoints.
//!
//! The filter is documented in `openapi.json` through the `tags` parameter
//! component; these tests keep that documented promise honest, including the
//! parts a client is most likely to rely on: case-insensitive matching, "at
//! least one of the requested tags" semantics, and the fact that a resource
//! without a `tags` array never matches.

mod common;

use axum::Router;
use axum::http::StatusCode;
use common::{get, json_request, send_json, test_app};
use serde_json::{Value, json};

/// Creates a resource and returns the generated identifier.
async fn create_tagged(app: &Router, uri: &str, body: Value) -> String {
    let (status, created) = send_json(app, json_request("POST", uri, &body)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "creating {body} at {uri}: {created}"
    );
    created["id"].as_str().expect("created id").to_owned()
}

/// The listing endpoint's answer, sorted so the assertions do not depend on
/// storage ordering.
async fn listed_ids(app: &Router, uri: &str) -> Vec<String> {
    let (status, listing) = send_json(app, get(uri)).await;
    assert_eq!(status, StatusCode::OK, "listing {uri}: {listing}");
    let mut ids: Vec<String> = listing
        .as_array()
        .unwrap_or_else(|| panic!("{uri} did not answer with an array: {listing}"))
        .iter()
        .map(|id| id.as_str().expect("id string").to_owned())
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn project_tags_round_trip_through_create_read_and_update() {
    let (_directory, app) = test_app();

    let id = create_tagged(
        &app,
        "/projects",
        json!({"name": "checkout", "tags": ["Smoke", "regression"]}),
    )
    .await;

    let (status, stored) = send_json(&app, get(&format!("/projects/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["tags"], json!(["Smoke", "regression"]));

    // A partial update replaces the tags it names and leaves the rest alone.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            &format!("/projects/{id}"),
            &json!({"tags": ["release"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get(&format!("/projects/{id}"))).await;
    assert_eq!(stored["tags"], json!(["release"]));
    assert_eq!(
        stored["name"], "checkout",
        "the update kept the omitted name"
    );

    // An explicit empty array clears the tags without dropping the document.
    let (status, _) = send_json(
        &app,
        json_request("PUT", &format!("/projects/{id}"), &json!({"tags": []})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, stored) = send_json(&app, get(&format!("/projects/{id}"))).await;
    assert_eq!(stored["tags"], json!([]));
}

#[tokio::test]
async fn the_tags_filter_is_case_insensitive_and_trims_whitespace() {
    let (_directory, app) = test_app();

    let id = create_tagged(
        &app,
        "/projects",
        json!({"name": "alpha", "tags": ["Smoke"]}),
    )
    .await;

    for query in ["Smoke", "smoke", "SMOKE", "  smoke  ", "smoke,unused"] {
        let uri = format!("/projects?tags={}", query.replace(' ', "%20"));
        assert_eq!(
            listed_ids(&app, &uri).await,
            vec![id.clone()],
            "`?tags={query}` should match the stored tag"
        );
    }
}

#[tokio::test]
async fn the_tags_filter_matches_when_the_resource_carries_any_requested_tag() {
    let (_directory, app) = test_app();

    let smoke = create_tagged(
        &app,
        "/projects",
        json!({"name": "smoke", "tags": ["smoke"]}),
    )
    .await;
    let regressions = create_tagged(
        &app,
        "/projects",
        json!({"name": "regressions", "tags": ["regression"]}),
    )
    .await;
    let both = create_tagged(
        &app,
        "/projects",
        json!({"name": "both", "tags": ["smoke", "regression"]}),
    )
    .await;

    let mut expected = vec![smoke.clone(), both.clone()];
    expected.sort();
    assert_eq!(listed_ids(&app, "/projects?tags=smoke").await, expected);

    let mut every = vec![smoke, regressions, both];
    every.sort();
    assert_eq!(
        listed_ids(&app, "/projects?tags=smoke,regression").await,
        every
    );

    assert_eq!(
        listed_ids(&app, "/projects?tags=unused").await,
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn a_resource_without_tags_never_matches_the_tags_filter() {
    let (_directory, app) = test_app();

    let untagged = create_tagged(&app, "/projects", json!({"name": "untagged"})).await;
    let tagged = create_tagged(
        &app,
        "/projects",
        json!({"name": "tagged", "tags": ["smoke"]}),
    )
    .await;

    // The untagged document is still listed, and still readable, without a
    // `tags` array — the field is optional and nothing rewrites it.
    let mut all = vec![untagged.clone(), tagged];
    all.sort();
    assert_eq!(listed_ids(&app, "/projects").await, all);

    let (status, stored) = send_json(&app, get(&format!("/projects/{untagged}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        stored["tags"].as_array().is_none(),
        "a resource stored without tags must not gain a tags array: {stored}"
    );

    assert_eq!(
        listed_ids(&app, "/projects?tags=smoke").await,
        vec!["tagged.json".to_owned()]
    );
}

#[tokio::test]
async fn the_tags_filter_works_on_suites() {
    let (_directory, app) = test_app();

    let project = create_tagged(&app, "/projects", json!({"name": "checkout"})).await;
    let smoke = create_tagged(
        &app,
        &format!("/projects/{project}/test_suites"),
        json!({"name": "smoke-suites", "tags": ["smoke"]}),
    )
    .await;
    let _regressions = create_tagged(
        &app,
        &format!("/projects/{project}/test_suites"),
        json!({"name": "regression-suites", "tags": ["regression"]}),
    )
    .await;

    let (status, stored) = send_json(&app, get(&format!("/test_suites/{smoke}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["tags"], json!(["smoke"]));

    assert_eq!(
        listed_ids(&app, "/test_suites?tags=smoke").await,
        vec![smoke]
    );
    assert_eq!(
        listed_ids(&app, "/test_suites?tags=regression").await,
        vec!["regression-suites.json".to_owned()]
    );
}

#[tokio::test]
async fn the_tags_filter_works_on_test_cases() {
    let (_directory, app) = test_app();

    let project = create_tagged(&app, "/projects", json!({"name": "checkout"})).await;
    let smoke = create_tagged(
        &app,
        &format!("/projects/{project}/test_cases"),
        json!({
            "testCaseId": "TC-SMOKE",
            "title": "Login",
            "expectedResult": "Stored",
            "tags": ["Smoke"],
        }),
    )
    .await;
    let _slow = create_tagged(
        &app,
        &format!("/projects/{project}/test_cases"),
        json!({
            "testCaseId": "TC-SLOW",
            "title": "Search",
            "expectedResult": "Stored",
            "tags": ["slow"],
        }),
    )
    .await;

    let (status, stored) = send_json(&app, get(&format!("/test_cases/{smoke}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["tags"], json!(["Smoke"]));

    assert_eq!(
        listed_ids(&app, "/test_cases?tags=smoke").await,
        vec![smoke]
    );
    // A case id is the `testCaseId` verbatim, with no `.json` suffix: the
    // suffix comes from `derive_create_id`, which only runs for the resources
    // that generate their own identifier.
    assert_eq!(
        listed_ids(&app, "/test_cases?tags=slow").await,
        vec!["TC-SLOW".to_owned()]
    );
}

#[tokio::test]
async fn tags_do_not_weaken_unknown_field_rejection() {
    let (_directory, app) = test_app();

    // `tags` is an accepted key, so the rejection has to come from the field
    // next to it — the models keep `deny_unknown_fields`.
    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"name": "checkout", "tags": ["smoke"], "unknownField": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    common::assert_error_envelope(&body, "invalid_request");

    assert_eq!(
        listed_ids(&app, "/projects").await,
        Vec::<String>::new(),
        "a rejected body must not be stored"
    );
}

/// `validate_payload` type-checks only the nested collections, so a scalar
/// field carrying the wrong JSON type is stored exactly as sent. This pins
/// that behaviour as deliberate rather than accidental; #121 tightens the
/// validation and replaces this test with one that expects a rejection.
#[tokio::test]
async fn a_wrong_typed_scalar_is_currently_stored_verbatim() {
    let (_directory, app) = test_app();

    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"name": "checkout", "tags": "smoke"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let id = created["id"].as_str().expect("created id");
    let (status, stored) = send_json(&app, get(&format!("/projects/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        stored["tags"],
        json!("smoke"),
        "the wrong-typed value survives a round trip (#121)"
    );

    // The documented `?tags=` filter reads the raw array, so the stored string
    // can never match it: the resource is silently invisible to the filter.
    assert_eq!(
        listed_ids(&app, "/projects?tags=smoke").await,
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn the_tags_filter_works_on_runs() {
    let (_directory, app) = test_app();

    let nightly = create_tagged(
        &app,
        "/test_runs",
        json!({"name": "nightly", "tags": ["ci"]}),
    )
    .await;
    let manual = create_tagged(&app, "/test_runs", json!({"name": "manual"})).await;

    let (status, stored) = send_json(&app, get(&format!("/test_runs/{nightly}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["tags"], json!(["ci"]));

    assert_eq!(listed_ids(&app, "/test_runs?tags=ci").await, vec![nightly]);

    let mut all = ["nightly.json".to_owned(), manual];
    all.sort();
    assert_eq!(listed_ids(&app, "/test_runs").await, all);
}

/// The published document advertises `?tags=` only where a tag can be stored.
///
/// `?tags=` walks the stored document's `tags` array, so on a resource whose
/// model has no `tags` field — and whose writes therefore refuse one — the
/// filter is documented but can only ever answer an empty listing. It is
/// published on the two list operations whose resource can carry tags;
/// `GET /test_suites` and `GET /test_cases` filter by tags as well but are
/// retired from the document as a whole (`api::UNDOCUMENTED_ROUTES`).
#[tokio::test]
async fn the_tags_parameter_is_published_only_where_a_tag_can_be_stored() {
    let document: Value =
        serde_json::from_str(include_str!("../openapi.json")).expect("openapi.json parses");

    let mut carries_tags: Vec<&str> = document["components"]["schemas"]
        .as_object()
        .expect("schemas")
        .iter()
        .filter(|(_, schema)| schema["properties"]["tags"]["items"]["type"] == json!("string"))
        .map(|(name, _)| name.as_str())
        .collect();
    carries_tags.sort();
    assert_eq!(
        carries_tags,
        vec!["Project", "TestCase", "TestRun", "TestSuite"],
        "the schemas publishing a tags array are the models that store one"
    );

    let mut documented: Vec<String> = Vec::new();
    for (path, item) in document["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("operation") {
            let Some(parameters) = operation.get("parameters").and_then(Value::as_array) else {
                continue;
            };
            if parameters
                .iter()
                .any(|parameter| parameter["$ref"] == json!("#/components/parameters/tags"))
            {
                documented.push(format!("{} {path}", method.to_uppercase()));
            }
        }
    }
    documented.sort();
    assert_eq!(
        documented,
        vec!["GET /projects", "GET /test_runs"],
        "`?tags=` is published only on the list operations of a taggable resource"
    );

    // The two resources the parameter is withheld from cannot store a tag at
    // all, so advertising the filter there would document a false promise.
    let (_directory, app) = test_app();
    for uri in ["/milestones", "/configurations"] {
        let (status, body) = send_json(
            &app,
            json_request("POST", uri, &json!({"name": "x", "tags": ["smoke"]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        common::assert_error_envelope(&body, "invalid_request");

        let (status, listed) = send_json(&app, get(&format!("{uri}?tags=smoke"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            listed,
            json!([]),
            "`?tags=` on {uri} can only ever answer an empty listing"
        );
    }
}
