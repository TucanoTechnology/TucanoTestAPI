//! The optional configuration file, and the resolver that layers it under the
//! environment.
//!
//! [`docs/security/configuration-decision.md`] decides the model this module
//! implements: environment first, file second, built-in defaults last, with
//! precedence resolved **per key**. The split of responsibility follows from
//! that:
//!
//! - [`ConfigFile`] is the file's schema. It is a plain serde document with
//!   `deny_unknown_fields`, so a typo in a key name is a startup error rather
//!   than a setting that silently never applies. The `version` marker is
//!   mandatory and checked before anything else, so a file written for a future
//!   schema is refused instead of being read for whatever happens to match.
//! - [`load`] reads the file named by [`CONFIG_FILE_ENV`] and nothing else: the
//!   decision forbids an implicit default-path search, because a stray file in
//!   a writable directory would then change a production deployment silently.
//!   No variable set means no file, which is how an existing env-only
//!   deployment keeps behaving exactly as it did.
//! - [`resolve`] is the precedence rule itself, and it is a pure function of an
//!   environment lookup closure — the same shape as
//!   [`crate::auth::AuthConfig::from_lookup`]. `std::env::set_var` is `unsafe`
//!   in edition 2024 and this crate forbids `unsafe_code`, so purity is not a
//!   style preference here; it is the only way to test every precedence case
//!   and every refusal without mutating global state.
//!
//! Errors are startup errors, and their text names the *setting*: the ADR's
//! non-negotiable rule is that no secret value, no raw file path and no raw file
//! contents reach a log line, an error envelope or a response. The error type
//! below carries none of those by construction, so no `Display` impl can leak
//! them by accident.
//!
//! [`docs/security/configuration-decision.md`]: https://github.com/TucanoTechnology/TucanoTestAPI/blob/main/docs/security/configuration-decision.md

use std::env;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub mod secret;

use secret::{KeyRing, SecretError, SecretValue, load_key_ring_from_lookup};

/// The environment variable naming the configuration file.
///
/// One of the three settings the ADR keeps environment-only, alongside
/// `TUCANO_DATA_DIR` and `PORT`: all three must be readable *before* the file
/// can be located, and the orchestrator owns all three. Setting it names a
/// file; leaving it unset is the documented way to say "no configuration file",
/// and preserves the pre-file behaviour of the service.
pub const CONFIG_FILE_ENV: &str = "TUCANO_CONFIG_FILE";

/// The only `version` value this build understands.
///
/// The marker exists so a future schema can be told apart from this one instead
/// of being half-read: a file whose `version` is not this exact value is
/// refused by name, before any other field is looked at.
pub const CONFIG_VERSION: u32 = 1;

/// The configuration file as it appears on disk.
///
/// Every field beyond `version` is optional, because a file exists to supply
/// *some* settings and the rest fall through to the environment or to the
/// built-in default. The document is strict: `deny_unknown_fields` mirrors the
/// stored documents' posture, and the alternative — ignoring what we do not
/// recognise — is a deployment that boots believing it applied a setting it
/// never read.
///
/// `version` is the one required field. It is a plain `u32` rather than an
/// enum, so an unknown version is reported as the number it was, which is what
/// makes the error message actionable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    /// The schema marker; must equal [`CONFIG_VERSION`].
    pub version: u32,
    /// Whether requests must carry a valid access token.
    pub auth_required: Option<bool>,
    /// The HS256 signing secret, inline.
    ///
    /// May be a plain string or an AEAD-encrypted envelope. After
    /// [`ConfigFile::resolve_secrets`], the value is always
    /// [`SecretValue::Plain`].
    pub jwt_secret: Option<SecretValue>,
    /// Path to a file holding the signing secret.
    pub jwt_secret_file: Option<String>,
    /// How long a minted access token stays valid.
    pub access_token_ttl: Option<String>,
    /// How long a refresh token stays valid.
    pub refresh_token_ttl: Option<String>,
    /// The username created at startup when the store holds no accounts.
    pub bootstrap_username: Option<String>,
    /// The password for [`ConfigFile::bootstrap_username`].
    ///
    /// May be a plain string or an AEAD-encrypted envelope. After
    /// [`ConfigFile::resolve_secrets`], the value is always
    /// [`SecretValue::Plain`].
    pub bootstrap_password: Option<SecretValue>,
}

