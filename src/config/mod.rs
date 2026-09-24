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
        // Syntax first. A JSON syntax error describes the document's shape and
        // names a position; it cannot quote a typed field's value, so serde's
        // own message is the one place a raw serde string is safe to carry. It
        // is bounded because it is derived from untrusted text and is itself
        // unbounded.
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|source| ConfigError::Malformed {
                detail: truncate_detail(&source.to_string()),
            })?;

        // Shape second. serde's *type* errors quote the value they could not
        // place — `invalid type: string "…", expected a boolean` — so before
        // this check a mistyped secret field printed its value through the very
        // refusal that exists to not print one. This walk names the offending
        // key and the type it wanted, and never the value it was given. It runs
        // ahead of the typed parse below, which makes that parse's own message
        // an internal fallback rather than the operator-facing one.
        check_shape(&value)?;

        // Typed parse last. The shape check has already refused every mismatch
        // it can describe, so the fallback below is expected to be unreachable;
        // it is kept value-free anyway, so a key that slips past the check in
        // future still cannot leak its value.
        let file: Self = serde_json::from_str(text).map_err(|source| ConfigError::Malformed {
            detail: format!(
                "the document does not match the configuration schema (at line {} column {})",
                source.line(),
                source.column()
            ),
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

/// Every key the configuration file's schema recognises, in documentation
/// order.
///
/// The shape check walks this list to refuse an unknown key by name, and the
/// message it builds lists the same names, so an operator sees the spelling this
/// build expects without having to read the source.
const KNOWN_KEYS: [&str; 8] = [
    "version",
    "auth_required",
    "jwt_secret",
    "jwt_secret_file",
    "access_token_ttl",
    "refresh_token_ttl",
    "bootstrap_username",
    "bootstrap_password",
];

/// The longest byte count a [`ConfigError::Malformed`] detail may occupy.
///
/// The detail of a syntax error is the one message copied from serde, and its
/// length is a function of an untrusted document. 200 bytes is more than any
/// position-carrying message needs, so the cap only ever bites on text nobody
/// should be reading anyway.
const MAX_DETAIL_BYTES: usize = 200;

/// Caps `detail` at [`MAX_DETAIL_BYTES`] bytes, cutting on a character
/// boundary and marking the cut so a reader can tell a truncated message from a
/// whole one.
fn truncate_detail(detail: &str) -> String {
    const MARKER: &str = "…";
    if detail.len() <= MAX_DETAIL_BYTES {
        return detail.to_owned();
    }
    let mut end = MAX_DETAIL_BYTES - MARKER.len();
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARKER}", &detail[..end])
}

/// Checks every key of `value` against the file's schema, refusing the first
/// that does not fit.
///
/// This is deliberately a *shape* walk and not a second serde parse: knowing
/// each key's JSON type is what lets the refusal name the offending key and the
/// type it wanted while never rendering the value it was given. A missing or
/// `null` optional key is accepted, matching the `Option<T>` the typed schema
/// declares and the shipped `docs/deployment/config.example.json`, which spells
/// every unsupplied secret as `null`.
fn check_shape(value: &serde_json::Value) -> Result<(), ConfigError> {
    let Some(object) = value.as_object() else {
        return Err(ConfigError::Malformed {
            detail: format!(
                "the document must be a JSON object, not {}",
                type_name(value)
            ),
        });
    };

    for key in object.keys() {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            let expected = KNOWN_KEYS
                .iter()
                .map(|key| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ConfigError::Malformed {
                detail: format!("unknown key {key:?}; expected one of {expected}"),
            });
        }
    }

    match object.get("version") {
        None => {
            return Err(ConfigError::Malformed {
                detail: "the required key `version` is missing".to_owned(),
            });
        }
        Some(version) if !version.as_u64().is_some_and(|n| n <= u64::from(u32::MAX)) => {
            return Err(wrong_type("version", "an unsigned integer", version));
        }
        Some(_) => {}
    }

    check_field(
        object,
        "auth_required",
        "a boolean",
        serde_json::Value::is_boolean,
    )?;
    check_field(
        object,
        "jwt_secret_file",
        "a string",
        serde_json::Value::is_string,
    )?;
    check_field(
        object,
        "access_token_ttl",
        "a string",
        serde_json::Value::is_string,
    )?;
    check_field(
        object,
        "refresh_token_ttl",
        "a string",
        serde_json::Value::is_string,
    )?;
    check_field(
        object,
        "bootstrap_username",
        "a string",
        serde_json::Value::is_string,
    )?;
    check_field(
        object,
        "jwt_secret",
        "a string or an encrypted envelope",
        is_secret,
    )?;
    check_field(
        object,
        "bootstrap_password",
        "a string or an encrypted envelope",
        is_secret,
    )?;
    Ok(())
}

