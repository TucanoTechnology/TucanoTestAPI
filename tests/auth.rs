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

mod common;

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

use common::{ROLE_CHECKED_WRITE_OPERATIONS, role_checked_write};

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

/// The seeded account, with or without the global system-administrator
/// capability. Everything else about it is fixed.
fn user(system_admin: bool) -> User {
    User {
        system_admin,
        ..account()
    }
}

/// A router that enforces authentication, backed by an isolated data directory
/// holding one account — whose global authority the caller chooses — and the
/// grants named. The caller keeps the `TempDir`.
fn enforcing_app_for(directory: &Path, system_admin: bool, grants: &[(&str, Role)]) -> Router {
    let store = AuthStore::new(directory).expect("auth store");
    store
        .insert_user(&user(system_admin))
        .expect("insert the account");
    for (project, role) in grants {
        store
            .set_role(project, USER_ID, *role)
            .expect("grant the role");
    }
    let repository = FileRepository::new(directory).expect("repository");
    api::router(repository, api::auth::AuthState::new(store, config()))
}

fn enforcing_app_with_grants(directory: &Path, grants: &[(&str, Role)]) -> Router {
    enforcing_app_for(directory, false, grants)
}

fn enforcing_app(directory: &Path) -> Router {
    enforcing_app_with_grants(directory, &[])
}

