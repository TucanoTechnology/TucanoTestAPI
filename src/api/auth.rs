//! Authentication, and the authorization guard, on the request path.
//!
//! [`Principal`] is an axum extractor: a handler that names it receives the
//! caller the `Authorization: Bearer <token>` header names, or a rejection that
//! renders as the 401 envelope the API already uses. Verification is a
//! signature check against the configured secret — see
//! [`crate::auth::authenticate`] — so resolving a caller reads no file.
//!
//! Authorization is a separate step a handler performs with [`authorize`],
//! because the project a request addresses is only known once its path has been
//! resolved. Both together make the matrix in the issue: a viewer reads, an
//! editor writes content and runs, an owner administers a project, and a system
//! administrator does anything anywhere.
//!
//! With `TUCANO_AUTH_REQUIRED` off the two steps are inert rather than
//! different: no identity is taken from the request and no grant is consulted,
//! which is exactly the trusted-network service the API was before auth
//! existed.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header, request::Parts},
};

use crate::{
    auth::{AuthConfig, AuthStore, Principal, Role, authenticate},
    domain::DomainError,
};

/// The authentication material a request needs, carried beside the service.
///
/// The store is consulted on sign-in and whenever a grant changes; the settings
/// are what an access token is verified against. Both are shared, so a router
/// state can hold this without copying a store per request.
#[derive(Clone)]
pub struct AuthState {
    /// Accounts and per-project grants.
    pub store: Arc<AuthStore>,
    /// Signing secret, token lifetimes, and whether auth is enforced.
    pub config: Arc<AuthConfig>,
}

impl AuthState {
    /// Wraps a store and its settings for sharing across requests.
    #[must_use]
    pub fn new(store: AuthStore, config: AuthConfig) -> Self {
        Self {
            store: Arc::new(store),
            config: Arc::new(config),
        }
    }
}

impl<S> FromRequestParts<S> for Principal
where
    S: Send + Sync,
    AuthState: FromRef<S>,
{
    type Rejection = DomainError;

    /// Resolves the caller from the request's `Authorization` header.
    ///
    /// When authentication is enforced a usable bearer token is required: a
    /// request that carries none is refused with `missing_token`, and one that
    /// carries a token that does not verify is refused with `invalid_token` or
    /// `token_expired`. When authentication is switched off the header is not
    /// consulted at all and the request carries no identity, which keeps the
    /// unauthenticated deployment byte-for-byte the service it was.
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthState::from_ref(state);
        if !auth.config.required {
            return Ok(anonymous());
        }
        let token = bearer_token(parts).ok_or_else(DomainError::missing_token)?;
        authenticate(&auth.config, token, now_seconds())
    }
}

/// Decides whether `principal` may act at `required` level in `project_id`.
///
/// With authentication switched off there is no identity to check, so the
/// operation is allowed: that is what `TUCANO_AUTH_REQUIRED=false` means, and a
/// handler must call this even then rather than deciding for itself. With it on,
/// a system administrator passes without a grant and everyone else needs a grant
/// at least as strong as `required`.
///
/// # Errors
///
/// [`DomainError::Forbidden`] when the caller is not entitled, and whatever
/// reading the grants failed with when the store cannot be read.
pub fn authorize(
    auth: &AuthState,
    principal: &Principal,
    project_id: &str,
    required: Role,
) -> Result<(), DomainError> {
    if !auth.config.required {
        return Ok(());
    }
    principal.require_role(&auth.store, project_id, required)
}

