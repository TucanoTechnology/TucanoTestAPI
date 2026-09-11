//! The first account a deployment starts with.
//!
//! An API that enforces auth is unusable until it has an account, and there is
//! no bootstrapping account to log in as. This is the one place that closes
//! that loop: at startup, if the store holds no accounts and the operator
//! configured bootstrap credentials, the first account is created from them.
//!
//! The account is a system administrator because it is the only account there
//! is: there are no grants to give it, so nothing else would let it in. It is
//! created once — as soon as any account exists this does nothing — so changing
//! the bootstrap variables later cannot quietly mint a second administrator.
//!
//! Refusing to start is deliberate. A server that enforces auth with no
//! accounts and no way to make one could only ever answer 401, so it fails at
//! startup with a message naming what to set, rather than coming up unusable.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use super::{AuthConfig, AuthStore, HashError, User, hash_password, random_id};

/// A deployment that cannot be given its first account.
#[derive(Debug)]
pub enum BootstrapError {
    /// The account could not be created.
    Hash(HashError),
    /// The store could not be read or written.
    Storage(io::Error),
    /// Auth is required, the store is empty, and no bootstrap credentials exist.
    MissingCredentials,
}

impl Display for BootstrapError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hash(error) => write!(formatter, "cannot hash the bootstrap password: {error}"),
            Self::Storage(error) => {
                write!(formatter, "cannot prepare the bootstrap account: {error}")
            }
            Self::MissingCredentials => formatter.write_str(
                "TUCANO_AUTH_REQUIRED is set and no accounts exist, so \
                 TUCANO_BOOTSTRAP_USERNAME and TUCANO_BOOTSTRAP_PASSWORD must be too",
            ),
        }
    }
}

impl Error for BootstrapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Hash(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::MissingCredentials => None,
        }
    }
}

impl From<HashError> for BootstrapError {
    fn from(error: HashError) -> Self {
        Self::Hash(error)
    }
}

impl From<io::Error> for BootstrapError {
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

/// Creates the first account from the bootstrap credentials, if that is needed.
///
/// Returns the username of the account that was created, or `None` when the
/// store already held an account or no credentials were configured. `now` is
/// the creation instant, in Unix seconds, as everywhere else in this module.
///
/// # Errors
///
/// [`BootstrapError::MissingCredentials`] when auth is required, the store is
/// empty, and no credentials were configured; the other variants when the
/// account could not be stored.
pub fn ensure_bootstrap_user(
    store: &AuthStore,
    config: &AuthConfig,
    now: u64,
) -> Result<Option<String>, BootstrapError> {
    if !store.users()?.is_empty() {
        return Ok(None);
    }
    let (Some(username), Some(password)) = (
        config.bootstrap_username.as_ref(),
        config.bootstrap_password.as_ref(),
    ) else {
        return if config.required {
            Err(BootstrapError::MissingCredentials)
        } else {
            // Nothing enforces auth, so an empty store is a valid starting
            // point rather than a deployment that cannot work.
            Ok(None)
        };
    };

    store.insert_user(&User {
        id: random_id(),
        username: username.clone(),
        password_hash: hash_password(password)?,
        system_admin: true,
        created_at: now,
        refresh_tokens: Vec::new(),
    })?;
    Ok(Some(username.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{Role, authenticate, login, verify_password};
    use std::time::Duration;
    use tempfile::TempDir;

    const NOW: u64 = 1_700_000_000;
    const SECRET: &[u8] = b"an-integration-test-secret-of-32-plus!";
    const PASSWORD: &str = "the-first-password";

    fn store() -> (TempDir, AuthStore) {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AuthStore::new(directory.path()).expect("store");
        (directory, store)
    }

    fn config(required: bool, credentials: bool) -> AuthConfig {
        AuthConfig {
            required,
            jwt_secret: Some(SECRET.to_vec()),
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(1_209_600),
            bootstrap_username: credentials.then(|| "root".to_owned()),
            bootstrap_password: credentials.then(|| PASSWORD.to_owned()),
        }
    }

    #[test]
    fn the_first_account_is_created_from_the_bootstrap_credentials() {
        let (_directory, store) = store();
        let created = ensure_bootstrap_user(&store, &config(true, true), NOW).expect("bootstrap");
        assert_eq!(created.as_deref(), Some("root"));

        let account = store
            .user_by_username("root")
            .expect("read")
            .expect("present");
        assert!(
            account.system_admin,
            "the only account must be able to administer"
        );
        assert_eq!(account.created_at, NOW);
        assert!(!account.id.is_empty());
        assert!(verify_password(&account.password_hash, PASSWORD));
        assert!(!verify_password(&account.password_hash, "another password"));
        assert_eq!(store.users().expect("users").len(), 1);
    }

    #[test]
    fn the_created_account_can_sign_in_and_act_as_an_owner_everywhere() {
        let (_directory, store) = store();
        ensure_bootstrap_user(&store, &config(true, true), NOW).expect("bootstrap");
        let tokens = login(&store, &config(true, true), "ROOT", PASSWORD, NOW).expect("login");
        let principal =
            authenticate(&store, &config(true, true), &tokens.access_token, NOW).expect("auth");
        assert_eq!(principal.username, "root");
        assert!(
            principal
                .require_role(&store, "p1.json", Role::Owner)
                .is_ok(),
            "with no grants to hand out, the first account answers for everything"
        );
    }

    #[test]
    fn a_store_that_already_has_an_account_is_left_alone() {
        let (_directory, store) = store();
        ensure_bootstrap_user(&store, &config(true, true), NOW).expect("bootstrap");
        let before = store.users().expect("users");

        let created = ensure_bootstrap_user(&store, &config(true, true), NOW + 1).expect("again");
        assert_eq!(created, None);
        assert_eq!(
            before,
            store.users().expect("users"),
            "a second start changes nothing, not even the hash"
        );
    }

    #[test]
    fn an_enforcing_server_with_no_accounts_and_no_credentials_refuses_to_start() {
        let (_directory, store) = store();
        let error = ensure_bootstrap_user(&store, &config(true, false), NOW).expect_err("refused");
        assert!(matches!(error, BootstrapError::MissingCredentials));
        assert!(error.to_string().contains("TUCANO_BOOTSTRAP_USERNAME"));
        assert!(store.users().expect("users").is_empty());
    }

    #[test]
    fn a_server_that_does_not_enforce_auth_may_start_with_no_accounts() {
        let (_directory, store) = store();
        assert_eq!(
            ensure_bootstrap_user(&store, &config(false, false), NOW).expect("quiet"),
            None
        );
        assert!(store.users().expect("users").is_empty());
    }

    #[test]
    fn credentials_are_created_even_when_auth_is_not_yet_enforced() {
        let (_directory, store) = store();
        assert_eq!(
            ensure_bootstrap_user(&store, &config(false, true), NOW)
                .expect("created")
                .as_deref(),
            Some("root"),
            "an operator who configured credentials gets the account"
        );
        assert!(store.user_by_username("root").expect("read").is_some());
    }
}