impl ConfigFile {
    /// Parses the document and checks its `version`, in that order.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Malformed`] if the text is not a [`ConfigFile`] — an
    /// unknown key, a field of the wrong type, or a `version` that is missing —
    /// or [`ConfigError::UnsupportedVersion`] if the marker names a schema this
    /// build does not implement.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let file: Self = serde_json::from_str(text).map_err(|source| ConfigError::Malformed {
            // serde's own message names an offending field or a line and
            // column, and it quotes *keys* rather than values where it quotes
            // anything at all — a secret lives in a value, never in a key name.
            detail: source.to_string(),
        })?;
        if file.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: file.version,
            });
        }
        Ok(file)
    }

    /// Reads and parses the file at `path`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::UnreadableFile`] when the file cannot be read, or
    /// whatever [`ConfigFile::parse`] returns.
    pub fn read(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|source| ConfigError::UnreadableFile { source })?;
        Self::parse(&text)
    }

    /// Decrypts every encrypted secret in the file, in place.
    ///
    /// After this call, every [`SecretValue`] in the file is
    /// [`SecretValue::Plain`]: the decrypted text replaces the envelope.
    /// `keys` is `None` when no key file is configured, which is fine if the
    /// file holds no encrypted values and a startup error if it does.
    ///
    /// # Errors
    ///
    /// [`ConfigError::DecryptionFailed`] wrapping the underlying
    /// [`SecretError`], which names the setting or key identifier at fault
    /// without carrying any secret value.
    pub fn resolve_secrets(&mut self, keys: Option<&KeyRing>) -> Result<(), ConfigError> {
        if let Some(ref value) = self.jwt_secret
            && value.is_encrypted()
        {
            let resolved = value.resolve(keys).map_err(ConfigError::DecryptionFailed)?;
            self.jwt_secret = Some(SecretValue::Plain(resolved));
        }
        if let Some(ref value) = self.bootstrap_password
            && value.is_encrypted()
        {
            let resolved = value.resolve(keys).map_err(ConfigError::DecryptionFailed)?;
            self.bootstrap_password = Some(SecretValue::Plain(resolved));
        }
        Ok(())
    }
}

/// Reads the configuration file named by [`CONFIG_FILE_ENV`], if any, and
/// decrypts any encrypted secrets using the supplied key ring.
///
/// `lookup` supplies the environment the same way
/// [`crate::auth::AuthConfig::from_lookup`] does, so the whole loader stays a
/// pure function of its inputs. `None` is not an error: it is the documented
/// "no configuration file" case, and the one an existing env-only deployment
/// hits unchanged.
///
/// `keys` is the key ring loaded from the key file. It is `None` when no key
/// file is configured, which is fine if the file holds no encrypted values
/// and a startup error if it does.
///
/// # Errors
///
/// [`ConfigError::UnreadableFile`] when a named file cannot be read — this
/// includes *missing*, because a variable that names a file which is not there
/// is a mistake to report rather than a file to skip silently: skipping would
/// bring the service up without the settings the operator meant to apply. Also
/// [`ConfigError::Malformed`], [`ConfigError::UnsupportedVersion`], and
/// [`ConfigError::DecryptionFailed`].
pub fn load(
    lookup: impl Fn(&str) -> Option<String>,
    keys: Option<&KeyRing>,
) -> Result<Option<ConfigFile>, ConfigError> {
    match lookup(CONFIG_FILE_ENV) {
        Some(path) => {
            let mut file = ConfigFile::read(Path::new(&path))?;
            file.resolve_secrets(keys)?;
            Ok(Some(file))
        }
        None => Ok(None),
    }
}

