//! The rules an authenticated session obeys: sign in, rotate, sign out.
//!
//! This is the seam between [`AuthStore`], which owns the disk, and the HTTP
//! layer, which owns the wire. Everything here is a pure function of its
//! arguments: the store and the settings are borrowed, the instant is a
//! parameter, and the answer is a [`DomainError`] the API can already render.
//! Nothing in this module reads the clock or touches the network, so a token's
//! whole life — issued, rotated, replayed, expired — is exercised directly.
//!
//! Three properties are load-bearing and are why the code is shaped as it is:
//!
//! - **One answer for a bad sign-in.** An unknown username still pays for an
//!   Argon2 verification, against a decoy hash, and returns the same
//!   `invalid_credentials` a wrong password does. Otherwise the reply time, or
//!   the reply itself, would tell an attacker which usernames exist.
//! - **Rotation is a revocation.** A refresh token is removed before its
//!   replacement is written, so a stolen token that is replayed after the
//!   legitimate client used it finds nothing. A crash between the two leaves
//!   the account signed out, which is the safe direction to fail.
//! - **Authority is a rank.** A grant is a [`Role`], and a request is allowed
//!   when the role the caller holds is at least the role it needs, so the
//!   comparison is written once instead of naming each acceptable role.

use crate::domain::DomainError;

use super::{
    AuthConfig, AuthStore, Role, StoredRefreshToken, TokenError, hash_refresh_token,
    mint_access_token, mint_refresh_token, random_id, verify_access_token, verify_password,
};

/// A valid Argon2id PHC string that no live account holds a credential for.
///
/// It exists so that signing in as a username that does not exist costs the
/// same as signing in as one that does: the verification runs either way, and
/// the reply says the same thing either way. Its salt and digest are zero, so
/// it is recognisable as a decoy and can never be reached by a real password by
/// accident. Its cost parameters match [`super::hash_password`] so the work
/// spent is comparable.
const DECOY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// The caller behind one request, once their access token has been verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// The account identifier the access token's `sub` named.
    pub user_id: String,
    /// The login name, for a reply that names the caller.
    pub username: String,
    /// Whether the account may act outside the projects granted to it.
    pub system_admin: bool,
}

impl Principal {
    /// Refuses the request unless the caller holds `required` in `project_id`.
    ///
    /// A system administrator passes every check without a grant; everyone else
    /// needs a role at least as strong as the one asked for. A missing grant and
    /// a grant that is too weak answer the same way, so the reply never maps out
    /// the grants of a project.
    ///
    /// # Errors
    ///
    /// [`DomainError::Forbidden`] when the caller is not entitled, and whatever
    /// reading the grants failed with when the store cannot be read.
    pub fn require_role(
        &self,
        store: &AuthStore,
        project_id: &str,
        required: Role,
    ) -> Result<(), DomainError> {
        if self.system_admin {
            return Ok(());
        }
        match store.role_of(project_id, &self.user_id)? {
            Some(held) if held >= required => Ok(()),
            _ => Err(DomainError::forbidden(format!(
                "This account needs the {} role in the project",
                role_name(required)
            ))),
        }
    }
}

/// The pair a successful sign-in or rotation hands back.
///
/// The wire shape of this is the API layer's business; here it is only the
/// tokens and the instants they stop being valid, so that a caller — and a test
/// — can reason about the session without the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTokens {
    /// The short-lived HS256 token to present on every request.
    pub access_token: String,
    /// The opaque token to present in exchange for the next pair.
    pub refresh_token: String,
    /// Unix seconds the access token stops being valid.
    pub access_expires_at: u64,
    /// Unix seconds the refresh token stops being valid.
    pub refresh_expires_at: u64,
}

/// Verifies a username and password and starts a session.
///
/// A username that does not exist and a password that does not match both
/// answer [`DomainError::invalid_credentials`], after the same amount of work.
/// The expired refresh tokens of a returning account are dropped on the way
/// through, so a client that signs in often does not accumulate dead records.
///
/// # Errors
///
/// [`DomainError::invalid_credentials`] for both halves of a failed sign-in,
/// [`DomainError::Internal`] when the server holds no signing secret, and
/// whatever reading or writing the store failed with.
pub fn login(
    store: &AuthStore,
    config: &AuthConfig,
    username: &str,
    password: &str,
    now: u64,
) -> Result<SessionTokens, DomainError> {
    let secret = secret(config)?;
    let user = store.user_by_username(username)?;
    let candidate = user
        .as_ref()
        .map_or(DECOY_HASH, |account| account.password_hash.as_str());
    // Verified before the account is unwrapped, so an unknown username pays the
    // Argon2 cost too.
    let verified = verify_password(candidate, password);
    let Some(user) = user else {
        return Err(DomainError::invalid_credentials());
    };
    if !verified {
        return Err(DomainError::invalid_credentials());
    }

    store.prune_expired_refresh_tokens(now)?;
    issue_tokens(store, config, &user.id, secret, now)
}

