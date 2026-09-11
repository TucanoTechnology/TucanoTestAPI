//! Authentication and per-project authorisation.
//!
//! The API keeps no database, so neither does this module: accounts, refresh
//! tokens, and project grants live under `auth/` inside the data directory,
//! and the store that reads them arrives in a later step. What is here is the
//! vocabulary the rest of the feature is written in, and it is deliberately
//! storage-free and clock-free so the rules can be exercised directly:
//!
//! - [`config`] resolves the environment into an [`AuthConfig`], refusing the
//!   settings a running server must never accept (a missing or short signing
//!   secret, an unusable lifetime).
//! - [`password`] hashes and checks credentials with Argon2id.
//! - [`token`] mints and verifies the short-lived HS256 access token and the
//!   opaque refresh token, and derives the digest the store keeps at rest.
//!
//! Nothing in here reaches the network or the disk except the one secret file
//! [`config`] may read, and nothing reads the clock on its own: the instant a
//! token is issued or checked against is a parameter, which is what makes the
//! expiry rules testable without sleeping.

pub mod config;
pub mod password;
pub mod token;

pub use config::{
    AuthConfig, ConfigError, DEFAULT_ACCESS_TTL, DEFAULT_REFRESH_TTL, MIN_SECRET_BYTES,
};
pub use password::{HashError, hash_password, verify_password};
pub use token::{
    Claims, REFRESH_TOKEN_BYTES, TokenError, hash_refresh_token, mint_access_token,
    mint_refresh_token, random_id, verify_access_token,
};
