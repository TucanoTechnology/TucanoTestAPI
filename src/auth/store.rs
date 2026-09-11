//! On-disk storage for accounts, refresh tokens, and project grants.
//!
//! The API keeps no database, so authentication state is files below the data
//! root exactly like every other resource: `auth/users.json` holds the accounts
//! and the refresh tokens issued to them, and `auth/projects/<project>.json`
//! records the role each account has in one project. A project with no file has
//! no grants, which is why creating a grant file and creating a project stay in
//! step.
//!
//! Two rules shape this module. Every write takes the same `.tucano.lock` the
//! document repository uses, so at most one writer touches the data directory
//! at a time — and because locking one file twice from one process blocks,
//! callers must never nest an [`AuthStore`] write inside a repository write.
//! Nothing here reads the clock either: instants are parameters and expiry is
//! decided by the caller, which is what makes the token lifecycle testable
//! without sleeping.

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::storage::{
    ensure_within, folder_name, folder_wire_id, set_private_permissions, unique_suffix,
    validate_component,
};

/// How much authority an account holds inside one project.
///
/// The variants are declared least privileged first, so a guard can ask for
/// `role >= Role::Editor` rather than naming each acceptable role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// May read the project and everything it owns.
    Viewer,
    /// May read and change the project's content.
    Editor,
    /// May change the content and decide who else can reach it.
    Owner,
}

/// One refresh token, stored as the digest of the secret it was minted from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRefreshToken {
    /// Identifier the client names when it rotates or revokes the token.
    pub id: String,
    /// Hex SHA-256 of the token. The token itself is unrecoverable from here.
    pub hash: String,
    /// Unix seconds after which the token is refused.
    pub expires_at: u64,
}

/// One account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// Stable identifier that grants and access tokens refer to.
    pub id: String,
    /// Login name, compared ASCII-case-insensitively.
    pub username: String,
    /// Argon2id PHC string produced by [`crate::auth::hash_password`].
    pub password_hash: String,
    /// Whether the account may act beyond the projects granted to it.
    pub system_admin: bool,
    /// Unix seconds the account was created.
    pub created_at: u64,
    /// Live refresh tokens, in the order they were issued.
    #[serde(default)]
    pub refresh_tokens: Vec<StoredRefreshToken>,
}

/// The role every account holds in one project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants {
    /// Keyed by account identifier, kept in identifier order so a rewrite of
    /// the file only moves the entry that changed.
    #[serde(default)]
    pub grants: BTreeMap<String, Role>,
}

/// The document `auth/users.json` holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct UserFile {
    #[serde(default)]
    users: Vec<User>,
}

/// Accounts and project grants, stored below the data root.
#[derive(Debug, Clone)]
pub struct AuthStore {
    root: PathBuf,
}

