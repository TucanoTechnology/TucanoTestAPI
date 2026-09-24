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

/// A scalar field carrying the wrong JSON type is rejected before anything is
/// persisted. `tags` is an accepted key, so the rejection comes from the value:
/// the model declares `Option<Vec<String>>` and a bare string contradicts it.
/// #121 tightened this — before it, `validate_payload` type-checked only the
/// nested collections and the wrong-typed value was stored verbatim, which left
/// the documented `?tags=` filter silently unable to match the document.
#[tokio::test]
async fn a_wrong_typed_scalar_is_rejected_with_the_field_named() {
    let (_directory, app) = test_app();

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            "/projects",
            &json!({"name": "checkout", "tags": "smoke"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    common::assert_error_envelope(&body, "invalid_request");
    assert_eq!(body["error"]["message"], json!("Field `tags` is invalid"));

    assert_eq!(
        listed_ids(&app, "/projects").await,
        Vec::<String>::new(),
        "a rejected body must not be stored"
    );
}

#[tokio::test]
async fn the_tags_filter_works_on_runs() {
    let (_directory, app) = test_app();
    let home = common::fixture_home(&app).await;

    let nightly = create_tagged(
        &app,
        &format!("/projects/{home}/test_runs"),
        json!({"name": "nightly", "tags": ["ci"]}),
    )
    .await;
    let manual = create_tagged(
        &app,
        &format!("/projects/{home}/test_runs"),
        json!({"name": "manual"}),
    )
    .await;

    let (status, stored) = send_json(&app, get(&format!("/test_runs/{nightly}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["tags"], json!(["ci"]));

    assert_eq!(listed_ids(&app, "/test_runs?tags=ci").await, vec![nightly]);

    let mut all = ["nightly.json".to_owned(), manual];
    all.sort();
    assert_eq!(listed_ids(&app, "/test_runs").await, all);
}

#[tokio::test]
async fn the_tags_filter_works_on_a_projects_suites() {
    let (_directory, app) = test_app();

    let project = create_tagged(&app, "/projects", json!({"name": "checkout"})).await;
    let listing = format!("/projects/{project}/test_suites");

    let smoke = create_tagged(
        &app,
        &listing,
        json!({"name": "smoke-suites", "tags": ["Smoke"]}),
    )
    .await;
    let regressions = create_tagged(
        &app,
        &listing,
        json!({"name": "regression-suites", "tags": ["regression"]}),
    )
    .await;
    let _untagged = create_tagged(&app, &listing, json!({"name": "untagged-suites"})).await;

    // The filter is additive: without it the project still lists everything it
    // owns, untagged suites included.
    let mut all = vec![
        smoke.clone(),
        regressions.clone(),
        "untagged-suites.json".to_owned(),
    ];
    all.sort();
    assert_eq!(listed_ids(&app, &listing).await, all);

    for query in ["Smoke", "smoke", "SMOKE", "%20smoke%20", "smoke,unused"] {
        assert_eq!(
            listed_ids(&app, &format!("{listing}?tags={query}")).await,
            vec![smoke.clone()],
            "`?tags={query}` should match the stored tag, case-insensitively and trimmed"
        );
    }

    let mut both = vec![smoke.clone(), regressions];
    both.sort();
    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=smoke,regression")).await,
        both,
        "a suite matching any requested tag is listed"
    );

    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=unused")).await,
        Vec::<String>::new(),
        "a filter that matches nothing answers an empty listing, not `400`"
    );
    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=smoke&filter=smoke")).await,
        vec![smoke],
        "`?tags=` composes with `?filter=`"
    );
}

#[tokio::test]
async fn the_tags_filter_works_on_a_projects_cases() {
    let (_directory, app) = test_app();

    let project = create_tagged(&app, "/projects", json!({"name": "checkout"})).await;
    let listing = format!("/projects/{project}/test_cases");

    let smoke = create_tagged(
        &app,
        &listing,
        json!({
            "testCaseId": "TC-SMOKE",
            "title": "Login",
            "expectedResult": "Stored",
            "tags": ["Smoke"],
        }),
    )
    .await;
    let slow = create_tagged(
        &app,
        &listing,
        json!({
            "testCaseId": "TC-SLOW",
            "title": "Search",
            "expectedResult": "Stored",
            "tags": ["slow"],
        }),
    )
    .await;
    let _untagged = create_tagged(
        &app,
        &listing,
        json!({"testCaseId": "TC-PLAIN", "title": "Logout", "expectedResult": "Stored"}),
    )
    .await;

    let mut all = vec![smoke.clone(), slow.clone(), "TC-PLAIN".to_owned()];
    all.sort();
    assert_eq!(listed_ids(&app, &listing).await, all);

    for query in ["Smoke", "smoke", "SMOKE", "%20smoke%20", "smoke,unused"] {
        assert_eq!(
            listed_ids(&app, &format!("{listing}?tags={query}")).await,
            vec![smoke.clone()],
            "`?tags={query}` should match the stored tag, case-insensitively and trimmed"
        );
    }

    let mut both = vec![smoke, slow.clone()];
    both.sort();
    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=smoke,slow")).await,
        both,
        "a case matching any requested tag is listed"
    );

    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=unused")).await,
        Vec::<String>::new(),
        "a filter that matches nothing answers an empty listing, not `400`"
    );
    assert_eq!(
        listed_ids(&app, &format!("{listing}?filter=slow&tags=slow")).await,
        vec![slow],
        "`?tags=` composes with `?filter=`"
    );
}

