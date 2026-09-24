//! The release and environment listings the GUI's context bar reads.
//!
//! `GET /releases` and `GET /environments` answer the distinct `name` of every
//! milestone, respectively test configuration, held by the projects the caller
//! reaches — sorted byte-wise so the client can render the list as it arrives,
//! and filtered rather than refused when the caller reaches only some projects.
//! Nothing on disk carries these lists: they are derived from the documents the
//! milestone and configuration stores already hold, so a rename in a project
//! changes what the next read answers without a second write.
//!
//! An installation that holds no milestone or no configuration answers `[]` at
//! `200` — never `404` — because the context bar reads this before it knows
//! whether the installation has a release at all.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::{documented_operations, get, send_json, test_app};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::Path;
use tower::ServiceExt;
use tucano_test::auth::{AuthConfig, AuthStore, Role, User, hash_password};
use tucano_test::{api, repository::FileRepository};

const SECRET: &[u8] = b"an-integration-test-secret-of-32-plus!";
const PASSWORD: &str = "correct horse battery staple";
const USER_ID: &str = "account-1";
const USERNAME: &str = "alice";
const NOW: u64 = 1_700_000_000;

/// Enforcement on, so the listings are read the way the GUI reads them: with a
/// caller whose grants decide which projects contribute.
fn config() -> AuthConfig {
    AuthConfig {
        required: true,
        jwt_secret: Some(SECRET.to_vec()),
        access_ttl: std::time::Duration::from_secs(900),
        refresh_ttl: std::time::Duration::from_secs(1_209_600),
        bootstrap_username: None,
        bootstrap_password: None,
    }
}

/// The same deployment with enforcement off, for seeding the tree the listings
/// are derived from. Every guard early-returns, so nothing is written to the
/// auth tree and the same directory can be reopened with enforcement on
/// afterwards.
fn config_without_enforcement() -> AuthConfig {
    AuthConfig {
        required: false,
        ..config()
    }
}

fn account() -> User {
    User {
        id: USER_ID.to_string(),
        username: USERNAME.to_string(),
        password_hash: hash_password(PASSWORD).expect("hash the password"),
        system_admin: false,
        created_at: NOW,
        refresh_tokens: Vec::new(),
    }
}

/// A router that enforces authentication, backed by an isolated data directory
/// holding one account and the grants named. The caller keeps the `TempDir`.
fn enforcing_app_for(directory: &Path, grants: &[(&str, Role)]) -> Router {
    let store = AuthStore::new(directory).expect("auth store");
    store.insert_user(&account()).expect("insert the account");
    for (project, role) in grants {
        store
            .set_role(project, USER_ID, *role)
            .expect("grant the role");
    }
    let repository = FileRepository::new(directory).expect("repository");
    common::with_probe(api::router(
        repository,
        api::auth::AuthState::new(store, config()),
    ))
}

fn seeding_app(directory: &Path) -> Router {
    let store = AuthStore::new(directory).expect("auth store");
    let repository = FileRepository::new(directory).expect("repository");
    common::with_probe(api::router(
        repository,
        api::auth::AuthState::new(store, config_without_enforcement()),
    ))
}