/// Loads the configuration file and the key ring from the process environment.
///
/// The key ring is loaded first, because it may be needed to decrypt the
/// configuration file. Both [`CONFIG_FILE_ENV`] and
/// [`secret::CONFIG_KEY_FILE_ENV`] are read from the environment.
///
/// # Errors
///
/// Any [`ConfigError`] from the file or the key ring; the caller is expected
/// to abort startup.
pub fn load_from_env() -> Result<Option<ConfigFile>, ConfigError> {
    let lookup = |key: &str| env::var(key).ok();
    let key_ring = load_key_ring_from_lookup(lookup).map_err(ConfigError::KeyRing)?;
    load(lookup, key_ring.as_ref())
}

/// A setting the server cannot start with.
///
/// The variants are deliberately few and each names the *setting* at fault.
/// None of them can carry a secret's value, the configuration file's path or
/// the file's contents: those are absent from this type by construction, so no
/// `Display` impl can leak them by accident. `detail` is the one free-text
/// field, and it is serde's own message about a key or a position.
#[derive(Debug)]
pub enum ConfigError {
    /// The named configuration file could not be read.
    ///
    /// Deliberately path-free: the ADR forbids a raw file path in an error. The
    /// message names `TUCANO_CONFIG_FILE` instead, which is what an operator
    /// needs in order to know which setting to look at.
    UnreadableFile { source: std::io::Error },
    /// The configuration file is not a valid document of this schema.
    Malformed { detail: String },
    /// The `version` marker is not [`CONFIG_VERSION`].
    UnsupportedVersion { found: u32 },
    /// An encrypted value in the configuration file could not be decrypted.
    ///
    /// Wraps a [`SecretError`] that names the setting or key identifier at
    /// fault. The wrapped error carries no secret value, key material, or raw
    /// ciphertext.
    DecryptionFailed(SecretError),
    /// The key ring file could not be loaded or parsed.
    ///
    /// Wraps a [`SecretError`] that names the setting. The wrapped error
    /// carries no key material or file contents.
    KeyRing(SecretError),
}

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnreadableFile { source } => write!(
                formatter,
                "{CONFIG_FILE_ENV} names a configuration file that cannot be read: {source}"
            ),
            Self::Malformed { detail } => write!(
                formatter,
                "the configuration file named by {CONFIG_FILE_ENV} is not valid: {detail}"
            ),
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "the configuration file named by {CONFIG_FILE_ENV} declares version {found}; \
                 this build implements version {CONFIG_VERSION}"
            ),
            Self::DecryptionFailed(source) => write!(
                formatter,
                "the configuration file contains an encrypted value that cannot be \
                 decrypted: {source}"
            ),
            Self::KeyRing(source) => {
                write!(formatter, "the key ring cannot be loaded: {source}")
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::UnreadableFile { source } => Some(source),
            Self::DecryptionFailed(source) => Some(source),
            Self::KeyRing(source) => Some(source),
            _ => None,
        }
    }
}

/// Where a setting's effective value came from.
///
/// Recorded so a caller can report precedence without re-deriving it, and so
/// the precedence tests can assert on provenance rather than only on the value
/// — "the environment won" and "the file happened to agree" are different
/// facts, and only the first is what the ADR requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The environment supplied the value.
    Environment,
    /// The configuration file supplied the value.
    File,
    /// Neither did; the built-in default applies.
    Default,
}

impl Display for Source {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment => formatter.write_str("the environment"),
            Self::File => formatter.write_str("the configuration file"),
            Self::Default => formatter.write_str("the built-in default"),
        }
    }
}

/// A resolved setting: its effective value and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting<T> {
    /// The effective value.
    pub value: T,
    /// Which of the three sources supplied it.
    pub source: Source,
}

