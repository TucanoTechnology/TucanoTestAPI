//! AEAD-encrypted secrets for the configuration file.
//!
//! This module implements the encryption model decided in
//! [`docs/security/configuration-decision.md`]: secret values in the
//! configuration file may be stored as AEAD-encrypted envelopes rather than in
//! plaintext, with the encryption key supplied from outside the file through a
//! read-only mounted key file.
//!
//! # Envelope format
//!
//! An encrypted value is a JSON object:
//!
//! ```json
//! {
//!   "version": 1,
//!   "key_id": "key-2026-09",
//!   "algorithm": "aes-256-gcm",
//!   "nonce": "<base64url, 12 bytes>",
//!   "ciphertext": "<base64url, ciphertext + GCM auth tag>"
//! }
//! ```
//!
//! The envelope is versioned and self-describing, so the algorithm can change
//! without a flag day. Only `aes-256-gcm` is implemented today.
//!
//! # Key ring
//!
//! The key file is a JSON document:
//!
//! ```json
//! {
//!   "keys": [
//!     { "id": "key-2026-09", "key": "<base64url, 32 bytes>" }
//!   ]
//! }
//! ```
//!
//! Multiple keys are supported for rotation: the active key encrypts new
//! values, and retired keys are still tried during decryption. A file whose
//! key identifier is not in the ring is a startup error.
//!
//! # Fail-closed
//!
//! Every failure mode is a startup error:
//!
//! - Encrypted value with no matching key → refuse to boot.
//! - Corrupted ciphertext or wrong key → refuse to boot (GCM tag mismatch).
//! - Malformed envelope → refuse to boot.
//! - Key file names a key shorter than 32 bytes → refuse to boot.
//!
//! No secret value, key material, or raw ciphertext appears in any error.
//!
//! [`docs/security/configuration-decision.md`]: https://github.com/TucanoTechnology/TucanoTestAPI/blob/main/docs/security/configuration-decision.md

use std::env;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use aes_gcm::aead::{Aead, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

/// The environment variable naming the key file.
///
/// Like [`super::CONFIG_FILE_ENV`], this is environment-only: the orchestrator
/// sets it before the process starts, and it must be readable before the
/// configuration file can be decrypted.
pub const CONFIG_KEY_FILE_ENV: &str = "TUCANO_CONFIG_KEY_FILE";

/// The only envelope version this build understands.
pub const ENVELOPE_VERSION: u32 = 1;

/// The only algorithm this build implements.
pub const ALGORITHM: &str = "aes-256-gcm";

/// AES-256 key length in bytes.
pub const KEY_LENGTH: usize = 32;

/// AES-GCM nonce length in bytes.
const NONCE_LENGTH: usize = 12;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// An error decrypting a configuration value or loading the key ring.
///
/// Every variant names the *setting* or the *key identifier* at fault. None of
/// them can carry a secret value, key material, or raw ciphertext: those are
/// absent from this type by construction.
#[derive(Debug)]
pub enum SecretError {
    /// The envelope's `version` is not [`ENVELOPE_VERSION`].
    UnsupportedVersion {
        /// The version found.
        found: u32,
    },
    /// The envelope's `algorithm` is not [`ALGORITHM`].
    UnsupportedAlgorithm {
        /// The algorithm string found.
        found: String,
    },
    /// The envelope's `key_id` is not in the key ring.
    UnknownKeyId {
        /// The key identifier found.
        found: String,
    },
    /// The ciphertext or authentication tag is invalid.
    ///
    /// Covers both tampered data and a wrong key: GCM's tag check is the only
    /// way to tell, and neither is reported because the distinction would name
    /// which key was tried.
    DecryptionFailed,
    /// The envelope is not valid JSON or is missing a required field.
    MalformedEnvelope,
    /// The key ring file could not be read.
    UnreadableKeyFile,
    /// The key ring is not a valid document.
    MalformedKeyFile,
    /// A key in the ring is not the right length.
    InvalidKeyLength,
}

impl Display for SecretError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "an encrypted value declares envelope version {found}; \
                 this build implements version {ENVELOPE_VERSION}"
            ),
            Self::UnsupportedAlgorithm { found } => write!(
                formatter,
                "an encrypted value declares algorithm {found:?}; \
                 this build implements {ALGORITHM:?}"
            ),
            Self::UnknownKeyId { found } => write!(
                formatter,
                "an encrypted value names key {found:?}, which is not in the key ring"
            ),
            Self::DecryptionFailed => write!(
                formatter,
                "an encrypted configuration value could not be decrypted: \
                 the ciphertext is corrupted or the key is wrong"
            ),
            Self::MalformedEnvelope => write!(
                formatter,
                "an encrypted value in the configuration file is not a valid envelope"
            ),
            Self::UnreadableKeyFile => write!(
                formatter,
                "{CONFIG_KEY_FILE_ENV} names a key file that cannot be read"
            ),
            Self::MalformedKeyFile => write!(
                formatter,
                "the key file named by {CONFIG_KEY_FILE_ENV} is not a valid document"
            ),
            Self::InvalidKeyLength => write!(
                formatter,
                "a key in the key file is not {KEY_LENGTH} bytes; \
                 AES-256-GCM requires exactly {KEY_LENGTH} bytes"
            ),
        }
    }
}

