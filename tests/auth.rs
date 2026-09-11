//! The authentication state on disk, and what a leaked copy of it would buy an
//! attacker.
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
//! Nothing here goes through the HTTP layer: the promise belongs to the store
//! and the token helpers, and testing it there keeps the assertion specific to
//! the format of the bytes at rest rather than to the shape of a response.

use tempfile::TempDir;
use tucano_test::auth::{
    AuthConfig, AuthStore, Role, User, hash_password, hash_refresh_token, login, refresh,
};

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