/// Resolves and parses one key across the two sources, preferring the
/// environment.
///
/// This is the ADR's precedence rule in one place, so no caller can implement
/// it slightly differently: look in the environment, then in the file, then
/// take the built-in default. Precedence is per key, which is why the file side
/// arrives as a closure — each key's file spelling differs from its environment
/// spelling, and reading that one field belongs with the rest of the caller's
/// knowledge of the key.
///
/// `parse` is applied to whichever source won, and its error names the setting
/// rather than quoting the value. The environment's raw text is passed in
/// unchanged, so a bad value is rejected with the message it would have produced
/// before this module existed.
///
/// # Errors
///
/// Whatever `parse` returns for the winning source.
pub fn resolve<T>(
    lookup: &impl Fn(&str) -> Option<String>,
    env_key: &str,
    from_file: impl FnOnce() -> Option<String>,
    parse: impl Fn(&str) -> Result<T, ConfigError>,
    default: T,
) -> Result<Setting<T>, ConfigError> {
    if let Some(raw) = lookup(env_key) {
        return Ok(Setting {
            value: parse(&raw)?,
            source: Source::Environment,
        });
    }
    match from_file() {
        Some(raw) => Ok(Setting {
            value: parse(&raw)?,
            source: Source::File,
        }),
        None => Ok(Setting {
            value: default,
            source: Source::Default,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A secret long enough to satisfy the auth module's floor.
    const SECRET_OK: &str = "a-test-secret-long-enough-for-hs256";

    fn document(extra: &str) -> String {
        format!(r#"{{"version": {CONFIG_VERSION}{extra}}}"#)
    }

    fn parse(json: &str) -> Result<ConfigFile, ConfigError> {
        ConfigFile::parse(json)
    }

    #[test]
    fn an_empty_file_is_valid_and_supplies_nothing() {
        let file = parse(&document("")).expect("version only");
        assert_eq!(file.version, CONFIG_VERSION);
        assert!(file.auth_required.is_none());
        assert!(file.jwt_secret.is_none());
        assert!(file.access_token_ttl.is_none());
    }

    #[test]
    fn the_version_marker_is_required() {
        let error = parse(r#"{"auth_required": true}"#).expect_err("no version");
        assert!(matches!(error, ConfigError::Malformed { .. }), "{error:?}");
    }

    #[test]
    fn an_unknown_version_is_refused_by_number() {
        let error = parse(r#"{"version": 99}"#).expect_err("future version");
        match error {
            ConfigError::UnsupportedVersion { found } => assert_eq!(found, 99),
            other => panic!("expected an unsupported-version error, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_key_is_refused_rather_than_ignored() {
        // A typo in a key name is the failure mode the strict schema exists for.
        let error =
            parse(&document(r#", "jwt_secret_fil": "/run/secrets/jwt""#)).expect_err("unknown key");
        match error {
            ConfigError::Malformed { detail } => assert!(
                detail.contains("jwt_secret_fil"),
                "the message should name the offending key: {detail}"
            ),
            other => panic!("expected a malformed error, got {other:?}"),
        }
    }

    #[test]
    fn a_field_of_the_wrong_type_is_refused() {
        let error = parse(&document(r#", "auth_required": "yes""#)).expect_err("wrong type");
        assert!(matches!(error, ConfigError::Malformed { .. }), "{error:?}");
    }

    #[test]
    fn no_error_text_carries_a_secret_value() {
        // The one rule the ADR calls non-negotiable: a startup error names the
        // setting, never the value. Checked against a document that *does* hold
        // a secret, with the failure provoked elsewhere in the same file.
        let text = document(&format!(r#", "jwt_secret": "{SECRET_OK}", "not_a_key": 1"#));
        let error = parse(&text).expect_err("unknown key");
        let rendered = error.to_string();
        assert!(
            !rendered.contains(SECRET_OK),
            "a startup error must not echo a secret value: {rendered}"
        );
    }

    #[test]
    fn an_unreadable_file_refuses_to_start_without_naming_the_path() {
        let error = ConfigFile::read(Path::new("/definitely/not/here.json")).expect_err("missing");
        let rendered = error.to_string();
        assert!(
            rendered.contains(CONFIG_FILE_ENV),
            "the message should name the setting: {rendered}"
        );
        assert!(
            !rendered.contains("/definitely/not/here.json"),
            "a startup error must not echo the raw path: {rendered}"
        );
    }

    #[test]
    fn a_valid_file_is_read_from_disk() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("tucano-config.json");
        std::fs::write(&path, document(r#", "auth_required": true"#)).expect("write file");
        let file = ConfigFile::read(&path).expect("read file");
        assert_eq!(file.auth_required, Some(true));
    }

    #[test]
    fn no_variable_means_no_file_and_no_error() {
        let loaded = load(|_| None, None).expect("absent file");
        assert!(loaded.is_none());
    }

    #[test]
    fn a_named_file_is_loaded_through_the_lookup() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("tucano-config.json");
        std::fs::write(&path, document("")).expect("write file");
        let loaded = load(
            |key| (key == CONFIG_FILE_ENV).then(|| path.to_str().expect("utf-8 path").to_owned()),
            None,
        )
        .expect("named file");
        assert_eq!(loaded.expect("file present").version, CONFIG_VERSION);
    }

    #[test]
    fn a_variable_naming_a_missing_file_is_an_error_not_a_skip() {
        // Silently skipping would let a deployment come up without the settings
        // the operator meant to apply, which is the failure mode the ADR names.
        let error = load(
            |key| (key == CONFIG_FILE_ENV).then(|| "/definitely/not/here.json".to_owned()),
            None,
        )
        .expect_err("missing named file");
        assert!(
            matches!(error, ConfigError::UnreadableFile { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn resolve_gives_the_environment_priority_over_the_file() {
        let setting = resolve(
            &|key| (key == "K").then(|| "from-env".to_owned()),
            "K",
            || Some("from-file".to_owned()),
            |raw| Ok(raw.to_owned()),
            "default".to_owned(),
        )
        .expect("resolve");
        assert_eq!(setting.value, "from-env");
        assert_eq!(setting.source, Source::Environment);
    }

    #[test]
    fn resolve_falls_back_to_the_file() {
        let setting = resolve(
            &|_| None,
            "K",
            || Some("from-file".to_owned()),
            |raw| Ok(raw.to_owned()),
            "default".to_owned(),
        )
        .expect("resolve");
        assert_eq!(setting.value, "from-file");
        assert_eq!(setting.source, Source::File);
    }

    #[test]
    fn resolve_falls_back_to_the_default() {
        let setting = resolve(
            &|_| None,
            "K",
            || None,
            |raw| Ok(raw.to_owned()),
            "default".to_owned(),
        )
        .expect("resolve");
        assert_eq!(setting.value, "default");
        assert_eq!(setting.source, Source::Default);
    }

    #[test]
    fn resolve_reports_a_bad_winning_value_by_setting() {
        // The parse error comes from the caller: it is the caller that knows
        // the spelling of the key it is reading.
        let error = resolve(
            &|key| (key == "K").then(|| "nonsense".to_owned()),
            "K",
            || None,
            |_| {
                Err(ConfigError::Malformed {
                    detail: "K is not a value the server understands".to_owned(),
                })
            },
            "default".to_owned(),
        )
        .expect_err("bad value");
        assert!(error.to_string().contains('K'), "{error}");
    }

    #[test]
    fn the_shipped_example_is_a_valid_document_of_this_schema() {
        // The example is documentation, and documentation that has drifted is
        // worse than none: this keeps it parseable and keeps its version marker
        // matching the build.
        let example = include_str!("../../docs/deployment/config.example.json");
        let file = ConfigFile::parse(example).expect("the shipped example must be valid");
        assert_eq!(file.version, CONFIG_VERSION);
    }

    // -- Encrypted secrets integration tests -----------------------------------

    fn test_key() -> [u8; secret::KEY_LENGTH] {
        let mut key = [0u8; secret::KEY_LENGTH];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(7).wrapping_add(13);
        }
        key
    }

    #[test]
    fn an_encrypted_jwt_secret_decrypts_with_the_key_ring() {
        let key = test_key();
        let envelope = secret::encrypt_value(b"a-test-secret-long-enough-for-hs256", &key, "k1")
            .expect("encrypt");
        let json = serde_json::json!({
            "version": CONFIG_VERSION,
            "auth_required": true,
            "jwt_secret": envelope,
        });
        let mut file = ConfigFile::parse(&json.to_string()).expect("parse encrypted config");
        let ring = secret::KeyRing::new(vec![("k1".to_owned(), key)]);
        file.resolve_secrets(Some(&ring)).expect("resolve");
        match file.jwt_secret {
            Some(SecretValue::Plain(ref s)) => {
                assert_eq!(s, "a-test-secret-long-enough-for-hs256")
            }
            other => panic!("expected a resolved plain secret, got {other:?}"),
        }
    }

    #[test]
    fn an_encrypted_secret_without_a_key_ring_fails_closed() {
        let key = test_key();
        let envelope = secret::encrypt_value(b"a-test-secret-long-enough-for-hs256", &key, "k1")
            .expect("encrypt");
        let json = serde_json::json!({
            "version": CONFIG_VERSION,
            "jwt_secret": envelope,
        });
        let mut file = ConfigFile::parse(&json.to_string()).expect("parse encrypted config");
        let error = file.resolve_secrets(None).expect_err("no key ring");
        assert!(
            matches!(error, ConfigError::DecryptionFailed(_)),
            "{error:?}"
        );
    }

    #[test]
    fn mixed_plain_and_encrypted_secrets_resolve_correctly() {
        let key = test_key();
        let envelope =
            secret::encrypt_value(b"the-encrypted-password-value!!", &key, "k1").expect("encrypt");
        let json = serde_json::json!({
            "version": CONFIG_VERSION,
            "jwt_secret": "a-plain-secret-long-enough-for-hs256",
            "bootstrap_username": "admin",
            "bootstrap_password": envelope,
        });
        let mut file = ConfigFile::parse(&json.to_string()).expect("parse mixed config");
        let ring = secret::KeyRing::new(vec![("k1".to_owned(), key)]);
        file.resolve_secrets(Some(&ring)).expect("resolve");

        match file.jwt_secret {
            Some(SecretValue::Plain(ref s)) => {
                assert_eq!(s, "a-plain-secret-long-enough-for-hs256")
            }
            other => panic!("jwt_secret should be plain, got {other:?}"),
        }
        match file.bootstrap_password {
            Some(SecretValue::Plain(ref s)) => {
                assert_eq!(s, "the-encrypted-password-value!!")
            }
            other => panic!("bootstrap_password should be plain, got {other:?}"),
        }
        assert_eq!(
            file.bootstrap_username.as_deref(),
            Some("admin"),
            "non-secret fields are unaffected"
        );
    }

    #[test]
    fn the_loader_decrypts_using_the_key_ring() {
        let directory = tempfile::tempdir().expect("tempdir");

        let key = test_key();
        let envelope = secret::encrypt_value(b"a-test-secret-long-enough-for-hs256", &key, "k1")
            .expect("encrypt");

        let config_path = directory.path().join("config.json");
        let config_json = serde_json::json!({
            "version": CONFIG_VERSION,
            "jwt_secret": envelope,
        });
        std::fs::write(&config_path, config_json.to_string()).expect("write config");

        let ring = secret::KeyRing::new(vec![("k1".to_owned(), key)]);
        let loaded = load(
            |name| {
                (name == CONFIG_FILE_ENV).then(|| config_path.to_str().expect("utf-8").to_owned())
            },
            Some(&ring),
        )
        .expect("load encrypted config");

        let file = loaded.expect("file present");
        match file.jwt_secret {
            Some(SecretValue::Plain(ref s)) => {
                assert_eq!(s, "a-test-secret-long-enough-for-hs256")
            }
            other => panic!("expected resolved secret, got {other:?}"),
        }
    }

    #[test]
    fn no_startup_error_from_an_encrypted_config_carries_a_secret() {
        let key = test_key();
        let secret_value = "recognisable-secret-do-not-leak-me";
        let envelope =
            secret::encrypt_value(secret_value.as_bytes(), &key, "missing-key").expect("encrypt");
        let json = serde_json::json!({
            "version": CONFIG_VERSION,
            "jwt_secret": envelope,
        });
        let mut file = ConfigFile::parse(&json.to_string()).expect("parse encrypted config");

        let ring = secret::KeyRing::new(vec![("different-key".to_owned(), key)]);
        let error = file
            .resolve_secrets(Some(&ring))
            .expect_err("key id mismatch");
        let rendered = error.to_string();
        assert!(
            !rendered.contains(secret_value),
            "a startup error must not echo a secret value: {rendered}"
        );
    }
}
