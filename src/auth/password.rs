//! Argon2id credential hashing.
//!
//! The stored form is the PHC string Argon2 emits (`$argon2id$v=19$m=…`), so
//! the cost parameters travel with the hash: raising them later re-hashes new
//! credentials without invalidating the ones already on disk, and an existing
//! record keeps verifying under the parameters it was made with.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand::RngCore;

/// Salt length, in bytes, for a freshly hashed credential.
const SALT_BYTES: usize = 16;

/// A credential could not be hashed.
///
/// The only way to reach this is a broken salt or an exhausted system random
/// source; the password itself cannot fail to hash.
#[derive(Debug)]
pub struct HashError;

impl Display for HashError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("credential could not be hashed")
    }
}

impl Error for HashError {}

/// Hashes a password with Argon2id and a fresh random salt.
///
/// # Errors
///
/// [`HashError`] if the salt cannot be built or the hash cannot be produced.
pub fn hash_password(password: &str) -> Result<String, HashError> {
    let mut salt_bytes = [0_u8; SALT_BYTES];
    rand::thread_rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| HashError)?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| HashError)
}

/// Checks a password against a stored PHC hash.
///
/// A hash that cannot be parsed is a mismatch rather than an error: the
/// caller's decision is the same either way, and returning an error would make
/// a corrupt record distinguishable from a wrong password. Callers must still
/// spend the same work on an unknown account as on a known one, or the reply
/// time tells an attacker which usernames exist.
#[must_use]
pub fn verify_password(stored: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_verifies_against_its_own_hash() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password(&hash, "correct horse battery staple"));
        assert!(!verify_password(&hash, "Correct horse battery staple"));
        assert!(!verify_password(&hash, ""));
    }

    #[test]
    fn the_same_password_hashes_twice_to_different_strings() {
        let first = hash_password("repeated").expect("hash");
        let second = hash_password("repeated").expect("hash");
        assert_ne!(first, second, "a fresh salt each time");
        assert!(verify_password(&first, "repeated"));
        assert!(verify_password(&second, "repeated"));
    }

    #[test]
    fn an_unreadable_hash_is_a_mismatch() {
        for stored in [
            "",
            "not-a-phc-string",
            "$argon2id$v=19$m=1",
            "$argon2i$broken",
        ] {
            assert!(!verify_password(stored, "anything"), "{stored:?}");
        }
    }

    #[test]
    fn a_hash_carries_the_parameters_it_was_made_with() {
        let hash = hash_password("parameters").expect("hash");
        assert!(hash.contains("$argon2id$"), "{hash}");
        assert!(hash.contains("$m="), "{hash}");
        assert!(hash.contains(",t="), "{hash}");
        assert!(hash.contains(",p="), "{hash}");
    }
}