/// The bearer token the request carries, if it names one.
///
/// The scheme is compared without regard to case, as RFC 7235 asks, and a header
/// that names another scheme, names none, or carries an empty token is treated
/// as no token at all: there is nothing to verify, so the request is refused as
/// unauthenticated rather than as badly authenticated.
fn bearer_token(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// The identity a request carries when authentication is switched off.
///
/// It names no account and holds no authority; [`authorize`] is what makes that
/// harmless, by refusing to consult a grant it was never told to enforce.
fn anonymous() -> Principal {
    Principal {
        user_id: String::new(),
        system_admin: false,
    }
}

/// Unix seconds now, or zero if the clock reads before 1970.
fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{hash_password, mint_access_token};
    use axum::http::{HeaderValue, Request};
    use std::time::Duration;
    use tempfile::TempDir;

    const SECRET: &[u8] = b"an-integration-test-secret-of-32-plus!";

    /// A router state that carries nothing but the authentication material.
    #[derive(Clone)]
    struct Dummy {
        auth: AuthState,
    }

    impl FromRef<Dummy> for AuthState {
        fn from_ref(state: &Dummy) -> Self {
            state.auth.clone()
        }
    }

    fn config(required: bool) -> AuthConfig {
        AuthConfig {
            required,
            jwt_secret: Some(SECRET.to_vec()),
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(1_209_600),
            bootstrap_username: None,
            bootstrap_password: None,
        }
    }

    fn state(required: bool) -> (TempDir, AuthState) {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AuthStore::new(directory.path()).expect("store");
        (directory, AuthState::new(store, config(required)))
    }

    /// Runs the extractor over a request whose `Authorization` header is
    /// `header`, against `state`.
    fn extract(auth: &AuthState, header: Option<&str>) -> Result<Principal, DomainError> {
        let mut request = Request::new(());
        if let Some(value) = header {
            request.headers_mut().insert(
                header::AUTHORIZATION,
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        let (mut parts, _) = request.into_parts();
        let dummy = Dummy { auth: auth.clone() };
        tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(Principal::from_request_parts(&mut parts, &dummy))
    }

    fn code(error: &DomainError) -> &str {
        match error {
            DomainError::Unauthenticated { code, .. } => code,
            other => panic!("expected an unauthenticated error, got {other:?}"),
        }
    }

    /// A token for `subject`, valid from now for fifteen minutes.
    fn token(subject: &str, system_admin: bool) -> String {
        mint_access_token(
            SECRET,
            subject,
            system_admin,
            Duration::from_secs(900),
            now_seconds(),
        )
    }

    #[test]
    fn a_bearer_token_resolves_to_the_caller_it_names() {
        let (_directory, auth) = state(true);
        let principal = extract(&auth, Some(&format!("Bearer {}", token("u1", false))))
            .expect("a valid token is accepted");
        assert_eq!(principal.user_id, "u1");
        assert!(!principal.system_admin);

        let admin = extract(&auth, Some(&format!("Bearer {}", token("root", true))))
            .expect("a valid token is accepted");
        assert_eq!(admin.user_id, "root");
        assert!(
            admin.system_admin,
            "the authority travels in the token, not in the store"
        );
    }

    #[test]
    fn the_bearer_scheme_is_matched_whatever_its_case() {
        let (_directory, auth) = state(true);
        let principal = extract(&auth, Some(&format!("bearer {}", token("u1", false))))
            .expect("the scheme is case-insensitive");
        assert_eq!(principal.user_id, "u1");
    }

    #[test]
    fn a_request_without_a_token_is_refused_while_authentication_is_enforced() {
        let (_directory, auth) = state(true);
        let error = extract(&auth, None).expect_err("no token");
        assert_eq!(code(&error), "missing_token");
    }

    #[test]
    fn a_header_that_names_no_token_is_refused_as_missing_rather_than_invalid() {
        let (_directory, auth) = state(true);
        for header in [
            "Basic dXNlcjpwYXNz",
            "Bearer",
            "Bearer ",
            "Bearer    ",
            "   ",
        ] {
            let error = extract(&auth, Some(header)).expect_err(header);
            assert_eq!(code(&error), "missing_token", "{header:?}");
        }
    }

    #[test]
    fn an_expired_token_reports_its_own_code() {
        let (_directory, auth) = state(true);
        let stale = mint_access_token(
            SECRET,
            "u1",
            false,
            Duration::from_secs(900),
            now_seconds() - 3600,
        );
        let error = extract(&auth, Some(&format!("Bearer {stale}"))).expect_err("expired");
        assert_eq!(code(&error), "token_expired");
    }

    #[test]
    fn a_token_signed_by_another_secret_is_refused() {
        let (_directory, auth) = state(true);
        let foreign = mint_access_token(
            b"a-different-signing-secret-32-plus!!!",
            "u1",
            false,
            Duration::from_secs(900),
            now_seconds(),
        );
        let error = extract(&auth, Some(&format!("Bearer {foreign}"))).expect_err("foreign");
        assert_eq!(code(&error), "invalid_token");
        for malformed in ["", "not.a.token", "Bearer", "a.b.c.d"] {
            let error = extract(&auth, Some(malformed)).expect_err(malformed);
            assert_eq!(code(&error), "missing_token", "{malformed:?}");
        }
    }

    #[test]
    fn with_authentication_switched_off_a_request_carries_no_identity() {
        let (_directory, auth) = state(false);
        assert_eq!(extract(&auth, None).expect("no auth"), anonymous());
        assert_eq!(
            extract(&auth, Some(&format!("Bearer {}", token("u1", true))))
                .expect("the header is ignored"),
            anonymous(),
            "identity is only taken when it is going to be enforced"
        );
    }

    #[test]
    fn authorization_is_not_enforced_while_authentication_is_off() {
        let (_directory, auth) = state(false);
        for required in [Role::Viewer, Role::Editor, Role::Owner] {
            assert!(
                authorize(&auth, &anonymous(), "p1.json", required).is_ok(),
                "the trusted-network mode allows {required:?}"
            );
        }
    }

    #[test]
    fn authorization_needs_a_grant_while_authentication_is_enforced() {
        let (_directory, auth) = state(true);
        let principal =
            extract(&auth, Some(&format!("Bearer {}", token("u1", false)))).expect("a valid token");
        assert!(matches!(
            authorize(&auth, &principal, "p1.json", Role::Viewer),
            Err(DomainError::Forbidden(_))
        ));

        auth.store
            .set_role("p1.json", "u1", Role::Viewer)
            .expect("grant");
        assert!(authorize(&auth, &principal, "p1.json", Role::Viewer).is_ok());
        assert!(matches!(
            authorize(&auth, &principal, "p1.json", Role::Editor),
            Err(DomainError::Forbidden(_))
        ));
        assert!(
            authorize(&auth, &principal, "elsewhere.json", Role::Viewer).is_err(),
            "a grant belongs to one project"
        );
    }

    #[test]
    fn a_system_administrator_is_authorized_without_any_grant() {
        let (_directory, auth) = state(true);
        let admin = extract(&auth, Some(&format!("Bearer {}", token("root", true))))
            .expect("a valid token");
        assert!(authorize(&auth, &admin, "p1.json", Role::Owner).is_ok());
    }

    #[test]
    fn a_password_hash_is_not_an_identity() {
        // Guards against an accidental coupling: the store's credential hashes
        // live in the same tree as the grants, but nothing on this path reads
        // them, so a request that presents a hash as its token is refused.
        let (_directory, auth) = state(true);
        let digest = hash_password("correct horse battery staple").expect("hash");
        let error = extract(&auth, Some(&format!("Bearer {digest}"))).expect_err("a hash");
        assert_eq!(code(&error), "invalid_token");
    }
}