#[tokio::test]
async fn the_tags_filter_works_on_a_projects_runs() {
    let (_directory, app) = test_app();
    let home = common::fixture_home(&app).await;
    let listing = format!("/projects/{home}/test_runs");

    let nightly = create_tagged(&app, &listing, json!({"name": "nightly", "tags": ["CI"]})).await;
    let manual = create_tagged(&app, &listing, json!({"name": "manual"})).await;

    let mut all = vec![nightly.clone(), manual];
    all.sort();
    assert_eq!(listed_ids(&app, &listing).await, all);

    for query in ["CI", "ci", "%20ci%20", "ci,unused"] {
        assert_eq!(
            listed_ids(&app, &format!("{listing}?tags={query}")).await,
            vec![nightly.clone()],
            "`?tags={query}` should match the stored tag, case-insensitively and trimmed"
        );
    }

    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=unused")).await,
        Vec::<String>::new(),
        "a filter that matches nothing answers an empty listing, not `400`"
    );
    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=ci&filter=night")).await,
        vec![nightly],
        "`?tags=` composes with `?filter=`"
    );
}

/// A project-scoped tag filter judges the occurrence the named project holds.
///
/// A suite identifier is unique inside its project, but two projects may each
/// hold a suite named `shared` with different tags. The listing answers from the
/// project the path names — the occurrence the project-scoped read and delete
/// address — rather than from the first occurrence a global lookup resolves, so
/// one project's labels never leak into another project's listing.
#[tokio::test]
async fn a_projects_tags_filter_judges_that_projects_occurrence() {
    let (_directory, app) = test_app();

    let alpha = create_tagged(&app, "/projects", json!({"name": "alpha"})).await;
    let beta = create_tagged(&app, "/projects", json!({"name": "beta"})).await;

    // The untagged occurrence is created first, so a global first-occurrence
    // lookup would resolve the untagged document.
    create_tagged(
        &app,
        &format!("/projects/{beta}/test_suites"),
        json!({"name": "shared"}),
    )
    .await;
    create_tagged(
        &app,
        &format!("/projects/{alpha}/test_suites"),
        json!({"name": "shared", "tags": ["smoke"]}),
    )
    .await;

    assert_eq!(
        listed_ids(&app, &format!("/projects/{alpha}/test_suites?tags=smoke")).await,
        vec!["shared.json".to_owned()],
        "alpha's occurrence carries the tag"
    );
    assert_eq!(
        listed_ids(&app, &format!("/projects/{beta}/test_suites?tags=smoke")).await,
        Vec::<String>::new(),
        "beta's occurrence carries no tag, so it never matches"
    );
}