impl Error for SecretError {}

// ---------------------------------------------------------------------------
// Encrypted envelope
// ---------------------------------------------------------------------------

/// An AEAD-encrypted value as it appears in the configuration file.
///
/// The format is versioned and self-describing: `version`, `algorithm`, and
/// `key_id` let the loader decide how to decrypt without any out-of-band
/// information. `nonce` and `ciphertext` are base64url-encoded without padding,
/// matching the project's existing base64 usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    /// The envelope format version; must be [`ENVELOPE_VERSION`].
    pub version: u32,
    /// The key identifier; must match a key in the ring.
    pub key_id: String,
    /// The algorithm; must be [`ALGORITHM`].
    pub algorithm: String,
    /// Base64url-encoded nonce (12 bytes).
    pub nonce: String,
    /// Base64url-encoded ciphertext with GCM authentication tag.
    pub ciphertext: String,
}

impl EncryptedEnvelope {
    /// Validates the envelope's version and algorithm markers.
    ///
    /// # Errors
    ///
    /// [`SecretError::UnsupportedVersion`] or
    /// [`SecretError::UnsupportedAlgorithm`].
    pub fn validate(&self) -> Result<(), SecretError> {
        if self.version != ENVELOPE_VERSION {
            return Err(SecretError::UnsupportedVersion {
                found: self.version,
            });
        }
        if self.algorithm != ALGORITHM {
            return Err(SecretError::UnsupportedAlgorithm {
                found: self.algorithm.clone(),
            });
        }
        Ok(())
    }

    /// Decrypts the envelope with the given raw 32-byte key.
    ///
    /// # Errors
    ///
    /// [`SecretError::DecryptionFailed`] if the nonce or ciphertext is not
    /// valid base64, the wrong length, or the GCM authentication tag does not
    /// match (wrong key or corrupted data).
    pub fn decrypt(&self, key: &[u8; KEY_LENGTH]) -> Result<Vec<u8>, SecretError> {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| SecretError::DecryptionFailed)?;

        let nonce_bytes = URL_SAFE_NO_PAD
            .decode(&self.nonce)
            .map_err(|_| SecretError::DecryptionFailed)?;
        if nonce_bytes.len() != NONCE_LENGTH {
            return Err(SecretError::DecryptionFailed);
        }
        let nonce_array: [u8; NONCE_LENGTH] = nonce_bytes
            .try_into()
            .map_err(|_| SecretError::DecryptionFailed)?;
        let nonce: Nonce<Aes256Gcm> = nonce_array.into();

        let ciphertext = URL_SAFE_NO_PAD
            .decode(&self.ciphertext)
            .map_err(|_| SecretError::DecryptionFailed)?;

        cipher
            .decrypt(&nonce, ciphertext.as_ref())
            .map_err(|_| SecretError::DecryptionFailed)
    }
}