fn request(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("request")
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Signs the seeded account in and returns its access token.
async fn sign_in_token(app: &Router) -> String {
    let (status, _, session) = send(
        app,
        request(
            "POST",
            "/auth/login",
            None,
            Some(json!({ "username": USERNAME, "password": PASSWORD })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "sign in: {session}");
    session["accessToken"]
        .as_str()
        .expect("access token")
        .to_owned()
}

/// Drives a request an authorised caller is entitled to and returns its body.
///
/// The listings are reads every caller may make: one that reaches some
/// projects gets their names, one that reaches none gets the empty array. A
/// refusal here would mean the read became a permission decision rather than a
/// filtered listing.
async fn call_ok(
    app: &Router,
    token: Option<&str>,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> Value {
    let (status, _, answer) = send(app, request(method, uri, token, body)).await;
    assert!(
        status.is_success(),
        "{method} {uri} answered {status}: {answer}"
    );
    answer
}

fn id_of(body: &Value) -> String {
    body["id"].as_str().expect("created id").to_owned()
}

async fn seed_project(app: &Router, name: &str) -> String {
    id_of(
        &call_ok(
            app,
            None,
            "POST",
            "/projects",
            Some(json!({ "name": name })),
        )
        .await,
    )
}

async fn seed_milestone(app: &Router, project: &str, name: &str) -> String {
    id_of(
        &call_ok(
            app,
            None,
            "POST",
            &format!("/projects/{project}/milestones"),
            Some(json!({ "name": name })),
        )
        .await,
    )
}

async fn seed_configuration(app: &Router, project: &str, name: &str) -> String {
    id_of(
        &call_ok(
            app,
            None,
            "POST",
            &format!("/projects/{project}/configurations"),
            Some(json!({ "name": name })),
        )
        .await,
    )
}

/// The releases are the distinct milestone names of the projects the caller
/// reaches, sorted byte-wise: a name two projects repeat is reported once, a
/// name only an unreachable project holds is not reported at all, and the
/// caller is answered rather than refused either way.
#[tokio::test]
async fn release_names_are_distinct_sorted_and_scoped_to_the_caller() {
    // Nothing is installed, so the derivation walks an empty tree: the listing
    // the context bar reads before any release exists is the empty array.
    let (_directory, app) = test_app();
    let (status, releases) = send_json(&app, get("/releases")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(releases, json!([]));
    let (status, environments) = send_json(&app, get("/environments")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(environments, json!([]));

    let directory = tempfile::tempdir().expect("temp dir");
    let seeding = seeding_app(directory.path());
    let alpha = seed_project(&seeding, "alpha").await;
    let beta = seed_project(&seeding, "beta").await;
    let gamma = seed_project(&seeding, "gamma").await;

    // Stored out of order on purpose, and `v1.0` twice across two projects: the
    // listing sorts byte-wise, so `v0.9` precedes the `v1.0` stored before it,
    // and the repeated name is reported once.
    seed_milestone(&seeding, &alpha, "v1.0").await;
    seed_milestone(&seeding, &alpha, "v0.9").await;
    seed_milestone(&seeding, &beta, "v1.0").await;
    seed_milestone(&seeding, &beta, "v2.0").await;
    seed_milestone(&seeding, &gamma, "v3.0").await;

    // The caller reaches alpha and beta, not gamma: gamma's `v3.0` contributes
    // nothing rather than turning the read into a refusal.
    let app = enforcing_app_for(
        directory.path(),
        &[
            (alpha.as_str(), Role::Viewer),
            (beta.as_str(), Role::Viewer),
        ],
    );
    let token = sign_in_token(&app).await;
    let releases = call_ok(&app, Some(&token), "GET", "/releases", None).await;
    assert_eq!(releases, json!(["v0.9", "v1.0", "v2.0"]));
}

/// The environments are the distinct configuration names of the projects the
/// caller reaches, under the same dedup, sort and reachability rules the
/// releases answer with.
#[tokio::test]
async fn environment_names_are_distinct_sorted_and_scoped_to_the_caller() {
    // An installation that holds no configuration answers the empty array, the
    // same way one that holds no milestone does.
    let (_directory, app) = test_app();
    let (status, environments) = send_json(&app, get("/environments")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(environments, json!([]));
    let (status, releases) = send_json(&app, get("/releases")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(releases, json!([]));

    let directory = tempfile::tempdir().expect("temp dir");
    let seeding = seeding_app(directory.path());
    let alpha = seed_project(&seeding, "alpha").await;
    let beta = seed_project(&seeding, "beta").await;
    let gamma = seed_project(&seeding, "gamma").await;

    // `firefox-linux` is stored before `chrome-linux`, and `chrome-linux` is
    // repeated across two projects, so one read proves the sort and the dedup
    // together.
    seed_configuration(&seeding, &alpha, "firefox-linux").await;
    seed_configuration(&seeding, &alpha, "chrome-linux").await;
    seed_configuration(&seeding, &beta, "chrome-linux").await;
    seed_configuration(&seeding, &beta, "webkit-linux").await;
    seed_configuration(&seeding, &gamma, "safari-macos").await;

    let app = enforcing_app_for(
        directory.path(),
        &[
            (alpha.as_str(), Role::Viewer),
            (beta.as_str(), Role::Viewer),
        ],
    );
    let token = sign_in_token(&app).await;
    let environments = call_ok(&app, Some(&token), "GET", "/environments", None).await;
    assert_eq!(
        environments,
        json!(["chrome-linux", "firefox-linux", "webkit-linux"])
    );

    // A caller whose only grant names a project that does not exist reaches no
    // project, so the same read answers the empty array rather than refusing:
    // the degrade path the GUI renders when its account is not yet on a
    // project.
    let unreachable = tempfile::tempdir().expect("temp dir");
    let seeding = seeding_app(unreachable.path());
    let delta = seed_project(&seeding, "delta").await;
    seed_configuration(&seeding, &delta, "chrome-linux").await;
    let app = enforcing_app_for(unreachable.path(), &[("missing.json", Role::Viewer)]);
    let token = sign_in_token(&app).await;
    let environments = call_ok(&app, Some(&token), "GET", "/environments", None).await;
    assert_eq!(environments, json!([]));
}

/// The two listings are published: the document the API serves declares both
/// operations, and both answer the bare array of strings the context bar reads
/// rather than an envelope it would have to unwrap.
#[tokio::test]
async fn the_release_and_environment_listings_are_published_in_the_document() {
    let (_directory, app) = test_app();
    let (status, document) = send_json(&app, get("/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);

    let documented = documented_operations(&document);
    for label in ["get /releases", "get /environments"] {
        assert!(
            documented.contains(label),
            "{label} is missing from openapi.json"
        );
    }

    for path in ["/releases", "/environments"] {
        let operation = &document["paths"][path]["get"];
        assert!(
            operation.get("parameters").is_none(),
            "{path} grew a parameter: {operation}"
        );
        let schema = &operation["responses"]["200"]["content"]["application/json"]["schema"];
        assert_eq!(schema["type"], json!("array"), "{path}: {schema}");
        assert_eq!(schema["items"]["type"], json!("string"), "{path}: {schema}");
    }
}