impl AuthStore {
    /// Open the store below `root`, creating the directories it needs.
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let store = Self { root: root.into() };
        fs::create_dir_all(store.grants_dir())?;
        Ok(store)
    }

    fn auth_dir(&self) -> PathBuf {
        self.root.join("auth")
    }

    fn users_path(&self) -> PathBuf {
        self.auth_dir().join("users.json")
    }

    fn grants_dir(&self) -> PathBuf {
        self.auth_dir().join("projects")
    }

    /// Path of the grant document for one project.
    ///
    /// The project is named the way the tree names it: the wire id loses its
    /// `.json` suffix to become a folder name, which is then validated, so a
    /// project identifier can never address a file outside `auth/projects/`.
    fn grant_path(&self, project_id: &str) -> io::Result<PathBuf> {
        let folder = folder_name(project_id);
        validate_component(folder)?;
        let path = self.grants_dir().join(format!("{folder}.json"));
        ensure_within(&self.root, &path)?;
        Ok(path)
    }

    /// Take the single writer lock for the data directory.
    ///
    /// This is the lock file the document repository uses, so an authentication
    /// write and a document write never interleave.
    fn acquire_lock(&self) -> io::Result<File> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".tucano.lock"))?;
        lock.lock_exclusive()?;
        Ok(lock)
    }

    fn read_users_unlocked(&self) -> io::Result<Vec<User>> {
        match fs::read_to_string(self.users_path()) {
            Ok(contents) => {
                let file: UserFile = serde_json::from_str(&contents)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                Ok(file.users)
            }
            // No file yet is no accounts, not a failure.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    fn write_users_unlocked(&self, users: &[User]) -> io::Result<()> {
        let file = UserFile {
            users: users.to_vec(),
        };
        let value = serde_json::to_value(&file)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_json_atomically(&self.users_path(), &value)
    }

    fn read_grants_unlocked(&self, project_id: &str) -> io::Result<Grants> {
        match fs::read_to_string(self.grant_path(project_id)?) {
            Ok(contents) => serde_json::from_str(&contents)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            // A project nobody was granted is a project with no grants, which is
            // also the state a project starts in.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Grants::default()),
            Err(error) => Err(error),
        }
    }

    fn write_grants_unlocked(&self, project_id: &str, grants: &Grants) -> io::Result<()> {
        let value = serde_json::to_value(grants)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_json_atomically(&self.grant_path(project_id)?, &value)
    }

    /// Every stored account, in the order the file lists them.
    pub fn users(&self) -> io::Result<Vec<User>> {
        let _lock = self.acquire_lock()?;
        self.read_users_unlocked()
    }

    /// The account with `id`, if it exists.
    pub fn user(&self, id: &str) -> io::Result<Option<User>> {
        Ok(self.users()?.into_iter().find(|user| user.id == id))
    }

    /// The account named `username`, if it exists.
    ///
    /// Usernames are compared ASCII-case-insensitively so `Alice` and `alice`
    /// are one account rather than two that a reader could confuse.
    pub fn user_by_username(&self, username: &str) -> io::Result<Option<User>> {
        Ok(self
            .users()?
            .into_iter()
            .find(|user| user.username.eq_ignore_ascii_case(username)))
    }

    /// Add an account. A taken identifier or username is a conflict.
    pub fn insert_user(&self, user: &User) -> io::Result<()> {
        let _lock = self.acquire_lock()?;
        let mut users = self.read_users_unlocked()?;
        if users.iter().any(|existing| existing.id == user.id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "account identifier is taken",
            ));
        }
        if users
            .iter()
            .any(|existing| existing.username.eq_ignore_ascii_case(&user.username))
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "username is taken",
            ));
        }
        users.push(user.clone());
        self.write_users_unlocked(&users)
    }

    /// The account that owns the refresh token with digest `hash`.
    pub fn user_by_refresh_hash(
        &self,
        hash: &str,
    ) -> io::Result<Option<(User, StoredRefreshToken)>> {
        let _lock = self.acquire_lock()?;
        for user in self.read_users_unlocked()? {
            if let Some(token) = user
                .refresh_tokens
                .iter()
                .find(|token| token.hash == hash)
                .cloned()
            {
                return Ok(Some((user, token)));
            }
        }
        Ok(None)
    }

    /// Record a freshly minted refresh token against an account.
    pub fn insert_refresh_token(
        &self,
        user_id: &str,
        token: &StoredRefreshToken,
    ) -> io::Result<()> {
        let _lock = self.acquire_lock()?;
        let mut users = self.read_users_unlocked()?;
        let user = users
            .iter_mut()
            .find(|user| user.id == user_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "account not found"))?;
        user.refresh_tokens.push(token.clone());
        self.write_users_unlocked(&users)
    }

    /// Drop one refresh token, reporting whether it was there to drop.
    ///
    /// Rotation and logout both come through here, so a spent token is gone
    /// before its replacement is written.
    pub fn revoke_refresh_token(&self, user_id: &str, token_id: &str) -> io::Result<bool> {
        let _lock = self.acquire_lock()?;
        let mut users = self.read_users_unlocked()?;
        let Some(user) = users.iter_mut().find(|user| user.id == user_id) else {
            return Ok(false);
        };
        let before = user.refresh_tokens.len();
        user.refresh_tokens.retain(|token| token.id != token_id);
        let removed = user.refresh_tokens.len() != before;
        if removed {
            self.write_users_unlocked(&users)?;
        }
        Ok(removed)
    }

    /// Remove every refresh token that expired at or before `now`, reporting
    /// how many were dropped.
    pub fn prune_expired_refresh_tokens(&self, now: u64) -> io::Result<usize> {
        let _lock = self.acquire_lock()?;
        let mut users = self.read_users_unlocked()?;
        let mut dropped = 0;
        for user in &mut users {
            let before = user.refresh_tokens.len();
            user.refresh_tokens.retain(|token| token.expires_at > now);
            dropped += before - user.refresh_tokens.len();
        }
        if dropped > 0 {
            self.write_users_unlocked(&users)?;
        }
        Ok(dropped)
    }

    /// The grants recorded for one project.
    pub fn grants(&self, project_id: &str) -> io::Result<Grants> {
        let _lock = self.acquire_lock()?;
        self.read_grants_unlocked(project_id)
    }

    /// The role `user_id` holds in `project_id`, if any.
    pub fn role_of(&self, project_id: &str, user_id: &str) -> io::Result<Option<Role>> {
        Ok(self.grants(project_id)?.grants.get(user_id).copied())
    }

    /// Record `role` for `user_id` in `project_id`, replacing any previous role.
    pub fn set_role(&self, project_id: &str, user_id: &str, role: Role) -> io::Result<()> {
        let _lock = self.acquire_lock()?;
        let mut grants = self.read_grants_unlocked(project_id)?;
        grants.grants.insert(user_id.to_owned(), role);
        self.write_grants_unlocked(project_id, &grants)
    }

    /// Drop `user_id`'s role in `project_id`, reporting whether there was one.
    pub fn remove_role(&self, project_id: &str, user_id: &str) -> io::Result<bool> {
        let _lock = self.acquire_lock()?;
        let mut grants = self.read_grants_unlocked(project_id)?;
        if grants.grants.remove(user_id).is_none() {
            return Ok(false);
        }
        self.write_grants_unlocked(project_id, &grants)?;
        Ok(true)
    }

    /// How many accounts hold a role in `project_id`.
    pub fn grant_count(&self, project_id: &str) -> io::Result<usize> {
        Ok(self.grants(project_id)?.grants.len())
    }

    /// Forget every grant for a project, which is what deleting it does.
    ///
    /// A project that never had a grant file is already forgotten.
    pub fn remove_project_grants(&self, project_id: &str) -> io::Result<()> {
        let _lock = self.acquire_lock()?;
        match fs::remove_file(self.grant_path(project_id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Wire identifiers of the projects `user_id` holds a role in, sorted.
    pub fn projects_for_user(&self, user_id: &str) -> io::Result<Vec<String>> {
        let _lock = self.acquire_lock()?;
        let entries = match fs::read_dir(self.grants_dir()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut projects = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(folder) = name.strip_suffix(".json") else {
                continue;
            };
            let grants = self.read_grants_unlocked(&folder_wire_id(folder))?;
            if grants.grants.contains_key(user_id) {
                projects.push(folder_wire_id(folder));
            }
        }
        projects.sort();
        projects.dedup();
        Ok(projects)
    }
}

/// Write a JSON document atomically: a same-directory temporary file, flushed
/// and synced, then renamed over the destination.
fn write_json_atomically(destination: &Path, value: &serde_json::Value) -> io::Result<()> {
    let directory = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".tucano-{}.tmp", unique_suffix()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    set_private_permissions(&file)?;
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, AuthStore) {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AuthStore::new(directory.path()).expect("store");
        (directory, store)
    }

    fn account(id: &str, username: &str) -> User {
        User {
            id: id.to_owned(),
            username: username.to_owned(),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA".to_owned(),
            system_admin: false,
            created_at: 1_700_000_000,
            refresh_tokens: Vec::new(),
        }
    }

    fn token(id: &str, expires_at: u64) -> StoredRefreshToken {
        StoredRefreshToken {
            id: id.to_owned(),
            hash: format!("digest-of-{id}"),
            expires_at,
        }
    }

    #[test]
    fn a_new_store_creates_its_directories() {
        let (directory, _store) = store();
        assert!(directory.path().join("auth").is_dir());
        assert!(directory.path().join("auth/projects").is_dir());
    }

    #[test]
    fn accounts_start_empty() {
        let (_directory, store) = store();
        assert!(store.users().expect("users").is_empty());
    }

    #[test]
    fn an_inserted_account_round_trips() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        let stored = store.user("u1").expect("read").expect("present");
        assert_eq!(stored, account("u1", "alice"));
    }

    #[test]
    fn a_taken_identifier_is_refused() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        let error = store
            .insert_user(&account("u1", "bob"))
            .expect_err("duplicate id");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(store.users().expect("users").len(), 1);
    }

    #[test]
    fn a_taken_username_is_refused_whatever_its_case() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        let error = store
            .insert_user(&account("u2", "Alice"))
            .expect_err("duplicate username");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(store.users().expect("users").len(), 1);
    }

    #[test]
    fn accounts_are_found_by_identifier_and_by_username() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        assert_eq!(store.user("u1").expect("by id").expect("there").id, "u1");
        assert_eq!(
            store
                .user_by_username("ALICE")
                .expect("by name")
                .expect("there")
                .id,
            "u1"
        );
        assert!(store.user("u2").expect("by id").is_none());
        assert!(store.user_by_username("bob").expect("by name").is_none());
    }

    #[test]
    fn a_refresh_token_is_stored_and_found_by_its_digest() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        store
            .insert_refresh_token("u1", &token("t1", 2_000_000_000))
            .expect("token");
        let (user, found) = store
            .user_by_refresh_hash("digest-of-t1")
            .expect("lookup")
            .expect("there");
        assert_eq!(user.id, "u1");
        assert_eq!(found, token("t1", 2_000_000_000));
        assert!(
            store
                .user_by_refresh_hash("digest-of-t2")
                .expect("lookup")
                .is_none()
        );
    }

    #[test]
    fn a_token_for_an_unknown_account_is_not_found() {
        let (_directory, store) = store();
        let error = store
            .insert_refresh_token("nobody", &token("t1", 2_000_000_000))
            .expect_err("unknown account");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn revoking_a_refresh_token_removes_it() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        store
            .insert_refresh_token("u1", &token("t1", 2_000_000_000))
            .expect("token");
        assert!(store.revoke_refresh_token("u1", "t1").expect("revoke"));
        assert!(
            store
                .user_by_refresh_hash("digest-of-t1")
                .expect("lookup")
                .is_none()
        );
    }

    #[test]
    fn revoking_an_unknown_refresh_token_reports_nothing_removed() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        assert!(!store.revoke_refresh_token("u1", "t1").expect("revoke"));
        assert!(!store.revoke_refresh_token("u2", "t1").expect("revoke"));
    }

    #[test]
    fn pruning_drops_only_the_expired_tokens() {
        let (_directory, store) = store();
        store.insert_user(&account("u1", "alice")).expect("insert");
        store
            .insert_refresh_token("u1", &token("old", 1_000))
            .expect("token");
        store
            .insert_refresh_token("u1", &token("live", 3_000))
            .expect("token");
        assert_eq!(store.prune_expired_refresh_tokens(2_000).expect("prune"), 1);
        let stored = store.user("u1").expect("read").expect("present");
        assert_eq!(stored.refresh_tokens, vec![token("live", 3_000)]);
        assert_eq!(store.prune_expired_refresh_tokens(2_000).expect("prune"), 0);
    }

    #[test]
    fn a_role_is_recorded_and_read_back() {
        let (_directory, store) = store();
        store
            .set_role("checkout.json", "u1", Role::Editor)
            .expect("set");
        assert_eq!(
            store.role_of("checkout.json", "u1").expect("role"),
            Some(Role::Editor)
        );
    }

    #[test]
    fn roles_are_scoped_to_their_project() {
        let (_directory, store) = store();
        store
            .set_role("checkout.json", "u1", Role::Owner)
            .expect("set");
        assert_eq!(store.grant_count("checkout.json").expect("count"), 1);
        assert_eq!(store.grant_count("payments.json").expect("count"), 0);
        assert_eq!(store.role_of("payments.json", "u1").expect("role"), None);
    }

    #[test]
    fn setting_a_role_again_replaces_it() {
        let (_directory, store) = store();
        store
            .set_role("checkout.json", "u1", Role::Viewer)
            .expect("set");
        store
            .set_role("checkout.json", "u1", Role::Owner)
            .expect("set");
        assert_eq!(store.grant_count("checkout.json").expect("count"), 1);
        assert_eq!(
            store.role_of("checkout.json", "u1").expect("role"),
            Some(Role::Owner)
        );
    }

    #[test]
    fn removing_a_role_reports_whether_it_was_there() {
        let (_directory, store) = store();
        store
            .set_role("checkout.json", "u1", Role::Editor)
            .expect("set");
        assert!(store.remove_role("checkout.json", "u1").expect("remove"));
        assert!(!store.remove_role("checkout.json", "u1").expect("remove"));
        assert_eq!(store.role_of("checkout.json", "u1").expect("role"), None);
    }

    #[test]
    fn forgetting_a_project_removes_its_grants() {
        let (_directory, store) = store();
        store
            .set_role("checkout.json", "u1", Role::Owner)
            .expect("set");
        store
            .remove_project_grants("checkout.json")
            .expect("forget");
        assert_eq!(store.grant_count("checkout.json").expect("count"), 0);
        store
            .remove_project_grants("checkout.json")
            .expect("forget");
    }

    #[test]
    fn projects_are_listed_for_the_accounts_that_hold_roles() {
        let (_directory, store) = store();
        store
            .set_role("payments.json", "u1", Role::Viewer)
            .expect("set");
        store
            .set_role("checkout.json", "u1", Role::Owner)
            .expect("set");
        store
            .set_role("checkout.json", "u2", Role::Viewer)
            .expect("set");
        assert_eq!(
            store.projects_for_user("u1").expect("list"),
            vec!["checkout.json".to_owned(), "payments.json".to_owned()]
        );
        assert_eq!(
            store.projects_for_user("u2").expect("list"),
            vec!["checkout.json".to_owned()]
        );
        assert!(store.projects_for_user("u3").expect("list").is_empty());
    }

    #[test]
    fn a_project_identifier_cannot_escape_the_auth_directory() {
        let (_directory, store) = store();
        for escape in ["../escape.json", "nested/escape.json", ".."] {
            assert!(
                store.role_of(escape, "u1").is_err(),
                "{escape} must not resolve"
            );
            assert!(
                store.set_role(escape, "u1", Role::Owner).is_err(),
                "{escape} must not resolve"
            );
        }
    }

    #[test]
    fn roles_rank_by_privilege() {
        assert!(Role::Owner > Role::Editor);
        assert!(Role::Editor > Role::Viewer);
    }
}