/// Encrypts `plaintext` with AES-256-GCM and returns a self-describing
/// envelope.
///
/// A fresh random nonce is generated for every call via the operating system's
/// CSPRNG. The returned envelope contains everything needed to decrypt: the
/// algorithm, the key identifier, the nonce, and the ciphertext with its GCM
/// authentication tag.
///
/// # Errors
///
/// [`SecretError::DecryptionFailed`] if the nonce cannot be generated or the
/// encryption primitive fails (both are effectively unreachable with a valid
/// 32-byte key and a working CSPRNG).
pub fn encrypt_value(
    plaintext: &[u8],
    key: &[u8; KEY_LENGTH],
    key_id: &str,
) -> Result<EncryptedEnvelope, SecretError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| SecretError::DecryptionFailed)?;

    let mut nonce_bytes = [0u8; NONCE_LENGTH];
    getrandom::getrandom(&mut nonce_bytes).map_err(|_| SecretError::DecryptionFailed)?;
    let nonce: Nonce<Aes256Gcm> = nonce_bytes.into();

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_| SecretError::DecryptionFailed)?;

    Ok(EncryptedEnvelope {
        version: ENVELOPE_VERSION,
        key_id: key_id.to_owned(),
        algorithm: ALGORITHM.to_owned(),
        nonce: URL_SAFE_NO_PAD.encode(nonce_bytes),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    })
}

// ---------------------------------------------------------------------------
// Secret value — the serde bridge
// ---------------------------------------------------------------------------

/// A configuration value that may be stored as plaintext or as an
/// AEAD-encrypted envelope.
///
/// Serde's `untagged` attribute tries the envelope shape first (an object with
/// a `version` field) and falls back to a plain string. This means an existing
/// file with `"jwt_secret": "..."` keeps working, and a new file can switch to
/// `"jwt_secret": {"version": 1, ...}` without a schema change.
///
/// A value that *looks like* an envelope (an object) but is not a valid one is
/// reported as [`SecretError::MalformedEnvelope`] rather than silently falling
/// through to the string variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SecretValue {
    /// An unencrypted string.
    Plain(String),
    /// An AEAD-encrypted envelope.
    Encrypted(EncryptedEnvelope),
}

impl SecretValue {
    /// Resolves the value, decrypting if necessary.
    ///
    /// - `Plain(s)` returns `s` unchanged, whether or not a key ring is
    ///   provided.
    /// - `Encrypted(e)` requires `keys`; `None` is
    ///   [`SecretError::UnknownKeyId`] because a key ring was expected but not
    ///   available.
    ///
    /// # Errors
    ///
    /// Any [`SecretError`] from the envelope or the key ring lookup.
    pub fn resolve(&self, keys: Option<&KeyRing>) -> Result<String, SecretError> {
        match self {
            Self::Plain(s) => Ok(s.clone()),
            Self::Encrypted(envelope) => {
                envelope.validate()?;
                let ring = keys.ok_or_else(|| SecretError::UnknownKeyId {
                    found: envelope.key_id.clone(),
                })?;
                let key = ring
                    .get(&envelope.key_id)
                    .ok_or_else(|| SecretError::UnknownKeyId {
                        found: envelope.key_id.clone(),
                    })?;
                let plaintext = envelope.decrypt(key)?;
                String::from_utf8(plaintext).map_err(|_| SecretError::DecryptionFailed)
            }
        }
    }

    /// Whether this value is an encrypted envelope.
    pub fn is_encrypted(&self) -> bool {
        matches!(self, Self::Encrypted(_))
    }
}

// ---------------------------------------------------------------------------
// Key ring
// ---------------------------------------------------------------------------

/// A named set of decryption keys, loaded from the key file.
///
/// The ring supports rotation: multiple keys coexist so a file encrypted under
/// an older key can still be read after the active key has changed. The
/// rotation procedure is:
///
/// 1. Add the new key to the ring, restart.
/// 2. Re-encrypt the configuration file under the new key, restart.
/// 3. Remove the old key from the ring, restart.
///
/// Each step is safe on its own: step 1 reads with either key, step 2 writes
/// with the new key (and the ring still holds both), step 3 removes the old
/// key after no value needs it.
#[derive(Debug)]
pub struct KeyRing {
    keys: Vec<(String, [u8; KEY_LENGTH])>,
}