/// `?configuration=` narrows a project's runs to those linking the named
/// configuration, and the tag filter narrows that same result rather than
/// replacing it.
#[tokio::test]
async fn a_projects_run_tags_filter_composes_with_the_configuration_filter() {
    let (_directory, app) = test_app();
    let home = common::fixture_home(&app).await;
    let listing = format!("/projects/{home}/test_runs");

    let nightly = create_tagged(&app, &listing, json!({"name": "nightly", "tags": ["ci"]})).await;
    let _smoke = create_tagged(&app, &listing, json!({"name": "smoke", "tags": ["ci"]})).await;
    let chrome = common::create_named(&app, "/configurations", "chrome-linux").await;

    let (status, body) = send_json(
        &app,
        json_request(
            "POST",
            &format!("/test_runs/{nightly}/configurations"),
            &json!({"configId": chrome.clone()}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "linking {chrome} to {nightly}: {body}"
    );

    assert_eq!(
        listed_ids(&app, &format!("{listing}?configuration={chrome}")).await,
        vec![nightly.clone()],
        "only the linked run carries the configuration"
    );
    assert_eq!(
        listed_ids(&app, &format!("{listing}?tags=ci&configuration={chrome}")).await,
        vec![nightly.clone()],
        "both parameters narrow the same result"
    );
    assert_eq!(
        listed_ids(
            &app,
            &format!("{listing}?tags=smoke&configuration={chrome}")
        )
        .await,
        Vec::<String>::new(),
        "the tag filter narrows the configuration result rather than widening it"
    );
    assert_eq!(
        listed_ids(&app, &format!("{listing}?configuration=missing.json")).await,
        Vec::<String>::new(),
        "an unnamed configuration answers an empty listing, not `400`"
    );
}

/// The published document advertises `?tags=` only where a tag can be stored.
///
/// `?tags=` walks the stored document's `tags` array, so on a resource whose
/// model has no `tags` field — and whose writes therefore refuse one — the
/// filter is documented but can only ever answer an empty listing. It is
/// published on every list operation whose resource can carry tags: the global
/// `GET /projects`, and the three project-scoped listings — the suites, cases
/// and runs a project owns. The flat collection paths that serve the same
/// resources — `GET /test_suites`, `GET /test_cases`, `GET /test_runs` — are
/// retired and carry no contract entry (`api::UNDOCUMENTED_ROUTES`).
///
/// Issue #293 is what added the parameter to the three project-scoped listings;
/// before it the parent-scoped collections published no query parameters at all.
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
        vec![
            "Project",
            "ProjectCreateRequest",
            "ProjectUpdateRequest",
            "TestCase",
            "TestCaseUpdateRequest",
            "TestRun",
            "TestRunCreateRequest",
            "TestRunUpdateRequest",
            "TestSuite",
            "TestSuiteUpdateRequest",
        ],
        "the schemas publishing a tags array are the models that store one and the write bodies that accept one"
    );

    // Every operation that references the parameter component, by method and
    // path.
    let publishes = |reference: &str| -> Vec<String> {
        let mut operations: Vec<String> = Vec::new();
        for (path, item) in document["paths"].as_object().expect("paths") {
            for (method, operation) in item.as_object().expect("operation") {
                let Some(parameters) = operation.get("parameters").and_then(Value::as_array) else {
                    continue;
                };
                if parameters
                    .iter()
                    .any(|parameter| parameter["$ref"] == json!(reference))
                {
                    operations.push(format!("{} {path}", method.to_uppercase()));
                }
            }
        }
        operations.sort();
        operations
    };

    assert_eq!(
        publishes("#/components/parameters/tags"),
        vec![
            "GET /projects",
            "GET /projects/{id}/test_cases",
            "GET /projects/{id}/test_runs",
            "GET /projects/{id}/test_suites",
        ],
        "`?tags=` is published only on the list operations of a taggable resource"
    );
    assert_eq!(
        publishes("#/components/parameters/filter"),
        vec![
            "GET /projects",
            "GET /projects/{id}/test_cases",
            "GET /projects/{id}/test_runs",
            "GET /projects/{id}/test_suites",
        ],
        "`?filter=` narrows the same listings the tag filter does"
    );
    assert_eq!(
        publishes("#/components/parameters/configuration"),
        vec!["GET /projects/{id}/test_runs"],
        "`?configuration=` is published only where a resource can link one"
    );

    // The two resources the parameter is withheld from cannot store a tag at
    // all, so advertising the filter there would document a false promise.
    let (_directory, app) = test_app();
    let project = common::create_project(&app, "checkout").await;
    for collection in ["milestones", "configurations"] {
        let (status, body) = send_json(
            &app,
            json_request(
                "POST",
                &format!("/projects/{project}/{collection}"),
                &json!({"name": "x", "tags": ["smoke"]}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{collection}: {body}");
        common::assert_error_envelope(&body, "invalid_request");

        let (status, listed) = send_json(&app, get(&format!("/{collection}?tags=smoke"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            listed,
            json!([]),
            "`?tags=` on /{collection} can only ever answer an empty listing"
        );
    }
}
