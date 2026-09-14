//! Seeding the demo accounts and grants the seed dataset needs.
//!
//! The seed dataset (see `docs/testing/seed-dataset-spec.md` §5) wants an
//! account that is *not* a system administrator and that holds a role on two
//! projects, because the seeded data is only meaningful when a reader can see
//! the difference between "administers the server" and "may change one
//! project". The API publishes no route that creates an account or writes a
//! grant — accounts are created by signing up against a store an operator
//! controls — so the seed script cannot drive it over HTTP. This module is the
//! one exception: the same binary that serves the API also writes those files
//! through the very store the server reads, so the result is a state the API
//! itself would accept rather than a hand-authored `users.json`.
//!
//! Idempotence is per-account and per-grant. Seeding an account that already
//! exists leaves its password alone and only fills in the grants that are
//! missing, so re-running the seed against a volume it has already touched
//! changes nothing instead of failing on a taken username.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use super::{AuthStore, HashError, Role, User, hash_password, random_id};

/// A seeding request that cannot be honoured.
#[derive(Debug)]
pub enum SeedError {
    /// A password could not be hashed, or an account could not be created.
    Storage(io::Error),
    /// The password was rejected by the hasher (for example it is empty).
    Hash(HashError),
    /// A role name is not one the API defines.
    UnknownRole(String),
}

impl Display for SeedError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "cannot write the seeded account: {error}"),
            Self::Hash(error) => write!(formatter, "cannot hash the seeded password: {error}"),
            Self::UnknownRole(role) => write!(
                formatter,
                "unknown role {role:?}: expected one of viewer, editor, owner"
            ),
        }
    }
}

impl Error for SeedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Hash(error) => Some(error),
            Self::UnknownRole(_) => None,
        }
    }
}

impl From<io::Error> for SeedError {
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<HashError> for SeedError {
    fn from(error: HashError) -> Self {
        Self::Hash(error)
    }
}

/// One account to create and the roles it should hold.
#[derive(Debug, Clone)]
pub struct AccountSpec {
    /// Login name; compared case-insensitively, as everywhere else.
    pub username: String,
    /// Plain password. Hashed before it reaches the store.
    pub password: String,
    /// Whether the account may act beyond the projects granted to it.
    pub system_admin: bool,
    /// `(project_id, role)` pairs to record. Roles use the wire spelling.
    pub grants: Vec<(String, String)>,
}

/// The account seeding left behind, so a caller can report what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededAccount {
    /// Identifier of the account, whether it was created now or already there.
    pub id: String,
    /// Username as it is stored.
    pub username: String,
    /// Whether this call created the account (`false` when it already existed).
    pub created: bool,
    /// `(project_id, role)` pairs that were absent and are now recorded.
    pub grants_added: Vec<(String, String)>,
}

/// Parses the wire spelling of a [`Role`].
///
/// # Errors
///
/// [`SeedError::UnknownRole`] when the name is not one the API defines.
pub fn parse_role(name: &str) -> Result<Role, SeedError> {
    match name.trim().to_ascii_lowercase().as_str() {
        "viewer" => Ok(Role::Viewer),
        "editor" => Ok(Role::Editor),
        "owner" => Ok(Role::Owner),
        _ => Err(SeedError::UnknownRole(name.to_owned())),
    }
}

/// Creates `spec`'s account when it is missing and records any grants it lacks.
///
/// An account that already exists is left exactly as it is — its password hash
/// is not rewritten — and only the grants named in `spec` that it does not hold
/// yet are added. A grant it already holds with a different role is upgraded or
/// downgraded to the requested one, because the spec is the authority on what
/// the account should hold, not whatever a previous run happened to write.
///
/// # Errors
///
/// [`SeedError::UnknownRole`] for an unusable role name, and the store variants
/// when the account or a grant cannot be written.
pub fn seed_account(store: &AuthStore, spec: &AccountSpec) -> Result<SeededAccount, SeedError> {
    let (id, username, created) = match store.user_by_username(&spec.username)? {
        Some(existing) => (existing.id, existing.username, false),
        None => {
            let id = random_id();
            store.insert_user(&User {
                id: id.clone(),
                username: spec.username.clone(),
                password_hash: hash_password(&spec.password)?,
                system_admin: spec.system_admin,
                created_at: now_seconds(),
                refresh_tokens: Vec::new(),
            })?;
            (id, spec.username.clone(), true)
        }
    };

    let mut grants_added = Vec::new();
    for (project_id, role_name) in &spec.grants {
        let role = parse_role(role_name)?;
        if store.role_of(project_id, &id)? != Some(role) {
            store.set_role(project_id, &id, role)?;
            grants_added.push((project_id.clone(), role_name.trim().to_ascii_lowercase()));
        }
    }

    Ok(SeededAccount {
        id,
        username,
        created,
        grants_added,
    })
}