/// A router whose seeded account administers everything, for building the
/// fixture the role matrix reads against.
fn administrating_app(directory: &Path) -> Router {
    enforcing_app_for(directory, true, &[])
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

// ---------------------------------------------------------------------------
// The authorization matrix.
//
// `src/api/access.rs` resolves a request to the projects behind it and asks one
// question of each. The tests below drive the HTTP surface with real tokens and
// real grants: with enforcement on, every guarded handler refuses a caller that
// names no token, a caller reaches only the projects it was granted, and a
// request that names an entity the store does not hold is still answerable when
// it is really a creation.
// ---------------------------------------------------------------------------

/// The same deployment with enforcement off, for seeding a tree without
/// authenticating. Every guard early-returns, so nothing is written to the auth
/// tree and the same directory can be reopened with enforcement on afterwards.
fn config_without_enforcement() -> AuthConfig {
    AuthConfig {
        required: false,
        ..config()
    }
}

fn seeding_app(directory: &Path) -> Router {
    let store = AuthStore::new(directory).expect("auth store");
    let repository = FileRepository::new(directory).expect("repository");
    api::router(
        repository,
        api::auth::AuthState::new(store, config_without_enforcement()),
    )
}

fn case_body(id: &str) -> Value {
    json!({ "testCaseId": id, "title": "Login", "expectedResult": "Stored" })
}

fn run_body(name: &str, project: &str) -> Value {
    json!({
        "name": name,
        "timestamp": "2026-09-04T00:00:00Z",
        "projects": [{ "projectId": project, "name": "alpha", "testSuites": [] }],
    })
}

fn id_of(body: &Value) -> String {
    body["id"].as_str().expect("created id").to_owned()
}

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

async fn assert_forbidden(app: &Router, token: &str, method: &str, uri: &str, body: Option<Value>) {
    let (status, _, answer) = send(app, request(method, uri, Some(token), body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {answer}");
    assert_eq!(error_code(&answer), "forbidden", "{method} {uri}: {answer}");
}

async fn assert_anonymous_refused(app: &Router, method: &str, uri: &str) {
    let (status, headers, answer) = send(app, request(method, uri, None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}: {answer}");
    assert_eq!(
        error_code(&answer),
        "missing_token",
        "{method} {uri}: {answer}"
    );
    assert_challenge(&headers);
}

async fn sign_in_token(app: &Router) -> String {
    access_token(&sign_in(app).await).to_owned()
}

/// The identifiers of one seeded tree: a project, a suite and a case in it, a
/// run covering the project, and a milestone referencing the run.
struct Tree {
    project: String,
    suite: String,
    case: String,
    run: String,
    milestone: String,
}

async fn seed_tree(app: &Router, token: Option<&str>) -> Tree {
    let project = id_of(
        &call_ok(
            app,
            token,
            "POST",
            "/projects",
            Some(json!({ "name": "alpha" })),
        )
        .await,
    );
    let suite = id_of(
        &call_ok(
            app,
            token,
            "POST",
            &format!("/projects/{project}/test_suites"),
            Some(json!({ "name": "smoke" })),
        )
        .await,
    );
    let case = id_of(
        &call_ok(
            app,
            token,
            "POST",
            &format!("/projects/{project}/test_cases"),
            Some(case_body("TC-1")),
        )
        .await,
    );
    let run = id_of(
        &call_ok(
            app,
            token,
            "POST",
            "/test_runs",
            Some(run_body("nightly", &project)),
        )
        .await,
    );
    let milestone = id_of(
        &call_ok(
            app,
            token,
            "POST",
            "/milestones",
            Some(json!({ "name": "sprint-42", "testRunIds": [run] })),
        )
        .await,
    );
    Tree {
        project,
        suite,
        case,
        run,
        milestone,
    }
}

/// Seeds the tree through an enforcement-off app, then reopens the same data
/// directory with enforcement on and a single grant, and signs in.
async fn app_with_role(directory: &Path, role: Role) -> (Router, Tree, String) {
    let tree = seed_tree(&seeding_app(directory), None).await;
    let app = enforcing_app_with_grants(directory, &[(tree.project.as_str(), role)]);
    let token = sign_in_token(&app).await;
    (app, tree, token)
}

async fn administrating_tree(directory: &Path) -> (Router, Tree, String) {
    let tree = seed_tree(&seeding_app(directory), None).await;
    let app = administrating_app(directory);
    let token = sign_in_token(&app).await;
    (app, tree, token)
}

/// A caller granted a role in a project that does not exist: reachable
/// everywhere, entitled nowhere.
async fn app_with_foreign_grant(directory: &Path) -> (Router, Tree, String) {
    let tree = seed_tree(&seeding_app(directory), None).await;
    let app = enforcing_app_with_grants(directory, &[("beta.json", Role::Owner)]);
    let token = sign_in_token(&app).await;
    (app, tree, token)
}

/// Every route that reads or writes a project resource refuses a caller that
/// names no token, before it parses the body: the refusal is the documented
/// `missing_token` envelope with its bearer challenge.
#[tokio::test]
async fn every_guarded_operation_refuses_an_anonymous_caller() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());

    let operations: Vec<(&str, &str)> = vec![
        ("GET", "/projects"),
        ("POST", "/projects"),
        ("GET", "/projects/missing.json"),
        ("PUT", "/projects/missing.json"),
        ("DELETE", "/projects/missing.json"),
        ("POST", "/projects/missing.json/duplicate"),
        ("GET", "/test_suites"),
        ("POST", "/test_suites"),
        ("GET", "/test_suites/missing.json"),
        ("PUT", "/test_suites/missing.json"),
        ("DELETE", "/test_suites/missing.json"),
        ("POST", "/test_suites/missing.json/duplicate"),
        ("GET", "/test_suites/missing.json/test_cases"),
        ("POST", "/test_suites/missing.json/test_cases"),
        ("DELETE", "/test_suites/missing.json/test_cases/missing"),
        ("GET", "/projects/missing.json/test_suites"),
        ("POST", "/projects/missing.json/test_suites"),
        ("DELETE", "/projects/missing.json/test_suites/missing.json"),
        ("GET", "/test_cases"),
        ("POST", "/test_cases"),
        ("GET", "/test_cases/missing"),
        ("PUT", "/test_cases/missing"),
        ("DELETE", "/test_cases/missing"),
        ("POST", "/test_cases/missing/duplicate"),
        ("POST", "/test_cases/missing/attachments"),
        ("GET", "/test_cases/missing/attachments/report.txt"),
        ("DELETE", "/test_cases/missing/attachments/report.txt"),
        ("GET", "/test_cases/missing/steps/0/attachments"),
        ("POST", "/test_cases/missing/steps/0/attachments"),
        (
            "DELETE",
            "/test_cases/missing/steps/0/attachments/report.txt",
        ),
        ("GET", "/test_cases/missing/history"),
        ("GET", "/test_cases/missing/history/1"),
        ("GET", "/projects/missing.json/test_cases"),
        ("POST", "/projects/missing.json/test_cases"),
        ("DELETE", "/projects/missing.json/test_cases/missing"),
        ("GET", "/test_runs"),
        ("POST", "/test_runs"),
        ("GET", "/test_runs/missing.json"),
        ("PUT", "/test_runs/missing.json"),
        ("DELETE", "/test_runs/missing.json"),
        ("POST", "/test_runs/missing.json/duplicate"),
        ("POST", "/test_runs/missing.json/test_suites"),
        ("POST", "/test_runs/missing.json/test_cases"),
        ("POST", "/test_runs/missing.json/results"),
        ("GET", "/test_runs/missing.json/results/missing/defects"),
        ("POST", "/test_runs/missing.json/results/missing/defects"),
        (
            "DELETE",
            "/test_runs/missing.json/results/missing/defects/link-1",
        ),
        ("POST", "/test_runs/missing.json/import/junit"),
        ("POST", "/test_runs/missing.json/import/json"),
        ("POST", "/test_runs/missing.json/configurations"),
        ("DELETE", "/test_runs/missing.json/configurations/config-1"),
        ("GET", "/milestones"),
        ("POST", "/milestones"),
        ("GET", "/milestones/missing.json"),
        ("PUT", "/milestones/missing.json"),
        ("DELETE", "/milestones/missing.json"),
        ("POST", "/milestones/missing.json/duplicate"),
        ("GET", "/milestones/missing.json/progress"),
        ("GET", "/configurations"),
        ("POST", "/configurations"),
        ("GET", "/configurations/missing.json"),
        ("PUT", "/configurations/missing.json"),
        ("DELETE", "/configurations/missing.json"),
        ("GET", "/reports/coverage"),
        ("GET", "/reports/summary"),
        ("POST", "/auth/logout"),
        ("GET", "/auth/me"),
    ];
    assert_eq!(
        operations.len(),
        67,
        "the guarded surface changed; update this matrix with it"
    );

    for (method, uri) in operations {
        assert_anonymous_refused(&app, method, uri).await;
    }
}

/// The endpoints that exist before a caller does stay open, and the ones a
/// caller signs in through answer their own documented failure rather than a
/// bearer challenge for a token they were trying to obtain.
#[tokio::test]
async fn the_public_endpoints_need_no_caller() {
    let directory = tempfile::tempdir().expect("temp dir");
    let app = enforcing_app(directory.path());

    for uri in ["/health", "/openapi.json", "/api-docs", "/api-docs/"] {
        let (status, _, answer) = send(&app, request("GET", uri, None, None)).await;
        assert_eq!(status, StatusCode::OK, "GET {uri}: {answer}");
    }

    let (status, headers, answer) = send(
        &app,
        request(
            "POST",
            "/auth/login",
            None,
            Some(json!({ "username": USERNAME, "password": "not the password" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{answer}");
    assert_eq!(error_code(&answer), "invalid_credentials");
    assert_challenge(&headers);

    let (status, headers, answer) = send(
        &app,
        request(
            "POST",
            "/auth/refresh",
            None,
            Some(json!({ "refreshToken": "not-a-token" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{answer}");
    assert_eq!(error_code(&answer), "invalid_refresh_token");
    assert_challenge(&headers);
}

/// A viewer reaches the project it was granted on every guarded read, and the
/// listings are filtered down to it rather than refused; every write is a 403.
#[tokio::test]
async fn a_viewer_reads_the_projects_it_reaches_and_cannot_write() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (app, tree, token) = app_with_role(directory.path(), Role::Viewer).await;
    let Tree {
        project,
        suite,
        case,
        run,
        milestone,
    } = &tree;
    let token = token.as_str();

    let projects = call_ok(&app, Some(token), "GET", "/projects", None).await;
    assert_eq!(projects, json!([project]));

    let single = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/projects/{project}"),
        None,
    )
    .await;
    assert_eq!(single["projectId"], json!(project));

    let suites = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/projects/{project}/test_suites"),
        None,
    )
    .await;
    assert_eq!(suites, json!([suite]));

    let project_cases = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/projects/{project}/test_cases"),
        None,
    )
    .await;
    assert_eq!(project_cases, json!([case]));

    let cases = call_ok(&app, Some(token), "GET", "/test_cases", None).await;
    assert_eq!(cases, json!([case]));

    let single_case = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/test_cases/{case}"),
        None,
    )
    .await;
    assert_eq!(single_case["testCaseId"], json!(case));

    let history = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/test_cases/{case}/history"),
        None,
    )
    .await;
    assert_eq!(history, json!([]));

    let all_suites = call_ok(&app, Some(token), "GET", "/test_suites", None).await;
    assert_eq!(all_suites, json!([suite]));

    let single_suite = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/test_suites/{suite}"),
        None,
    )
    .await;
    assert_eq!(single_suite["suiteId"], json!(suite));

    let suite_cases = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/test_suites/{suite}/test_cases"),
        None,
    )
    .await;
    assert_eq!(suite_cases, json!([]));

    let runs = call_ok(&app, Some(token), "GET", "/test_runs", None).await;
    assert_eq!(runs, json!([run]));

    let single_run = call_ok(&app, Some(token), "GET", &format!("/test_runs/{run}"), None).await;
    assert_eq!(single_run["testRunId"], json!(run));

    let milestones = call_ok(&app, Some(token), "GET", "/milestones", None).await;
    assert_eq!(milestones, json!([milestone]));

    let single_milestone = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/milestones/{milestone}"),
        None,
    )
    .await;
    assert_eq!(single_milestone["milestoneId"], json!(milestone));

    call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/milestones/{milestone}/progress"),
        None,
    )
    .await;

    let configurations = call_ok(&app, Some(token), "GET", "/configurations", None).await;
    assert_eq!(configurations, json!([]));

    let coverage = call_ok(&app, Some(token), "GET", "/reports/coverage", None).await;
    assert_eq!(coverage["totalCases"], json!(1));
    assert!(
        coverage.get("projectId").is_none(),
        "an unscoped report echoed an identifier: {coverage}"
    );
    assert_eq!(coverage["suites"][0]["suiteId"], json!(suite));
    assert_eq!(coverage["suites"][0]["caseCount"], json!(0));

    let scoped = call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/reports/coverage?projectId={project}"),
        None,
    )
    .await;
    assert_eq!(scoped["projectId"], json!(project));

    call_ok(&app, Some(token), "GET", "/reports/summary", None).await;
    call_ok(
        &app,
        Some(token),
        "GET",
        &format!("/reports/summary?projectId={project}"),
        None,
    )
    .await;

    let me = call_ok(&app, Some(token), "GET", "/auth/me", None).await;
    assert_eq!(me["roles"][project.as_str()], json!("viewer"));

    // The matrix is derived from the shared surface, so the routes this test
    // proves answer `403` and the routes `service` proves document one can
    // never drift apart.
    let writes: Vec<(&str, String, Option<Value>)> = ROLE_CHECKED_WRITE_OPERATIONS
        .iter()
        .map(|label| {
            role_checked_write(
                label,
                project.as_str(),
                suite.as_str(),
                case.as_str(),
                run.as_str(),
                milestone.as_str(),
            )
        })
        .collect();
    assert_eq!(
        writes.len(),
        33,
        "the write surface changed; update this matrix with it"
    );

    for (method, uri, body) in writes {
        assert_forbidden(&app, token, method, &uri, body).await;
    }
}

/// An editor writes the content inside its project but not the project itself
/// and not the milestones that crown it.
#[tokio::test]
async fn an_editor_writes_content_but_not_projects_or_milestones() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (app, tree, token) = app_with_role(directory.path(), Role::Editor).await;
    let Tree {
        project,
        suite,
        case,
        run,
        milestone,
    } = &tree;
    let token = token.as_str();

    // Creating a case whose identifier the store does not hold is a creation,
    // not an attempt to read a foreign one.
    let created = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            &format!("/projects/{project}/test_cases"),
            Some(case_body("TC-NEW")),
        )
        .await,
    );
    assert_eq!(created, "TC-NEW");

    let composed = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            &format!("/test_suites/{suite}/test_cases"),
            Some(case_body("TC-NEW-2")),
        )
        .await,
    );
    assert_eq!(composed, "TC-NEW-2");

    call_ok(
        &app,
        Some(token),
        "PUT",
        &format!("/test_suites/{suite}"),
        Some(json!({})),
    )
    .await;

    let copy = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            &format!("/test_suites/{suite}/duplicate"),
            Some(json!({ "newId": "smoke-copy.json" })),
        )
        .await,
    );
    assert_eq!(copy, "smoke-copy.json");
    call_ok(
        &app,
        Some(token),
        "DELETE",
        &format!("/test_suites/{copy}"),
        None,
    )
    .await;

    let extra = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            "/test_runs",
            Some(run_body("extra", project)),
        )
        .await,
    );
    assert_eq!(extra, "extra.json");
    call_ok(
        &app,
        Some(token),
        "PUT",
        &format!("/test_runs/{extra}"),
        Some(json!({})),
    )
    .await;
    call_ok(
        &app,
        Some(token),
        "DELETE",
        &format!("/test_runs/{extra}"),
        None,
    )
    .await;

    let run_copy = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            &format!("/test_runs/{run}/duplicate"),
            Some(json!({ "newId": "nightly-copy.json" })),
        )
        .await,
    );
    assert_eq!(run_copy, "nightly-copy.json");
    call_ok(
        &app,
        Some(token),
        "DELETE",
        &format!("/test_runs/{run_copy}"),
        None,
    )
    .await;

    call_ok(
        &app,
        Some(token),
        "POST",
        &format!("/test_runs/{run}/test_suites"),
        Some(json!({ "suiteId": suite })),
    )
    .await;
    call_ok(
        &app,
        Some(token),
        "POST",
        &format!("/test_runs/{run}/test_cases"),
        Some(json!({ "testCaseId": case })),
    )
    .await;
    call_ok(
        &app,
        Some(token),
        "POST",
        &format!("/test_runs/{run}/results"),
        Some(json!({ "testCaseId": case, "status": "Passed" })),
    )
    .await;

    assert_forbidden(
        &app,
        token,
        "POST",
        "/projects",
        Some(json!({ "name": "beta" })),
    )
    .await;
    assert_forbidden(
        &app,
        token,
        "PUT",
        &format!("/projects/{project}"),
        Some(json!({})),
    )
    .await;
    assert_forbidden(&app, token, "DELETE", &format!("/projects/{project}"), None).await;
    assert_forbidden(
        &app,
        token,
        "POST",
        &format!("/projects/{project}/duplicate"),
        Some(json!({})),
    )
    .await;
    assert_forbidden(
        &app,
        token,
        "POST",
        "/milestones",
        Some(json!({ "name": "linked", "testRunIds": [run] })),
    )
    .await;
    assert_forbidden(
        &app,
        token,
        "PUT",
        &format!("/milestones/{milestone}"),
        Some(json!({})),
    )
    .await;
    assert_forbidden(
        &app,
        token,
        "DELETE",
        &format!("/milestones/{milestone}"),
        None,
    )
    .await;
    assert_forbidden(
        &app,
        token,
        "POST",
        &format!("/milestones/{milestone}/duplicate"),
        Some(json!({})),
    )
    .await;
}