/// Exchanges a refresh token for a fresh pair, revoking the one presented.
///
/// # Errors
///
/// [`DomainError::invalid_refresh_token`] when the token is unknown, already
/// spent, or expired — the last of which also removes the dead record —
/// [`DomainError::Internal`] when the server holds no signing secret, and
/// whatever reading or writing the store failed with.
pub fn refresh(
    store: &AuthStore,
    config: &AuthConfig,
    refresh_token: &str,
    now: u64,
) -> Result<SessionTokens, DomainError> {
    let secret = secret(config)?;
    let digest = hash_refresh_token(refresh_token);
    let Some((user, stored)) = store.user_by_refresh_hash(&digest)? else {
        return Err(DomainError::invalid_refresh_token());
    };
    if stored.expires_at <= now {
        // Refuse, and take the record that can never be used again with us.
        store.revoke_refresh_token(&user.id, &stored.id)?;
        return Err(DomainError::invalid_refresh_token());
    }
    // The spent token goes first, so a replay of it cannot find it, whatever
    // happens next.
    store.revoke_refresh_token(&user.id, &stored.id)?;
    issue_tokens(store, config, &user.id, secret, now)
}

/// Revokes one refresh token of the authenticated caller.
///
/// Reports whether a record was removed, so a caller that wants to say more
/// than 204 can. A token that is unknown, expired, or another account's
/// revokes nothing and reports `false`: the endpoint answers the same either
/// way, so it never confirms whether a token existed. Expiry is not consulted
/// because dropping an expired record is the same cleanup as dropping a live
/// one.
///
/// # Errors
///
/// Whatever reading or writing the store failed with.
pub fn logout(
    store: &AuthStore,
    principal: &Principal,
    refresh_token: &str,
) -> Result<bool, DomainError> {
    let digest = hash_refresh_token(refresh_token);
    let Some((user, stored)) = store.user_by_refresh_hash(&digest)? else {
        return Ok(false);
    };
    if user.id != principal.user_id {
        return Ok(false);
    }
    store.revoke_refresh_token(&user.id, &stored.id)?;
    Ok(true)
}

/// Verifies a bearer access token and resolves it to the account it names.
///
/// The account is looked up again on every request rather than trusted from the
/// claims, so deleting an account ends its sessions immediately instead of when
/// its tokens happen to expire.
///
/// # Errors
///
/// [`DomainError::token_expired`] when the token was well-formed but has
/// passed, [`DomainError::invalid_token`] when it is malformed, badly signed,
/// or names an account that no longer exists, [`DomainError::Internal`] when
/// the server holds no signing secret, and whatever reading the store failed
/// with.
pub fn authenticate(
    store: &AuthStore,
    config: &AuthConfig,
    access_token: &str,
    now: u64,
) -> Result<Principal, DomainError> {
    let secret = secret(config)?;
    let claims = verify_access_token(secret, access_token, now).map_err(|error| match error {
        TokenError::Expired => DomainError::token_expired(),
        TokenError::Malformed | TokenError::BadSignature => DomainError::invalid_token(),
    })?;
    let Some(user) = store.user(&claims.subject)? else {
        return Err(DomainError::invalid_token());
    };
    Ok(Principal {
        user_id: user.id,
        username: user.username,
        system_admin: user.system_admin,
    })
}

/// Mints a pair for `user_id` and records the refresh half.
fn issue_tokens(
    store: &AuthStore,
    config: &AuthConfig,
    user_id: &str,
    secret: &[u8],
    now: u64,
) -> Result<SessionTokens, DomainError> {
    let access_token = mint_access_token(secret, user_id, config.access_ttl, now);
    let refresh_token = mint_refresh_token();
    let refresh_expires_at = now.saturating_add(config.refresh_ttl.as_secs());
    store.insert_refresh_token(
        user_id,
        &StoredRefreshToken {
            id: random_id(),
            hash: hash_refresh_token(&refresh_token),
            expires_at: refresh_expires_at,
        },
    )?;
    Ok(SessionTokens {
        access_token,
        refresh_token,
        access_expires_at: now.saturating_add(config.access_ttl.as_secs()),
        refresh_expires_at,
    })
}