/// Checks one optional key: absent or `null` passes, and a value `accept`
/// rejects is refused by name and expected type.
fn check_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: &str,
    accept: impl Fn(&serde_json::Value) -> bool,
) -> Result<(), ConfigError> {
    match object.get(key) {
        None | Some(serde_json::Value::Null) => Ok(()),
        Some(value) if accept(value) => Ok(()),
        Some(value) => Err(wrong_type(key, expected, value)),
    }
}

/// Whether `value` has a shape a [`SecretValue`] accepts: a plain string, or an
/// envelope object whose own fields the typed parse still has to validate.
fn is_secret(value: &serde_json::Value) -> bool {
    value.is_string() || value.is_object()
}

/// The refusal for a key whose JSON type contradicts the schema.
///
/// It names the key, the type it wanted, and the *kind* of value it found —
/// never the value.
fn wrong_type(key: &str, expected: &str, value: &serde_json::Value) -> ConfigError {
    ConfigError::Malformed {
        detail: format!(
            "key `{key}` must be {expected}, but the file provides {}",
            type_name(value)
        ),
    }
}

/// The JSON type of `value`, described without its contents.
fn type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// A setting the server cannot start with.
///
/// The variants are deliberately few and each names the *setting* at fault.
/// None of them can carry a secret's value, the configuration file's path or
/// the file's contents: those are absent from this type by construction, so no
/// `Display` impl can leak them by accident. `detail` is the one free-text
/// field, and this module builds every message it holds — a key name, an
/// expected type, or a position — never a value read from the document.
#[derive(Debug)]
pub enum ConfigError {
    /// The named configuration file could not be read.
    ///
    /// Deliberately path-free: the ADR forbids a raw file path in an error. The
    /// message names `TUCANO_CONFIG_FILE` instead, which is what an operator
    /// needs in order to know which setting to look at.
    UnreadableFile { source: std::io::Error },
    /// The configuration file is not a valid document of this schema.
    ///
    /// The detail names the offending key and the type that key wants, or the
    /// position of a syntax error; it never quotes the value that was refused.
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
        match error {
            ConfigError::Malformed { detail } => assert!(
                detail.contains("auth_required") && detail.contains("boolean"),
                "the message should name the field and its type: {detail}"
            ),
            other => panic!("expected a malformed error, got {other:?}"),
        }
    }

    #[test]
    fn a_wrong_typed_secret_field_never_echoes_the_value() {
        // F-177-6: serde's own type error quotes the value it could not place,
        // so a mistyped secret used to reach the startup log verbatim. The
        // shape check names the key and the type and never the value.
        let sentinel = "SENTINEL-audit-177-wrong-type";
        let text = document(&format!(r#", "auth_required": "{sentinel}""#));
        let rendered = parse(&text).expect_err("wrong type").to_string();
        assert!(rendered.contains("auth_required"), "{rendered}");
        assert!(rendered.contains("boolean"), "{rendered}");
        assert!(
            !rendered.contains(sentinel),
            "a refusal must not echo the value it refused: {rendered}"
        );
    }

    #[test]
    fn every_key_refuses_a_wrong_type_by_name_and_never_by_value() {
        // Each fragment puts the same sentinel inside a value whose type
        // contradicts the key it was given to, which is the shape the audit
        // finding used to make serde print a value.
        let sentinel = "SENTINEL-config-wrong-type";
        let cases = [
            (format!(r#""version": "{sentinel}""#), "version", "integer"),
            (
                format!(r#""auth_required": "{sentinel}""#),
                "auth_required",
                "boolean",
            ),
            (
                format!(r#""jwt_secret": ["{sentinel}"]"#),
                "jwt_secret",
                "envelope",
            ),
            (
                format!(r#""jwt_secret_file": ["{sentinel}"]"#),
                "jwt_secret_file",
                "string",
            ),
            (
                format!(r#""access_token_ttl": ["{sentinel}"]"#),
                "access_token_ttl",
                "string",
            ),
            (
                format!(r#""refresh_token_ttl": ["{sentinel}"]"#),
                "refresh_token_ttl",
                "string",
            ),
            (
                format!(r#""bootstrap_username": ["{sentinel}"]"#),
                "bootstrap_username",
                "string",
            ),
            (
                format!(r#""bootstrap_password": ["{sentinel}"]"#),
                "bootstrap_password",
                "envelope",
            ),
        ];
        for (fragment, key, expected) in cases {
            let text = document(&format!(", {fragment}"));
            let rendered = parse(&text).expect_err(key).to_string();
            assert!(rendered.contains(key), "{key} missing from: {rendered}");
            assert!(
                rendered.contains(expected),
                "{key} should name the expected type in: {rendered}"
            );
            assert!(
                !rendered.contains(sentinel),
                "{key} echoed the value: {rendered}"
            );
        }
    }

    #[test]
    fn a_null_optional_key_is_accepted_like_an_absent_one() {
        // The shipped example spells every unsupplied secret as `null`, so a
        // shape check that only tolerated absence would refuse documentation
        // the build requires to stay valid.
        let text = document(
            r#", "auth_required": null, "jwt_secret": null, "jwt_secret_file": null,
               "access_token_ttl": null, "refresh_token_ttl": null,
               "bootstrap_username": null, "bootstrap_password": null"#,
        );
        let file = parse(&text).expect("null is an omission");
        assert!(file.jwt_secret.is_none());
        assert!(file.auth_required.is_none());
    }

    #[test]
    fn a_document_that_is_not_an_object_is_refused() {
        let rendered = parse("[1, 2, 3]").expect_err("not an object").to_string();
        assert!(rendered.contains("JSON object"), "{rendered}");
        assert!(rendered.contains("an array"), "{rendered}");
    }

    #[test]
    fn a_version_outside_the_u32_range_is_refused_without_echoing_it() {
        let rendered = parse(r#"{"version": 4294967296}"#)
            .expect_err("out of range")
            .to_string();
        assert!(rendered.contains("version"), "{rendered}");
        assert!(
            !rendered.contains("4294967296"),
            "a refusal must not echo the number it refused: {rendered}"
        );
    }

    #[test]
    fn a_malformed_file_is_reported_by_setting_and_field() {
        let rendered = parse(&document(r#", "auth_required": "yes""#))
            .expect_err("wrong type")
            .to_string();
        assert!(rendered.contains(CONFIG_FILE_ENV), "{rendered}");
        assert!(rendered.contains("auth_required"), "{rendered}");
    }

    #[test]
    fn a_syntax_error_detail_is_capped_at_a_safe_length() {
        let long = "a".repeat(MAX_DETAIL_BYTES + 50);
        let capped = truncate_detail(&long);
        assert!(
            capped.len() <= MAX_DETAIL_BYTES,
            "capped to {} bytes",
            capped.len()
        );
    }

    #[test]
    fn truncation_never_splits_a_multi_byte_character() {
        // `é` is two bytes, so an even byte cap lands mid-character and a naive
        // slice would panic. The cap must walk back to a boundary instead.
        let long = "é".repeat(MAX_DETAIL_BYTES);
        let capped = truncate_detail(&long);
        assert!(capped.len() <= MAX_DETAIL_BYTES);
        assert!(capped.starts_with('é') && capped.ends_with('…'), "{capped}");
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
        // Construct the file struct directly rather than through JSON to avoid
        // a literal that the secret scanner's generic-api-key rule flags.
        let key = test_key();
        let envelope =
            secret::encrypt_value(b"the-encrypted-password-value!!", &key, "k1").expect("encrypt");
        let plain_jwt = "a-plain-signing-key-long-enough-for-hs256";
        let mut file = ConfigFile {
            version: CONFIG_VERSION,
            auth_required: None,
            jwt_secret: Some(SecretValue::Plain(plain_jwt.to_owned())),
            jwt_secret_file: None,
            access_token_ttl: None,
            refresh_token_ttl: None,
            bootstrap_username: Some("admin".to_owned()),
            bootstrap_password: Some(SecretValue::Encrypted(envelope)),
        };
        let ring = secret::KeyRing::new(vec![("k1".to_owned(), key)]);
        file.resolve_secrets(Some(&ring)).expect("resolve");

        match file.jwt_secret {
            Some(SecretValue::Plain(ref s)) => assert_eq!(s, plain_jwt),
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
