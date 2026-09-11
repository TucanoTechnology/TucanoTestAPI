//! The authentication state on disk, what a leaked copy of it would buy an
//! attacker, and the session endpoints a client signs in through.
//!
//! The API has no database, so accounts, refresh tokens, and project grants are
//! files below the data root like every other resource. That is only safe while
//! those files hold what a server can check a secret against and never the
//! secret itself. This file reads the bytes the store actually wrote and
//! asserts the properties the design promises: a password is present only as an
//! Argon2id hash, a refresh token only as its SHA-256 digest, and no usable
//! access token is written at all — access tokens are stateless and live only
//! in the client's hands.
//!
//! The store-level tests read the bytes the store actually wrote; the HTTP
//! tests below them drive the session endpoints and assert the contract a
//! client sees, which no assertion on the files at rest can cover.

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::Path;
use tempfile::TempDir;
use tower::ServiceExt;
use tucano_test::auth::{
    AuthConfig, AuthStore, Role, User, hash_password, hash_refresh_token, login, refresh,
};
use tucano_test::{api, repository::FileRepository};

const SECRET: &[u8] = b"an-integration-test-secret-of-32-plus!";
const PASSWORD: &str = "correct horse battery staple";
const USER_ID: &str = "account-1";
const USERNAME: &str = "alice";
const NOW: u64 = 1_700_000_000;

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

fn read(directory: &TempDir, relative: &str) -> String {
    std::fs::read_to_string(directory.path().join(relative))
        .unwrap_or_else(|error| panic!("reading {relative}: {error}"))
}

/// A signed-in account leaves no secret behind: the file a copy of the data
/// directory would expose can only be checked against, not replayed.
#[test]
fn the_accounts_file_holds_no_plaintext_or_usable_token() {
    let directory = tempfile::tempdir().expect("temp dir");
    let store = AuthStore::new(directory.path()).expect("store");
    store.insert_user(&account()).expect("insert the account");

    let session = login(&store, &config(), USERNAME, PASSWORD, NOW).expect("sign in");
    // Rotate, so one refresh token is spent and one is live when the file is
    // read; neither may appear.
    let rotated = refresh(&store, &config(), &session.refresh_token, NOW + 1).expect("rotate");
    let contents = read(&directory, "auth/users.json");

    assert!(
        !contents.contains(PASSWORD),
        "the plaintext password reached the file:\n{contents}"
    );
    assert!(
        contents.contains("$argon2id$"),
        "the password is not stored as an Argon2id PHC string:\n{contents}"
    );

    for token in [
        session.refresh_token.as_str(),
        rotated.refresh_token.as_str(),
        session.access_token.as_str(),
        rotated.access_token.as_str(),
    ] {
        assert!(
            !contents.contains(token),
            "a usable token reached the file:\n{contents}"
        );
    }

    // The digest of the live refresh token is what replaces it, and it is a
    // plain lowercase SHA-256 hex string rather than the token or a hash of
    // anything recoverable.
    let digest = hash_refresh_token(&rotated.refresh_token);
    assert_eq!(digest.len(), 64, "not a SHA-256 hex digest: {digest}");
    assert!(
        digest
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()),
        "not a lowercase hex digest: {digest}"
    );
    assert!(
        contents.contains(&digest),
        "the live refresh token is not tracked by its digest:\n{contents}"
    );
    assert!(
        !contents.contains(&hash_refresh_token(&session.refresh_token)),
        "the spent refresh token is still tracked:\n{contents}"
    );
}

/// Grants name accounts and roles, never credentials, so a leaked grants file
/// reveals who can reach a project without handing anyone a way in.
#[test]
fn the_grants_file_names_accounts_and_lowercase_roles_only() {
    let directory = tempfile::tempdir().expect("temp dir");
    let store = AuthStore::new(directory.path()).expect("store");
    store.insert_user(&account()).expect("insert the account");
    store
        .set_role("project-a.json", USER_ID, Role::Owner)
        .expect("grant the role");

    let contents = read(&directory, "auth/projects/project-a.json");

    assert!(
        contents.contains(USER_ID),
        "the grant does not name the account:\n{contents}"
    );
    assert!(
        contents.contains("owner"),
        "the role is not the lowercase wire name:\n{contents}"
    );
    assert!(
        !contents.contains(PASSWORD) && !contents.contains("$argon2id$"),
        "credential material reached the grants file:\n{contents}"
    );
}

/// A router that enforces authentication, backed by an isolated data directory
/// holding one account and the grants named. The caller keeps the `TempDir`.
fn enforcing_app_with_grants(directory: &Path, grants: &[(&str, Role)]) -> Router {
    let store = AuthStore::new(directory).expect("auth store");
    store.insert_user(&account()).expect("insert the account");
    for (project, role) in grants {
        store
            .set_role(project, USER_ID, *role)
            .expect("grant the role");
    }
    let repository = FileRepository::new(directory).expect("repository");
    api::router(repository, api::auth::AuthState::new(store, config()))
}