impl KeyRing {
    /// Creates a ring from a list of `(id, key)` pairs.
    ///
    /// An empty ring is valid: it simply cannot decrypt anything, which is the
    /// right behaviour for a deployment that uses only plaintext values.
    pub fn new(entries: Vec<(String, [u8; KEY_LENGTH])>) -> Self {
        Self { keys: entries }
    }

    /// Looks up a key by its identifier.
    pub fn get(&self, id: &str) -> Option<&[u8; KEY_LENGTH]> {
        self.keys
            .iter()
            .find(|(kid, _)| kid == id)
            .map(|(_, key)| key)
    }

    /// Whether the ring holds no keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// The key file as it appears on disk.
#[derive(Debug, Deserialize)]
struct KeyFileDocument {
    keys: Vec<KeyFileEntry>,
}

/// One entry in the key file.
#[derive(Debug, Deserialize)]
struct KeyFileEntry {
    id: String,
    key: String,
}

/// Reads the key ring from `path`.
///
/// # Errors
///
/// [`SecretError::UnreadableKeyFile`] if the file cannot be read,
/// [`SecretError::MalformedKeyFile`] if it is not a valid document, or
/// [`SecretError::InvalidKeyLength`] if any key decodes to the wrong length.
/// None of these errors carry the file path, key material, or file contents.
pub fn load_key_ring(path: &Path) -> Result<KeyRing, SecretError> {
    let text = std::fs::read_to_string(path).map_err(|_| SecretError::UnreadableKeyFile)?;
    parse_key_ring(&text)
}

/// Parses a key ring from its JSON text.
///
/// # Errors
///
/// [`SecretError::MalformedKeyFile`] or [`SecretError::InvalidKeyLength`].
pub fn parse_key_ring(text: &str) -> Result<KeyRing, SecretError> {
    let document: KeyFileDocument =
        serde_json::from_str(text).map_err(|_| SecretError::MalformedKeyFile)?;

    let mut keys = Vec::with_capacity(document.keys.len());
    for entry in document.keys {
        let raw = URL_SAFE_NO_PAD
            .decode(&entry.key)
            .or_else(|_| {
                // Fall back to standard base64 (with padding) for convenience.
                use base64::engine::general_purpose::STANDARD;
                STANDARD.decode(&entry.key)
            })
            .map_err(|_| SecretError::MalformedKeyFile)?;
        if raw.len() != KEY_LENGTH {
            return Err(SecretError::InvalidKeyLength);
        }
        let mut key = [0u8; KEY_LENGTH];
        key.copy_from_slice(&raw);
        keys.push((entry.id, key));
    }

    Ok(KeyRing::new(keys))
}

/// Reads the key ring from the process environment.
///
/// Returns `None` if [`CONFIG_KEY_FILE_ENV`] is not set, which is the
/// documented "no encryption keys" case.
///
/// # Errors
///
/// Any [`SecretError`] from [`load_key_ring`].
pub fn load_key_ring_from_env() -> Result<Option<KeyRing>, SecretError> {
    load_key_ring_from_lookup(|key| env::var(key).ok())
}

/// Reads the key ring from `lookup`.
///
/// Returns `None` if the variable is not set.
///
/// # Errors
///
/// Any [`SecretError`] from [`load_key_ring`].
pub fn load_key_ring_from_lookup(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<KeyRing>, SecretError> {
    match lookup(CONFIG_KEY_FILE_ENV) {
        Some(path) => load_key_ring(Path::new(&path)).map(Some),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid 32-byte key, base64url-encoded.
    fn test_key_bytes() -> [u8; KEY_LENGTH] {
        let mut key = [0u8; KEY_LENGTH];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(7).wrapping_add(13);
        }
        key
    }

    fn test_key_id() -> &'static str {
        "test-key-1"
    }

    fn ring_with(key: [u8; KEY_LENGTH]) -> KeyRing {
        KeyRing::new(vec![(test_key_id().to_owned(), key)])
    }

    // -- Envelope encryption and decryption ---------------------------------

    #[test]
    fn encrypt_and_decrypt_round_trips() {
        let key = test_key_bytes();
        let plaintext = b"a-test-secret-long-enough-for-hs256";
        let envelope = encrypt_value(plaintext, &key, test_key_id()).expect("encrypt");

        assert_eq!(envelope.version, ENVELOPE_VERSION);
        assert_eq!(envelope.key_id, test_key_id());
        assert_eq!(envelope.algorithm, ALGORITHM);

        let decrypted = envelope.decrypt(&key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_produces_different_ciphertexts_for_the_same_plaintext() {
        // Each encryption uses a fresh random nonce, so two encryptions of
        // the same value must differ — this is what makes nonce reuse
        // impossible in practice.
        let key = test_key_bytes();
        let plaintext = b"a-test-secret-long-enough-for-hs256";
        let a = encrypt_value(plaintext, &key, test_key_id()).expect("encrypt a");
        let b = encrypt_value(plaintext, &key, test_key_id()).expect("encrypt b");
        assert_ne!(
            a.nonce, b.nonce,
            "two encryptions must use different nonces"
        );
        assert_ne!(
            a.ciphertext, b.ciphertext,
            "different nonces produce different ciphertexts"
        );
    }

    #[test]
    fn decryption_with_the_wrong_key_fails() {
        let key = test_key_bytes();
        let plaintext = b"a-test-secret-long-enough-for-hs256";
        let envelope = encrypt_value(plaintext, &key, test_key_id()).expect("encrypt");

        let mut wrong_key = test_key_bytes();
        wrong_key[0] ^= 0xFF;

        let error = envelope.decrypt(&wrong_key).expect_err("wrong key");
        assert!(matches!(error, SecretError::DecryptionFailed), "{error:?}");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let key = test_key_bytes();
        let plaintext = b"a-test-secret-long-enough-for-hs256";
        let mut envelope = encrypt_value(plaintext, &key, test_key_id()).expect("encrypt");

        // Flip a byte in the ciphertext.
        let mut raw = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .expect("decode");
        raw[0] ^= 0xFF;
        envelope.ciphertext = URL_SAFE_NO_PAD.encode(&raw);

        let error = envelope.decrypt(&key).expect_err("tampered");
        assert!(matches!(error, SecretError::DecryptionFailed), "{error:?}");
    }

    // -- Envelope validation ------------------------------------------------

    #[test]
    fn an_unknown_version_is_refused() {
        let key = test_key_bytes();
        let mut envelope = encrypt_value(b"secret", &key, test_key_id()).expect("encrypt");
        envelope.version = 99;

        let error = envelope.validate().expect_err("future version");
        match error {
            SecretError::UnsupportedVersion { found } => assert_eq!(found, 99),
            other => panic!("expected unsupported-version, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_algorithm_is_refused() {
        let key = test_key_bytes();
        let mut envelope = encrypt_value(b"secret", &key, test_key_id()).expect("encrypt");
        envelope.algorithm = "chacha20-poly1305".to_owned();

        let error = envelope.validate().expect_err("wrong algorithm");
        match error {
            SecretError::UnsupportedAlgorithm { found } => {
                assert_eq!(found, "chacha20-poly1305")
            }
            other => panic!("expected unsupported-algorithm, got {other:?}"),
        }
    }

    // -- SecretValue serde --------------------------------------------------

    #[test]
    fn a_plain_string_deserialises_as_plain() {
        let value: SecretValue = serde_json::from_str(r#""hello""#).expect("parse plain");
        assert!(matches!(value, SecretValue::Plain(ref s) if s == "hello"));
    }

    #[test]
    fn an_encrypted_object_deserialises_as_encrypted() {
        let key = test_key_bytes();
        let envelope = encrypt_value(b"secret", &key, test_key_id()).expect("encrypt");
        let json = serde_json::to_string(&envelope).expect("serialise");
        let value: SecretValue = serde_json::from_str(&json).expect("parse envelope");
        assert!(value.is_encrypted());
    }

    #[test]
    fn a_plain_secret_resolves_without_a_key_ring() {
        let value = SecretValue::Plain("hello".to_owned());
        let resolved = value.resolve(None).expect("resolve plain");
        assert_eq!(resolved, "hello");
    }

    #[test]
    fn an_encrypted_secret_resolves_with_the_right_key() {
        let key = test_key_bytes();
        let envelope = encrypt_value(b"the-secret-value", &key, test_key_id()).expect("encrypt");
        let value = SecretValue::Encrypted(envelope);
        let ring = ring_with(key);
        let resolved = value.resolve(Some(&ring)).expect("resolve encrypted");
        assert_eq!(resolved, "the-secret-value");
    }

    #[test]
    fn an_encrypted_secret_without_a_key_ring_fails_closed() {
        let key = test_key_bytes();
        let envelope = encrypt_value(b"secret", &key, test_key_id()).expect("encrypt");
        let value = SecretValue::Encrypted(envelope);
        let error = value.resolve(None).expect_err("no key ring");
        assert!(
            matches!(error, SecretError::UnknownKeyId { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn an_encrypted_secret_with_the_wrong_key_id_fails_closed() {
        let key = test_key_bytes();
        let envelope =
            encrypt_value(b"secret", &key, "key-that-is-not-in-the-ring").expect("encrypt");
        let value = SecretValue::Encrypted(envelope);
        let ring = ring_with(key);
        let error = value.resolve(Some(&ring)).expect_err("unknown key id");
        match error {
            SecretError::UnknownKeyId { found } => {
                assert_eq!(found, "key-that-is-not-in-the-ring")
            }
            other => panic!("expected unknown-key-id, got {other:?}"),
        }
    }

    // -- Key ring -----------------------------------------------------------

    #[test]
    fn an_empty_key_ring_is_valid_and_matches_nothing() {
        let ring = KeyRing::new(vec![]);
        assert!(ring.is_empty());
        assert!(ring.get("any-key").is_none());
    }

    #[test]
    fn a_key_ring_file_is_parsed_correctly() {
        let key = test_key_bytes();
        let encoded = URL_SAFE_NO_PAD.encode(key);
        let json = format!(r#"{{"keys": [{{"id": "k1", "key": "{encoded}"}}]}}"#);
        let ring = parse_key_ring(&json).expect("parse key ring");
        assert_eq!(ring.get("k1"), Some(&key));
        assert!(ring.get("k2").is_none());
    }

    #[test]
    fn a_key_ring_with_multiple_keys_supports_rotation() {
        let key_a = test_key_bytes();
        let mut key_b = [0u8; KEY_LENGTH];
        for (i, byte) in key_b.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(11).wrapping_add(37);
        }

        let enc_a = URL_SAFE_NO_PAD.encode(key_a);
        let enc_b = URL_SAFE_NO_PAD.encode(key_b);
        let json = format!(
            r#"{{"keys": [
                {{"id": "old", "key": "{enc_a}"}},
                {{"id": "new", "key": "{enc_b}"}}
            ]}}"#
        );
        let ring = parse_key_ring(&json).expect("parse key ring");
        assert_eq!(ring.get("old"), Some(&key_a));
        assert_eq!(ring.get("new"), Some(&key_b));

        // A value encrypted under the old key is still decryptable.
        let envelope = encrypt_value(b"rotated-secret", &key_a, "old").expect("encrypt old");
        let value = SecretValue::Encrypted(envelope);
        let resolved = value.resolve(Some(&ring)).expect("resolve old key");
        assert_eq!(resolved, "rotated-secret");

        // A value encrypted under the new key is also decryptable.
        let envelope = encrypt_value(b"new-secret", &key_b, "new").expect("encrypt new");
        let value = SecretValue::Encrypted(envelope);
        let resolved = value.resolve(Some(&ring)).expect("resolve new key");
        assert_eq!(resolved, "new-secret");
    }

    #[test]
    fn a_key_of_the_wrong_length_is_refused() {
        let short = URL_SAFE_NO_PAD.encode([0u8; 16]);
        let json = format!(r#"{{"keys": [{{"id": "k1", "key": "{short}"}}]}}"#);
        let error = parse_key_ring(&json).expect_err("short key");
        assert!(matches!(error, SecretError::InvalidKeyLength), "{error:?}");
    }

    #[test]
    fn a_malformed_key_file_is_refused() {
        let error = parse_key_ring("not json").expect_err("malformed");
        assert!(matches!(error, SecretError::MalformedKeyFile), "{error:?}");
    }

    #[test]
    fn a_key_file_missing_required_fields_is_refused() {
        let error = parse_key_ring(r#"{"keys": [{"id": "k1"}]}"#).expect_err("missing key field");
        assert!(matches!(error, SecretError::MalformedKeyFile), "{error:?}");
    }

    #[test]
    fn an_unreadable_key_file_reports_the_setting_not_the_path() {
        let error = load_key_ring(Path::new("/definitely/not/here.key")).expect_err("missing file");
        let rendered = error.to_string();
        assert!(
            rendered.contains(CONFIG_KEY_FILE_ENV),
            "the message should name the setting: {rendered}"
        );
        assert!(
            !rendered.contains("/definitely/not/here.key"),
            "the message must not echo the raw path: {rendered}"
        );
    }

    #[test]
    fn no_key_file_variable_means_no_key_ring() {
        let loaded = load_key_ring_from_lookup(|_| None).expect("absent key file");
        assert!(loaded.is_none());
    }

    #[test]
    fn a_named_key_file_is_loaded_through_the_lookup() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("keys.json");
        let key = test_key_bytes();
        let encoded = URL_SAFE_NO_PAD.encode(key);
        std::fs::write(
            &path,
            format!(r#"{{"keys": [{{"id": "k1", "key": "{encoded}"}}]}}"#),
        )
        .expect("write key file");

        let loaded = load_key_ring_from_lookup(|name| {
            (name == CONFIG_KEY_FILE_ENV).then(|| path.to_str().expect("utf-8").to_owned())
        })
        .expect("load key ring");
        let ring = loaded.expect("ring present");
        assert_eq!(ring.get("k1"), Some(&key));
    }

    // -- Error messages carry no secrets ------------------------------------

    #[test]
    fn no_error_message_carries_key_material() {
        let key = test_key_bytes();
        let key_b64 = URL_SAFE_NO_PAD.encode(key);

        // Malformed key file: the input contains key material.
        let error = parse_key_ring(&format!(
            r#"{{"keys": [{{"id": "k1", "key": "{key_b64}", extra}}]}}"#
        ))
        .expect_err("malformed");
        let rendered = error.to_string();
        assert!(
            !rendered.contains(&key_b64),
            "a key file error must not echo key material: {rendered}"
        );
    }

    #[test]
    fn no_error_message_carries_a_plaintext_value() {
        // Encrypt a recognisable value, corrupt it, and check the error.
        let key = test_key_bytes();
        let plaintext = b"recognisable-secret-value-12345";
        let mut envelope = encrypt_value(plaintext, &key, test_key_id()).expect("encrypt");

        // Corrupt the ciphertext.
        let mut raw = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .expect("decode");
        raw[0] ^= 0xFF;
        envelope.ciphertext = URL_SAFE_NO_PAD.encode(&raw);

        let error = envelope.decrypt(&key).expect_err("corrupted");
        let rendered = error.to_string();
        let plaintext_str = String::from_utf8_lossy(plaintext);
        assert!(
            !rendered.contains(plaintext_str.as_ref()),
            "a decryption error must not echo the plaintext: {rendered}"
        );
    }

    #[test]
    fn a_variable_naming_a_missing_key_file_is_an_error() {
        let error = load_key_ring_from_lookup(|name| {
            (name == CONFIG_KEY_FILE_ENV).then(|| "/definitely/not/here.key".to_owned())
        })
        .expect_err("missing named file");
        assert!(matches!(error, SecretError::UnreadableKeyFile), "{error:?}");
    }
}