/// An owner administers the project it was granted, including the milestones
/// linked to it, but creating a project needs the system administrator.
#[tokio::test]
async fn an_owner_administers_its_project_but_not_the_installation() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (app, tree, token) = app_with_role(directory.path(), Role::Owner).await;
    let Tree { project, run, .. } = &tree;
    let token = token.as_str();

    call_ok(
        &app,
        Some(token),
        "PUT",
        &format!("/projects/{project}"),
        Some(json!({})),
    )
    .await;

    let milestone = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            "/milestones",
            Some(json!({ "name": "linked", "testRunIds": [run] })),
        )
        .await,
    );
    assert_eq!(milestone, "linked.json");
    call_ok(
        &app,
        Some(token),
        "PUT",
        &format!("/milestones/{milestone}"),
        Some(json!({})),
    )
    .await;
    call_ok(
        &app,
        Some(token),
        "DELETE",
        &format!("/milestones/{milestone}"),
        None,
    )
    .await;

    assert_forbidden(
        &app,
        token,
        "POST",
        "/projects",
        Some(json!({ "name": "beta" })),
    )
    .await;
    assert_forbidden(
        &app,
        token,
        "POST",
        &format!("/projects/{project}/duplicate"),
        Some(json!({})),
    )
    .await;
}

/// The system administrator is unfiltered: it reads and writes every project
/// and is the only identity that may create one.
#[tokio::test]
async fn a_system_administrator_reaches_everything() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (app, tree, token) = administrating_tree(directory.path()).await;
    let Tree { project, run, .. } = &tree;
    let token = token.as_str();

    let projects = call_ok(&app, Some(token), "GET", "/projects", None).await;
    assert_eq!(projects, json!([project]));

    let beta = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            "/projects",
            Some(json!({ "name": "beta" })),
        )
        .await,
    );
    assert_eq!(beta, "beta.json");
    call_ok(
        &app,
        Some(token),
        "PUT",
        "/projects/beta.json",
        Some(json!({})),
    )
    .await;

    let gamma = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            "/projects/beta.json/duplicate",
            Some(json!({ "newId": "gamma.json", "newName": "gamma" })),
        )
        .await,
    );
    assert_eq!(gamma, "gamma.json");

    let milestone = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            "/milestones",
            Some(json!({ "name": "linked", "testRunIds": [run] })),
        )
        .await,
    );
    assert_eq!(milestone, "linked.json");

    let (status, _, answer) = send(
        &app,
        request(
            "POST",
            "/milestones",
            Some(token),
            Some(json!({ "name": "orphan" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(error_code(&answer), "invalid_request");
}

/// A milestone is a project resource, so one that references no project is
/// refused rather than left open to any authenticated caller.
#[tokio::test]
async fn a_milestone_must_reference_a_project() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (app, tree, token) = administrating_tree(directory.path()).await;
    let token = token.as_str();

    for body in [
        json!({ "name": "orphan" }),
        json!({ "name": "orphan", "testRunIds": [] }),
        json!({ "name": "orphan", "testSuiteIds": [] }),
    ] {
        let (status, _, answer) = send(
            &app,
            request("POST", "/milestones", Some(token), Some(body)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert_eq!(error_code(&answer), "invalid_request");
    }

    let (status, _, answer) = send(
        &app,
        request(
            "PUT",
            &format!("/milestones/{}", tree.milestone),
            Some(token),
            Some(json!({ "testRunIds": [] })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(error_code(&answer), "invalid_request");
}

/// A configuration is installation-wide, so every authenticated caller may
/// administer it whatever projects it reaches.
#[tokio::test]
async fn configurations_are_reachable_by_every_authenticated_caller() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (app, _tree, token) = app_with_role(directory.path(), Role::Viewer).await;
    let token = token.as_str();

    let created = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            "/configurations",
            Some(json!({ "name": "chrome" })),
        )
        .await,
    );
    assert_eq!(created, "chrome.json");

    let listed = call_ok(&app, Some(token), "GET", "/configurations", None).await;
    assert_eq!(listed, json!(["chrome.json"]));

    call_ok(
        &app,
        Some(token),
        "GET",
        "/configurations/chrome.json",
        None,
    )
    .await;
    call_ok(
        &app,
        Some(token),
        "PUT",
        "/configurations/chrome.json",
        Some(json!({ "browser": "chrome" })),
    )
    .await;
    call_ok(
        &app,
        Some(token),
        "DELETE",
        "/configurations/chrome.json",
        None,
    )
    .await;
}

/// A caller granted a role in a project that does not exist reaches nothing:
/// listings come back empty and every direct read is a 403.
#[tokio::test]
async fn a_caller_with_no_grant_sees_nothing() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (app, tree, token) = app_with_foreign_grant(directory.path()).await;
    let Tree {
        project,
        suite,
        case,
        run,
        milestone,
    } = &tree;
    let token = token.as_str();

    for uri in [
        "/projects",
        "/test_suites",
        "/test_cases",
        "/test_runs",
        "/milestones",
        "/configurations",
    ] {
        let listed = call_ok(&app, Some(token), "GET", uri, None).await;
        assert_eq!(listed, json!([]), "{uri}");
    }

    for uri in [
        format!("/projects/{project}"),
        format!("/test_suites/{suite}"),
        format!("/test_cases/{case}"),
        format!("/test_runs/{run}"),
        format!("/milestones/{milestone}"),
    ] {
        assert_forbidden(&app, token, "GET", &uri, None).await;
    }

    let coverage = call_ok(&app, Some(token), "GET", "/reports/coverage", None).await;
    assert_eq!(coverage["totalCases"], json!(0));
    assert!(
        coverage.get("projectId").is_none(),
        "an unscoped report echoed an identifier: {coverage}"
    );
    assert_eq!(coverage["suites"], json!([]));

    assert_forbidden(
        &app,
        token,
        "GET",
        &format!("/reports/coverage?projectId={project}"),
        None,
    )
    .await;

    call_ok(&app, Some(token), "GET", "/reports/summary", None).await;
    assert_forbidden(
        &app,
        token,
        "GET",
        &format!("/reports/summary?projectId={project}"),
        None,
    )
    .await;
}

/// Composing a case resolves the source it names only when the source is a
/// document the store already holds. A brand-new identifier is the case's own,
/// so a creation must not be answered with the 404 a missing source would give.
#[tokio::test]
async fn composing_a_case_checks_the_source_only_when_it_exists() {
    let directory = tempfile::tempdir().expect("temp dir");
    let seeding = seeding_app(directory.path());
    let alpha = id_of(
        &call_ok(
            &seeding,
            None,
            "POST",
            "/projects",
            Some(json!({ "name": "alpha" })),
        )
        .await,
    );
    let beta = id_of(
        &call_ok(
            &seeding,
            None,
            "POST",
            "/projects",
            Some(json!({ "name": "beta" })),
        )
        .await,
    );
    let suite = id_of(
        &call_ok(
            &seeding,
            None,
            "POST",
            &format!("/projects/{alpha}/test_suites"),
            Some(json!({ "name": "smoke" })),
        )
        .await,
    );
    let beta_case = id_of(
        &call_ok(
            &seeding,
            None,
            "POST",
            &format!("/projects/{beta}/test_cases"),
            Some(case_body("TC-BETA")),
        )
        .await,
    );
    assert_eq!(beta_case, "TC-BETA");

    let app = enforcing_app_with_grants(directory.path(), &[(alpha.as_str(), Role::Editor)]);
    let token = sign_in_token(&app).await;
    let token = token.as_str();

    let created = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            &format!("/projects/{alpha}/test_cases"),
            Some(case_body("TC-NEW")),
        )
        .await,
    );
    assert_eq!(created, "TC-NEW");

    let composed = id_of(
        &call_ok(
            &app,
            Some(token),
            "POST",
            &format!("/test_suites/{suite}/test_cases"),
            Some(case_body("TC-NEW-2")),
        )
        .await,
    );
    assert_eq!(composed, "TC-NEW-2");

    assert_forbidden(
        &app,
        token,
        "POST",
        &format!("/projects/{alpha}/test_cases"),
        Some(json!({ "testCaseId": beta_case })),
    )
    .await;
}