fn enforcing_app(directory: &Path) -> Router {
    enforcing_app_with_grants(directory, &[])
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

/// Signs the seeded account in and returns the session body.
async fn sign_in(app: &Router) -> Value {
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
    session
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().expect("error code")
}

fn assert_challenge(headers: &HeaderMap) {
    let challenge = headers
        .get(header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .expect("WWW-Authenticate challenge");
    assert!(
        challenge.starts_with("Bearer realm="),
        "not a bearer challenge: {challenge}"
    );
}

fn access_token(session: &Value) -> &str {
    session["accessToken"].as_str().expect("access token")
}

fn refresh_token(session: &Value) -> &str {
    session["refreshToken"].as_str().expect("refresh token")
}

/// A sign-in answers a session the API accepts, in the wire shape the document
/// publishes.
#[tokio::test]
async fn signing_in_answers_a_session_the_api_accepts() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());

    let session = sign_in(&app).await;
    assert_eq!(session["tokenType"], json!("Bearer"));
    assert_eq!(session["expiresIn"], json!(900));
    assert!(
        !refresh_token(&session).is_empty(),
        "the sign-in issued no refresh token"
    );

    let (status, _, me) = send(
        &app,
        request("GET", "/auth/me", Some(access_token(&session)), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "me: {me}");
    assert_eq!(me["id"], json!(USER_ID));
    assert_eq!(me["username"], json!(USERNAME));
    assert_eq!(me["systemAdmin"], json!(false));
    assert_eq!(me["roles"], json!({}));
}

/// An unknown username and a wrong password are one answer, so the reply cannot
/// be used to discover which usernames exist.
#[tokio::test]
async fn the_two_ways_a_sign_in_fails_are_one_answer() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());

    let (status, headers, wrong) = send(
        &app,
        request(
            "POST",
            "/auth/login",
            None,
            Some(json!({ "username": USERNAME, "password": "not the password" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&wrong), "invalid_credentials");
    assert_challenge(&headers);

    let (status, _, unknown) = send(
        &app,
        request(
            "POST",
            "/auth/login",
            None,
            Some(json!({ "username": "nobody", "password": PASSWORD })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&unknown), "invalid_credentials");
    assert_eq!(
        wrong["error"]["message"], unknown["error"]["message"],
        "the two failures are distinguishable by their message"
    );
}

/// Rotation spends the token it exchanges, so a replayed token is refused
/// rather than answered.
#[tokio::test]
async fn a_refresh_token_is_spent_by_the_exchange_that_uses_it() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());
    let session = sign_in(&app).await;
    let spent = refresh_token(&session).to_owned();

    let (status, _, rotated) = send(
        &app,
        request(
            "POST",
            "/auth/refresh",
            None,
            Some(json!({ "refreshToken": spent })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "rotate: {rotated}");
    assert_ne!(
        refresh_token(&rotated),
        spent,
        "rotation reissued the token it spent"
    );

    let (status, headers, replay) = send(
        &app,
        request(
            "POST",
            "/auth/refresh",
            None,
            Some(json!({ "refreshToken": spent })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&replay), "invalid_refresh_token");
    assert_challenge(&headers);
}

/// Signing out revokes the refresh token it names, so it cannot be exchanged
/// afterwards.
#[tokio::test]
async fn signing_out_revokes_the_refresh_token_it_names() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());
    let session = sign_in(&app).await;
    let token = refresh_token(&session).to_owned();

    let (status, _, body) = send(
        &app,
        request(
            "POST",
            "/auth/logout",
            Some(access_token(&session)),
            Some(json!({ "refreshToken": token })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "sign out: {body}");
    assert_eq!(body["message"], json!("Signed out"));

    let (status, _, replay) = send(
        &app,
        request(
            "POST",
            "/auth/refresh",
            None,
            Some(json!({ "refreshToken": token })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&replay), "invalid_refresh_token");
}

/// A request that names no caller is refused with `missing_token`, never
/// treated as an anonymous one.
#[tokio::test]
async fn a_request_that_names_no_caller_is_refused() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());

    let (status, headers, body) = send(&app, request("GET", "/auth/me", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "missing_token");
    assert_challenge(&headers);
}

/// A token this deployment did not mint is refused as badly authenticated, not
/// as unauthenticated.
#[tokio::test]
async fn a_token_this_deployment_did_not_mint_is_refused() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());

    let (status, headers, body) =
        send(&app, request("GET", "/auth/me", Some("not.a.token"), None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "invalid_token");
    assert_challenge(&headers);
}

/// The caller is told which projects it can reach and at what level.
#[tokio::test]
async fn the_caller_is_told_which_projects_it_can_reach() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app_with_grants(directory.path(), &[("project-a.json", Role::Owner)]);
    let session = sign_in(&app).await;

    let (status, _, me) = send(
        &app,
        request("GET", "/auth/me", Some(access_token(&session)), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "me: {me}");
    assert_eq!(me["roles"], json!({ "project-a.json": "owner" }));
}

/// A body the endpoint does not understand is the documented `invalid_request`
/// envelope, not axum's own plain-text rejection: an unknown field (the schema
/// declares `additionalProperties: false`) and malformed JSON alike.
#[tokio::test]
async fn a_body_the_endpoint_does_not_understand_is_an_invalid_request() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());

    let (status, _, body) = send(
        &app,
        request(
            "POST",
            "/auth/login",
            None,
            Some(json!({ "username": USERNAME, "password": PASSWORD, "extra": true })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "invalid_request");

    let malformed = Request::builder()
        .method("POST")
        .uri("/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{ not json"))
        .expect("request");
    let (status, headers, body) = send(&app, malformed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "invalid_request");
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("application/json")),
        "the rejection is not the JSON envelope"
    );
}
