//! #422: the summary report's identifier filters must not tell a
//! restricted caller what EXISTS in tenants it cannot see. "No such
//! milestone" and "a milestone you may not know about" answer
//! identically — the empty report, shaped exactly like any filter that
//! matched nothing. Stated answers stay: a caller that CAN see one of the
//! homes of an ambiguous identifier still gets the documented 409, and a
//! trusted deployment still gets every 404.

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use common::{assert_error_envelope, send_json};
use serde_json::Value;
use tucano_test::{
    api,
    auth::{AuthConfig, AuthStore, Role, User, hash_password},
    repository::FileRepository,
};

const SECRET: &[u8] = b"report-oracle-secret-of-at-least-32-bytes!";
const PASSWORD: &str = "passw0rd-for-oracle-test";

fn account(id: &str, username: &str, admin: bool) -> User {
    User {
        id: id.to_owned(),
        username: username.to_owned(),
        password_hash: hash_password(PASSWORD).expect("hash"),
        system_admin: admin,
        created_at: 1_700_000_000,
        refresh_tokens: Vec::new(),
    }
}

fn request(method: &str, uri: &str, token: &str, body: Option<&Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    let body = match body {
        Some(body) => {
            builder = builder.header("content-type", "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("request")
}

async fn token(app: &Router, username: &str) -> String {
    let (status, session) = send_json(
        app,
        request(
            "POST",
            "/auth/login",
            "none",
            Some(&serde_json::json!({"username": username, "password": PASSWORD})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "sign in {username}: {session}");
    session["accessToken"].as_str().expect("token").to_owned()
}

async fn created(app: &Router, admin: &str, path: &str, body: &Value) -> Value {
    let (status, created) = send_json(app, request("POST", path, admin, Some(body))).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "POST {path} -> {status}: {created}"
    );
    created
}

async fn report(app: &Router, caller: &str, query: &str) -> (StatusCode, Value) {
    send_json(
        app,
        request("GET", &format!("/reports/summary?{query}"), caller, None),
    )
    .await
}

#[tokio::test]
async fn foreign_and_absent_identifiers_answer_identically_to_a_restricted_caller() {
    let directory = tempfile::TempDir::new().expect("temp dir");
    let repository = FileRepository::new(directory.path()).expect("repo");
    let store = AuthStore::new(directory.path()).expect("store");
    store
        .insert_user(&account("u-admin", "adm", true))
        .expect("admin");
    store
        .insert_user(&account("u-w", "watcher", false))
        .expect("watcher");
    store
        .insert_user(&account("u-o", "outsider", false))
        .expect("outsider");
    let config = AuthConfig {
        required: true,
        jwt_secret: Some(SECRET.to_vec()),
        access_ttl: std::time::Duration::from_secs(900),
        refresh_ttl: std::time::Duration::from_secs(1_209_600),
        bootstrap_username: None,
        bootstrap_password: None,
    };
    let app = api::router(repository, api::auth::AuthState::new(store.clone(), config));
    let admin = token(&app, "adm").await;

    // Three tenants, one milestone hidden in B (plus a duplicated
    // identifier across A and B for the ambiguity contract), and a
    // configuration in B.
    for name in ["alpha", "bravo", "charlie"] {
        created(
            &app,
            &admin,
            "/projects",
            &serde_json::json!({"name": name}),
        )
        .await;
    }
    store
        .set_role("alpha.json", "u-w", Role::Viewer)
        .expect("watcher grant");
    store
        .set_role("charlie.json", "u-o", Role::Viewer)
        .expect("outsider grant");
    created(
        &app,
        &admin,
        "/projects/bravo.json/milestones",
        &serde_json::json!({"milestoneId": "hidden.json", "name": "Hidden", "testRunIds": ["ghost-run.json"]}),
    )
    .await;
    for project in ["alpha", "bravo"] {
        created(
            &app,
            &admin,
            &format!("/projects/{project}.json/milestones"),
            &serde_json::json!({"milestoneId": "dupe.json", "name": "Dupe", "testRunIds": ["ghost-run.json"]}),
        )
        .await;
    }
    created(
        &app,
        &admin,
        "/projects/bravo.json/configurations",
        &serde_json::json!({"name": "cfg-visible"}),
    )
    .await;

    let watcher = token(&app, "watcher").await;
    let outsider = token(&app, "outsider").await;

    // The core equivalence: an identifier that EXISTS out of scope and one
    // that does not exist at all must be indistinguishable.
    for (probe, exists) in [
        ("milestoneId=hidden.json", "foreign milestone"),
        ("milestoneId=does-not-exist.json", "absent milestone"),
        (
            "configurationId=does-not-exist.json",
            "absent configuration",
        ),
    ] {
        let (status, body) = report(&app, &outsider, probe).await;
        assert_eq!(status, StatusCode::OK, "{exists} as outsider: {body}");
        assert_eq!(body["total"], 0, "{exists} must report emptiness: {body}");
    }

    // Ambiguity the caller CAN see one home of remains the documented 409 —
    // masking only removes cross-tenant signal, never a stated answer.
    let (status, body) = report(&app, &watcher, "milestoneId=dupe.json").await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "visible-home ambiguity: {body}"
    );
    assert_error_envelope(&body, "conflict");
    // …and to the caller who sees NEITHER home it is just another empty
    // report, identical to an absent identifier.
    let (dupe_status, dupe_body) = report(&app, &outsider, "milestoneId=dupe.json").await;
    let (absent_status, absent_body) =
        report(&app, &outsider, "milestoneId=does-not-exist.json").await;
    assert_eq!(
        (dupe_status, &dupe_body),
        (absent_status, &absent_body),
        "the two probes must be byte-identical answers"
    );

    // A trusted caller's documented 404s are untouched (the reports suite
    // pins them for the auth-off router); ADMIN over the enforcing router
    // also keeps them: full scope, genuine not-found.
    let (status, body) = report(&app, &admin, "milestoneId=does-not-exist.json").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "admin keeps the real 404: {body}"
    );
    assert_error_envelope(&body, "not_found");
}