/// Seconds since the Unix epoch, matching how the bootstrap account is stamped.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::verify_password;
    use tempfile::TempDir;

    const PASSWORD: &str = "viewer-seed-password";

    fn store() -> (TempDir, AuthStore) {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AuthStore::new(directory.path()).expect("store");
        (directory, store)
    }

    fn spec() -> AccountSpec {
        AccountSpec {
            username: "viewer".to_owned(),
            password: PASSWORD.to_owned(),
            system_admin: false,
            grants: vec![
                ("checkout.json".to_owned(), "owner".to_owned()),
                ("payments.json".to_owned(), "owner".to_owned()),
            ],
        }
    }

    #[test]
    fn creates_an_account_and_records_every_grant() {
        let (_directory, store) = store();
        let seeded = seed_account(&store, &spec()).expect("seed");

        assert!(seeded.created);
        assert_eq!(seeded.username, "viewer");
        assert_eq!(
            seeded.grants_added,
            vec![
                ("checkout.json".to_owned(), "owner".to_owned()),
                ("payments.json".to_owned(), "owner".to_owned()),
            ]
        );

        let account = store
            .user_by_username("viewer")
            .expect("read")
            .expect("present");
        assert!(
            !account.system_admin,
            "the seeded viewer must not administer"
        );
        assert!(verify_password(&account.password_hash, PASSWORD));
        assert_eq!(account.id, seeded.id);
        assert_eq!(
            store.role_of("checkout.json", &account.id).expect("role"),
            Some(Role::Owner)
        );
        assert_eq!(
            store.role_of("payments.json", &account.id).expect("role"),
            Some(Role::Owner)
        );
        // The grants file is named after the project folder, with no suffix.
        assert!(store.grants("payments.json").expect("grants").grants.len() == 1);
    }

    #[test]
    fn seeding_twice_changes_nothing_and_keeps_the_password() {
        let (_directory, store) = store();
        let first = seed_account(&store, &spec()).expect("first");
        let before = store
            .user_by_username("viewer")
            .expect("read")
            .expect("present");

        let second = seed_account(&store, &spec()).expect("second");
        assert!(!second.created);
        assert!(second.grants_added.is_empty(), "grants are already held");
        assert_eq!(second.id, first.id);

        let after = store
            .user_by_username("viewer")
            .expect("read")
            .expect("present");
        assert_eq!(before, after, "a second run does not rewrite the account");
    }

    #[test]
    fn seeding_twice_tolerates_a_reordered_grant_list() {
        let (_directory, store) = store();
        seed_account(&store, &spec()).expect("first");

        let mut reordered = spec();
        reordered.grants.reverse();
        let second = seed_account(&store, &reordered).expect("second");
        assert!(second.grants_added.is_empty());
    }

    #[test]
    fn a_missing_grant_is_added_without_touching_the_account() {
        let (_directory, store) = store();
        seed_account(&store, &spec()).expect("first");

        let mut extended = spec();
        extended
            .grants
            .push(("extra.json".to_owned(), "viewer".to_owned()));
        let seeded = seed_account(&store, &extended).expect("second");
        assert_eq!(
            seeded.grants_added,
            vec![("extra.json".to_owned(), "viewer".to_owned())]
        );
        assert_eq!(
            store.role_of("extra.json", &seeded.id).expect("role"),
            Some(Role::Viewer)
        );
    }

    #[test]
    fn a_grant_held_at_the_wrong_role_is_replaced() {
        let (_directory, store) = store();
        let account = store.user_by_username("viewer").expect("read").is_none();
        assert!(account, "no account yet");

        let mut viewer_first = spec();
        viewer_first.grants = vec![("checkout.json".to_owned(), "viewer".to_owned())];
        let seeded = seed_account(&store, &viewer_first).expect("first");
        assert_eq!(
            store.role_of("checkout.json", &seeded.id).expect("role"),
            Some(Role::Viewer)
        );

        seed_account(&store, &spec()).expect("promote");
        assert_eq!(
            store.role_of("checkout.json", &seeded.id).expect("role"),
            Some(Role::Owner)
        );
    }

    #[test]
    fn the_seeded_account_can_sign_in() {
        let (_directory, store) = store();
        let seeded = seed_account(&store, &spec()).expect("seed");
        let account = store
            .user_by_username(&seeded.username)
            .expect("read")
            .expect("present");
        assert!(verify_password(&account.password_hash, PASSWORD));
        assert!(!account.system_admin);
    }

    #[test]
    fn an_unknown_role_is_rejected_by_name() {
        let error = parse_role("superuser").expect_err("unknown");
        assert!(matches!(error, SeedError::UnknownRole(ref role) if role == "superuser"));
        assert!(error.to_string().contains("superuser"));

        assert_eq!(parse_role("Owner").expect("case-insensitive"), Role::Owner);
        assert_eq!(parse_role(" editor ").expect("trimmed"), Role::Editor);
    }

    #[test]
    fn a_system_administrator_can_be_seeded_when_asked() {
        let (_directory, store) = store();
        let mut spec = spec();
        spec.system_admin = true;
        spec.grants.clear();
        let seeded = seed_account(&store, &spec).expect("seed");
        assert!(seeded.created);
        assert!(seeded.grants_added.is_empty());
        assert!(
            store
                .user_by_username("viewer")
                .expect("read")
                .expect("present")
                .system_admin
        );
    }
}