/// The signing secret, or an internal error naming the misconfiguration.
///
/// [`AuthConfig`] refuses to resolve without a secret once auth is required, so
/// reaching this means the server was assembled outside its own configuration
/// path — which is a failure to report, not a credential to reject.
fn secret(config: &AuthConfig) -> Result<&[u8], DomainError> {
    config
        .jwt_secret
        .as_deref()
        .ok_or_else(|| DomainError::Internal("auth is not configured with a signing secret".into()))
}

/// The name a role is spelled with in a reply.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::Viewer => "viewer",
        Role::Editor => "editor",
        Role::Owner => "owner",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{User, hash_password};
    use std::time::Duration;
    use tempfile::TempDir;

    const NOW: u64 = 1_700_000_000;
    const PASSWORD: &str = "correct horse battery staple";
    const SECRET: &[u8] = b"an-integration-test-secret-of-32-plus!";

    fn config() -> AuthConfig {
        AuthConfig {
            required: true,
            jwt_secret: Some(SECRET.to_vec()),
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(1_209_600),
            bootstrap_username: None,
            bootstrap_password: None,
        }
    }

    fn account(id: &str, username: &str) -> User {
        User {
            id: id.to_owned(),
            username: username.to_owned(),
            password_hash: hash_password(PASSWORD).expect("hash"),
            system_admin: false,
            created_at: NOW,
            refresh_tokens: Vec::new(),
        }
    }

    fn seeded() -> (TempDir, AuthStore) {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AuthStore::new(directory.path()).expect("store");
        store
            .insert_user(&account("u1", "alice"))
            .expect("insert alice");
        (directory, store)
    }

    fn code(error: &DomainError) -> &str {
        match error {
            DomainError::Unauthenticated { code, .. } => code,
            other => panic!("expected an unauthenticated error, got {other:?}"),
        }
    }

    fn refresh_token_count(store: &AuthStore, user_id: &str) -> usize {
        store
            .user(user_id)
            .expect("read")
            .expect("present")
            .refresh_tokens
            .len()
    }

    #[test]
    fn the_decoy_hash_parses_and_never_verifies() {
        assert!(
            argon2::password_hash::PasswordHash::new(DECOY_HASH).is_ok(),
            "the decoy must parse, or an unknown username would skip the work it is there to spend"
        );
        assert!(!verify_password(DECOY_HASH, PASSWORD));
        assert!(!verify_password(DECOY_HASH, ""));
    }

    #[test]
    fn signing_in_issues_a_verifiable_access_token_and_a_stored_refresh_token() {
        let (_directory, store) = seeded();
        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let claims = verify_access_token(SECRET, &tokens.access_token, NOW).expect("verify");
        assert_eq!(claims.subject, "u1");
        assert_eq!(claims.issued_at, NOW);
        assert_eq!(claims.expires_at, NOW + 900);
        assert_eq!(tokens.access_expires_at, NOW + 900);
        assert_eq!(tokens.refresh_expires_at, NOW + 1_209_600);

        let digest = hash_refresh_token(&tokens.refresh_token);
        let (owner, stored) = store
            .user_by_refresh_hash(&digest)
            .expect("lookup")
            .expect("stored");
        assert_eq!(owner.id, "u1");
        assert_eq!(stored.expires_at, NOW + 1_209_600);
        assert_ne!(
            stored.hash, tokens.refresh_token,
            "only the digest is kept, never the token"
        );
        assert_eq!(refresh_token_count(&store, "u1"), 1);
    }

    #[test]
    fn signing_in_compares_the_username_whatever_its_case() {
        let (_directory, store) = seeded();
        assert!(login(&store, &config(), "ALICE", PASSWORD, NOW).is_ok());
    }

    #[test]
    fn a_wrong_password_is_refused_and_starts_no_session() {
        let (_directory, store) = seeded();
        let error = login(&store, &config(), "alice", "wrong", NOW).expect_err("wrong password");
        assert_eq!(code(&error), "invalid_credentials");
        assert_eq!(refresh_token_count(&store, "u1"), 0);
    }

    #[test]
    fn an_unknown_username_answers_exactly_as_a_wrong_password_does() {
        let (_directory, store) = seeded();
        let unknown = login(&store, &config(), "nobody", PASSWORD, NOW).expect_err("unknown");
        let wrong = login(&store, &config(), "alice", "wrong", NOW).expect_err("wrong");
        assert_eq!(code(&unknown), code(&wrong));
        assert_eq!(unknown.to_string(), wrong.to_string());
    }

    #[test]
    fn signing_in_drops_the_expired_refresh_tokens_of_a_returning_account() {
        let (_directory, store) = seeded();
        store
            .insert_refresh_token(
                "u1",
                &StoredRefreshToken {
                    id: "dead".to_owned(),
                    hash: "digest-of-dead".to_owned(),
                    expires_at: NOW - 1,
                },
            )
            .expect("insert dead");
        store
            .insert_refresh_token(
                "u1",
                &StoredRefreshToken {
                    id: "live".to_owned(),
                    hash: "digest-of-live".to_owned(),
                    expires_at: NOW + 1,
                },
            )
            .expect("insert live");

        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let stored = store.user("u1").expect("read").expect("present");
        assert_eq!(
            stored.refresh_tokens.len(),
            2,
            "the live one, plus the new one"
        );
        assert!(
            stored.refresh_tokens.iter().all(|token| token.id != "dead"),
            "the expired one is gone"
        );
        assert!(
            store
                .user_by_refresh_hash(&hash_refresh_token(&tokens.refresh_token))
                .expect("lookup")
                .is_some(),
            "the new one is there"
        );
    }

    #[test]
    fn rotating_a_refresh_token_replaces_it() {
        let (_directory, store) = seeded();
        let first = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let second = refresh(&store, &config(), &first.refresh_token, NOW + 10).expect("refresh");

        assert_ne!(second.refresh_token, first.refresh_token);
        let spent = hash_refresh_token(&first.refresh_token);
        assert!(
            store
                .user_by_refresh_hash(&spent)
                .expect("lookup")
                .is_none(),
            "the spent token is gone"
        );
        let fresh = hash_refresh_token(&second.refresh_token);
        let (owner, stored) = store
            .user_by_refresh_hash(&fresh)
            .expect("lookup")
            .expect("stored");
        assert_eq!(owner.id, "u1");
        assert_eq!(stored.expires_at, NOW + 10 + 1_209_600);
        assert_eq!(
            refresh_token_count(&store, "u1"),
            1,
            "a rotation is not a copy"
        );

        let claims = verify_access_token(SECRET, &second.access_token, NOW + 10).expect("verify");
        assert_eq!(claims.subject, "u1", "the session stays the same account");
        assert_eq!(claims.expires_at, NOW + 10 + 900);
        assert_ne!(
            second.access_token, first.access_token,
            "each pair carries a new access token as well"
        );
    }

    #[test]
    fn a_replayed_refresh_token_is_refused() {
        let (_directory, store) = seeded();
        let first = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        refresh(&store, &config(), &first.refresh_token, NOW + 10).expect("rotate");
        let error = refresh(&store, &config(), &first.refresh_token, NOW + 20)
            .expect_err("replay of a spent token");
        assert_eq!(code(&error), "invalid_refresh_token");
    }

    #[test]
    fn an_unknown_refresh_token_is_refused() {
        let (_directory, store) = seeded();
        let error = refresh(&store, &config(), "no-such-token", NOW).expect_err("unknown");
        assert_eq!(code(&error), "invalid_refresh_token");
    }

    #[test]
    fn an_expired_refresh_token_is_refused_and_removed() {
        let (_directory, store) = seeded();
        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let digest = hash_refresh_token(&tokens.refresh_token);
        let (_, stored) = store
            .user_by_refresh_hash(&digest)
            .expect("lookup")
            .expect("stored");
        assert_eq!(stored.expires_at, NOW + 1_209_600);

        let error = refresh(&store, &config(), &tokens.refresh_token, stored.expires_at)
            .expect_err("expired");
        assert_eq!(code(&error), "invalid_refresh_token", "exp is exclusive");
        assert!(
            store
                .user_by_refresh_hash(&digest)
                .expect("lookup")
                .is_none(),
            "the dead record is not left behind"
        );
    }

    #[test]
    fn authenticating_returns_the_account_behind_the_token() {
        let (_directory, store) = seeded();
        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let principal =
            authenticate(&store, &config(), &tokens.access_token, NOW).expect("authenticate");
        assert_eq!(principal.user_id, "u1");
        assert_eq!(principal.username, "alice");
        assert!(!principal.system_admin);
    }

    #[test]
    fn an_expired_access_token_reports_its_own_code() {
        let (_directory, store) = seeded();
        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let error =
            authenticate(&store, &config(), &tokens.access_token, NOW + 900).expect_err("expired");
        assert_eq!(code(&error), "token_expired");
    }

    #[test]
    fn a_malformed_or_foreignly_signed_access_token_reports_invalid_token() {
        let (_directory, store) = seeded();
        let mut other = config();
        other.jwt_secret = Some(b"a-different-signing-secret-32-plus!!!".to_vec());
        let tokens = login(&store, &other, "alice", PASSWORD, NOW).expect("login");
        for token in ["", "not.a.token", tokens.access_token.as_str()] {
            let error = authenticate(&store, &config(), token, NOW).expect_err(token);
            assert_eq!(code(&error), "invalid_token", "{token:?}");
        }
    }

    #[test]
    fn a_token_for_an_account_that_no_longer_exists_is_refused() {
        let (_directory, store) = seeded();
        // A token that is correctly signed for an account this store has never
        // held. Trusting the claims would accept it; resolving them would not.
        let ghost = mint_access_token(SECRET, "ghost", Duration::from_secs(900), NOW);
        let error = authenticate(&store, &config(), &ghost, NOW).expect_err("no such account");
        assert_eq!(code(&error), "invalid_token");
    }

    #[test]
    fn a_server_without_a_signing_secret_reports_an_internal_error() {
        let (_directory, store) = seeded();
        let mut unconfigured = config();
        unconfigured.jwt_secret = None;
        assert!(matches!(
            login(&store, &unconfigured, "alice", PASSWORD, NOW),
            Err(DomainError::Internal(_))
        ));
        assert!(matches!(
            authenticate(&store, &unconfigured, "any.token.at-all", NOW),
            Err(DomainError::Internal(_))
        ));
    }

    #[test]
    fn a_role_is_allowed_when_it_ranks_at_least_as_high_as_the_one_required() {
        let (_directory, store) = seeded();
        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let principal =
            authenticate(&store, &config(), &tokens.access_token, NOW).expect("authenticate");

        store
            .set_role("p1.json", "u1", Role::Viewer)
            .expect("grant");
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Viewer)
                .is_ok()
        );
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Editor)
                .is_err(),
            "a viewer cannot edit"
        );

        store
            .set_role("p1.json", "u1", Role::Editor)
            .expect("grant");
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Viewer)
                .is_ok()
        );
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Editor)
                .is_ok()
        );
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Owner)
                .is_err(),
            "an editor cannot own"
        );

        store.set_role("p1.json", "u1", Role::Owner).expect("grant");
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Owner)
                .is_ok()
        );
    }

    #[test]
    fn a_caller_with_no_grant_is_forbidden() {
        let (_directory, store) = seeded();
        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let principal =
            authenticate(&store, &config(), &tokens.access_token, NOW).expect("authenticate");
        assert!(matches!(
            principal.require_role(&store, "p1.json", Role::Viewer),
            Err(DomainError::Forbidden(_))
        ));
    }

    #[test]
    fn a_system_administrator_passes_every_check_without_a_grant() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AuthStore::new(directory.path()).expect("store");
        let mut admin = account("u9", "root");
        admin.system_admin = true;
        store.insert_user(&admin).expect("insert");
        let tokens = login(&store, &config(), "root", PASSWORD, NOW).expect("login");
        let principal =
            authenticate(&store, &config(), &tokens.access_token, NOW).expect("authenticate");
        assert!(principal.system_admin);
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Owner)
                .is_ok()
        );
    }

    #[test]
    fn roles_are_ordered_least_privilege_first() {
        assert!(Role::Viewer < Role::Editor);
        assert!(Role::Editor < Role::Owner);
    }

    #[test]
    fn signing_out_revokes_the_callers_refresh_token_once() {
        let (_directory, store) = seeded();
        let tokens = login(&store, &config(), "alice", PASSWORD, NOW).expect("login");
        let principal =
            authenticate(&store, &config(), &tokens.access_token, NOW).expect("authenticate");
        assert!(logout(&store, &principal, &tokens.refresh_token).expect("logout"));
        assert!(
            store
                .user_by_refresh_hash(&hash_refresh_token(&tokens.refresh_token))
                .expect("lookup")
                .is_none()
        );
        assert!(
            !logout(&store, &principal, &tokens.refresh_token).expect("logout again"),
            "signing out twice is not an error, and reports nothing the second time"
        );
    }

    #[test]
    fn signing_out_does_not_revoke_another_accounts_token() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AuthStore::new(directory.path()).expect("store");
        store.insert_user(&account("u1", "alice")).expect("alice");
        store.insert_user(&account("u2", "bob")).expect("bob");
        let alice = login(&store, &config(), "alice", PASSWORD, NOW).expect("login alice");
        let bob = login(&store, &config(), "bob", PASSWORD, NOW).expect("login bob");
        let principal =
            authenticate(&store, &config(), &alice.access_token, NOW).expect("authenticate");

        assert!(!logout(&store, &principal, &bob.refresh_token).expect("logout"));
        assert!(
            store
                .user_by_refresh_hash(&hash_refresh_token(&bob.refresh_token))
                .expect("lookup")
                .is_some(),
            "bob's session is untouched"
        );
    }
}
