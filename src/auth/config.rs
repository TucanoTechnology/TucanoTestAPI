//! The auth settings a running server needs, resolved once at startup.
//!
//! Everything here is a pure function of an environment lookup, which is what
//! lets the tests drive every branch through [`AuthConfig::from_lookup`] with a
//! closure. Reading the process environment directly would have meant mutating
//! it — `std::env::set_var` is `unsafe` in edition 2024, and the crate forbids
//! `unsafe_code` — and global state would have made these tests order-dependent
//! even if it were safe.
//!
//! Configuration errors are startup errors, not request errors: they are
//! returned from `from_env` so the process can refuse to boot with a secret too
//! short to sign anything, rather than come up and fail strangely later.

use std::env;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

/// The smallest HS256 signing secret the server will start with, in bytes.
///
/// A key shorter than the digest it feeds weakens the signature below what the
/// algorithm can carry. Refusing it at startup turns a silent weakness into an
/// operator error the operator can act on.
pub const MIN_SECRET_BYTES: usize = 32;

/// Access-token lifetime when `TUCANO_ACCESS_TOKEN_TTL` is unset.
pub const DEFAULT_ACCESS_TTL: Duration = Duration::from_secs(15 * 60);

/// Refresh-token lifetime when `TUCANO_REFRESH_TOKEN_TTL` is unset.
pub const DEFAULT_REFRESH_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The auth settings resolved from the environment.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Whether requests must carry a valid access token.
    ///
    /// Defaults to `false`, which is the trusted-network behaviour the API had
    /// before it had accounts at all: nothing changes for an existing
    /// deployment until an operator opts in.
    pub required: bool,
    /// The HS256 signing secret.
    ///
    /// Present whenever `required` is set, and absent by default; a server that
    /// enforces auth cannot be configured without one.
    pub jwt_secret: Option<Vec<u8>>,
    /// How long a minted access token stays valid.
    pub access_ttl: Duration,
    /// How long a refresh token stays valid.
    pub refresh_ttl: Duration,
    /// The username created at startup when the store holds no accounts.
    pub bootstrap_username: Option<String>,
    /// The password for [`AuthConfig::bootstrap_username`].
    pub bootstrap_password: Option<String>,
}

impl AuthConfig {
    /// Resolves the configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Any [`ConfigError`]; the caller is expected to abort startup.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// Resolves the configuration from `lookup`, in the order the settings are
    /// documented: the flag, then the lifetimes, then the secret.
    ///
    /// # Errors
    ///
    /// Any [`ConfigError`].
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = match lookup("TUCANO_AUTH_REQUIRED") {
            Some(raw) => parse_bool(&raw)?,
            None => false,
        };
        let access_ttl = match lookup("TUCANO_ACCESS_TOKEN_TTL") {
            Some(raw) => parse_duration(&raw, "TUCANO_ACCESS_TOKEN_TTL")?,
            None => DEFAULT_ACCESS_TTL,
        };
        let refresh_ttl = match lookup("TUCANO_REFRESH_TOKEN_TTL") {
            Some(raw) => parse_duration(&raw, "TUCANO_REFRESH_TOKEN_TTL")?,
            None => DEFAULT_REFRESH_TTL,
        };

        let jwt_secret = match (
            lookup("TUCANO_JWT_SECRET"),
            lookup("TUCANO_JWT_SECRET_FILE"),
        ) {
            (Some(_), Some(_)) => return Err(ConfigError::SecretSourcesConflict),
            (Some(inline), None) => Some(inline.into_bytes()),
            (None, Some(path)) => Some(read_secret_file(&path)?),
            (None, None) => None,
        };
        match jwt_secret.as_ref().map(Vec::len) {
            Some(length) if length < MIN_SECRET_BYTES => {
                return Err(ConfigError::ShortSecret { length });
            }
            Some(_) => {}
            // A disabled server never signs anything, so it needs no secret;
            // an enforcing one cannot be allowed to start without one.
            None if required => return Err(ConfigError::MissingSecret),
            None => {}
        }

        let bootstrap_username = lookup("TUCANO_BOOTSTRAP_USERNAME");
        let bootstrap_password = lookup("TUCANO_BOOTSTRAP_PASSWORD");
        if bootstrap_username.is_some() != bootstrap_password.is_some() {
            return Err(ConfigError::IncompleteBootstrap);
        }

        Ok(Self {
            required,
            jwt_secret,
            access_ttl,
            refresh_ttl,
            bootstrap_username,
            bootstrap_password,
        })
    }
}

/// A setting the server cannot start with.
#[derive(Debug)]
pub enum ConfigError {
    /// `TUCANO_AUTH_REQUIRED` is not a value the server understands.
    InvalidBool { value: String },
    /// A lifetime is empty, zero, unparseable, or too large to be a duration.
    InvalidTtl { key: &'static str, value: String },
    /// Both `TUCANO_JWT_SECRET` and `TUCANO_JWT_SECRET_FILE` were set.
    SecretSourcesConflict,
    /// Auth is required but no signing secret was configured.
    MissingSecret,
    /// The signing secret is present but shorter than [`MIN_SECRET_BYTES`].
    ShortSecret { length: usize },
    /// The secret file could not be read.
    SecretFile {
        path: String,
        source: std::io::Error,
    },
    /// Only one of the two bootstrap variables was set.
    IncompleteBootstrap,
}

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBool { value } => write!(
                formatter,
                "TUCANO_AUTH_REQUIRED must be true or false, not {value:?}"
            ),
            Self::InvalidTtl { key, value } => {
                write!(formatter, "{key} is not a duration: {value:?}")
            }
            Self::SecretSourcesConflict => write!(
                formatter,
                "set either TUCANO_JWT_SECRET or TUCANO_JWT_SECRET_FILE, not both"
            ),
            Self::MissingSecret => write!(
                formatter,
                "TUCANO_AUTH_REQUIRED is set, so TUCANO_JWT_SECRET or TUCANO_JWT_SECRET_FILE must be too"
            ),
            Self::ShortSecret { length } => write!(
                formatter,
                "the JWT signing secret is {length} bytes; at least {MIN_SECRET_BYTES} are required"
            ),
            Self::SecretFile { path, source } => {
                write!(
                    formatter,
                    "cannot read TUCANO_JWT_SECRET_FILE {path:?}: {source}"
                )
            }
            Self::IncompleteBootstrap => write!(
                formatter,
                "set TUCANO_BOOTSTRAP_USERNAME and TUCANO_BOOTSTRAP_PASSWORD together"
            ),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SecretFile { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Parses a boolean flag, accepting the spellings an operator is likely to
/// write and nothing else.
fn parse_bool(raw: &str) -> Result<bool, ConfigError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(ConfigError::InvalidBool {
            value: raw.to_owned(),
        }),
    }
}

/// Parses a lifetime: bare seconds, or a `s`, `m`, `h`, or `d` suffix.
///
/// A zero length is rejected rather than silently accepted, because a token
/// that expires the instant it is issued is always a mistake, and the failure
/// belongs at startup where the operator can see it.
fn parse_duration(raw: &str, key: &'static str) -> Result<Duration, ConfigError> {
    let trimmed = raw.trim();
    let (digits, scale) = match trimmed.as_bytes().last() {
        Some(b's') => (&trimmed[..trimmed.len() - 1], 1_u64),
        Some(b'm') => (&trimmed[..trimmed.len() - 1], 60_u64),
        Some(b'h') => (&trimmed[..trimmed.len() - 1], 3_600_u64),
        Some(b'd') => (&trimmed[..trimmed.len() - 1], 86_400_u64),
        Some(_) => (trimmed, 1_u64),
        None => return Err(invalid_ttl(key, raw)),
    };
    let count: u64 = digits.trim().parse().map_err(|_| invalid_ttl(key, raw))?;
    if count == 0 {
        return Err(invalid_ttl(key, raw));
    }
    let seconds = count
        .checked_mul(scale)
        .ok_or_else(|| invalid_ttl(key, raw))?;
    Ok(Duration::from_secs(seconds))
}

fn invalid_ttl(key: &'static str, value: &str) -> ConfigError {
    ConfigError::InvalidTtl {
        key,
        value: value.to_owned(),
    }
}

/// Reads a secret file and trims the whitespace around its contents, so a
/// secret written by `echo` or `printf` is usable as it stands.
fn read_secret_file(path: &str) -> Result<Vec<u8>, ConfigError> {
    let contents = std::fs::read(path).map_err(|source| ConfigError::SecretFile {
        path: path.to_owned(),
        source,
    })?;
    let start = contents
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(contents.len());
    let end = contents
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    Ok(contents[start..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A secret long enough to pass [`MIN_SECRET_BYTES`].
    const SECRET_OK: &str = "a-test-secret-long-enough-for-hs256";

    fn config(pairs: &[(&str, &str)]) -> Result<AuthConfig, ConfigError> {
        AuthConfig::from_lookup(|key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn auth_is_disabled_by_default_and_needs_no_secret() {
        let config = config(&[]).expect("defaults");
        assert!(!config.required);
        assert!(config.jwt_secret.is_none());
        assert_eq!(config.access_ttl, DEFAULT_ACCESS_TTL);
        assert_eq!(config.refresh_ttl, DEFAULT_REFRESH_TTL);
        assert!(config.bootstrap_username.is_none());
        assert!(config.bootstrap_password.is_none());
    }

    #[test]
    fn requiring_auth_without_a_secret_refuses_to_start() {
        let error = config(&[("TUCANO_AUTH_REQUIRED", "true")]).expect_err("no secret");
        assert!(matches!(error, ConfigError::MissingSecret), "{error:?}");
    }

    #[test]
    fn a_secret_shorter_than_the_minimum_refuses_to_start() {
        let error = config(&[("TUCANO_JWT_SECRET", "too-short")]).expect_err("short secret");
        match error {
            ConfigError::ShortSecret { length } => assert_eq!(length, "too-short".len()),
            other => panic!("expected a short-secret error, got {other:?}"),
        }
    }

    #[test]
    fn an_inline_secret_is_used_as_written() {
        let config = config(&[("TUCANO_JWT_SECRET", SECRET_OK)]).expect("config");
        assert_eq!(config.jwt_secret.as_deref(), Some(SECRET_OK.as_bytes()));
    }

    #[test]
    fn inline_and_file_secrets_cannot_both_be_set() {
        let error = config(&[
            ("TUCANO_JWT_SECRET", SECRET_OK),
            ("TUCANO_JWT_SECRET_FILE", "/tmp/never-read"),
        ])
        .expect_err("both sources");
        assert!(
            matches!(error, ConfigError::SecretSourcesConflict),
            "{error:?}"
        );
    }

    #[test]
    fn a_secret_file_is_read_and_its_trailing_newline_trimmed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("jwt.secret");
        std::fs::write(&path, format!("{SECRET_OK}\n")).expect("write secret");
        let config = config(&[
            ("TUCANO_AUTH_REQUIRED", "true"),
            ("TUCANO_JWT_SECRET_FILE", path.to_str().expect("utf-8 path")),
        ])
        .expect("config");
        assert_eq!(config.jwt_secret.as_deref(), Some(SECRET_OK.as_bytes()));
        assert!(config.required);
    }

    #[test]
    fn an_unreadable_secret_file_refuses_to_start() {
        let error = config(&[("TUCANO_JWT_SECRET_FILE", "/definitely/not/here")])
            .expect_err("missing file");
        assert!(matches!(error, ConfigError::SecretFile { .. }), "{error:?}");
    }

    #[test]
    fn durations_accept_seconds_and_unit_suffixes() {
        assert_eq!(
            parse_duration("900", "K").expect("seconds"),
            Duration::from_secs(900)
        );
        assert_eq!(
            parse_duration(" 15m ", "K").expect("minutes"),
            DEFAULT_ACCESS_TTL
        );
        assert_eq!(
            parse_duration("2h", "K").expect("hours"),
            Duration::from_secs(7_200)
        );
        assert_eq!(
            parse_duration("14d", "K").expect("days"),
            DEFAULT_REFRESH_TTL
        );
    }

    #[test]
    fn an_unusable_duration_refuses_to_start() {
        for raw in [
            "",
            "   ",
            "m",
            "0",
            "0s",
            "abc",
            "-5",
            "1.5h",
            "999999999999999999999",
            "9999999999999999d",
        ] {
            assert!(
                matches!(
                    parse_duration(raw, "TUCANO_ACCESS_TOKEN_TTL"),
                    Err(ConfigError::InvalidTtl { .. })
                ),
                "{raw:?} should not parse"
            );
        }
    }

    #[test]
    fn a_configured_access_lifetime_replaces_the_default() {
        let config = config(&[
            ("TUCANO_JWT_SECRET", SECRET_OK),
            ("TUCANO_ACCESS_TOKEN_TTL", "30m"),
            ("TUCANO_REFRESH_TOKEN_TTL", "7d"),
        ])
        .expect("config");
        assert_eq!(config.access_ttl, Duration::from_secs(1_800));
        assert_eq!(config.refresh_ttl, Duration::from_secs(604_800));
    }

    #[test]
    fn the_required_flag_accepts_the_usual_spellings() {
        for (raw, expected) in [
            ("true", true),
            ("TRUE", true),
            (" 1 ", true),
            ("yes", true),
            ("on", true),
            ("false", false),
            ("0", false),
            ("no", false),
            ("off", false),
        ] {
            let config = config(&[
                ("TUCANO_AUTH_REQUIRED", raw),
                ("TUCANO_JWT_SECRET", SECRET_OK),
            ])
            .unwrap_or_else(|error| panic!("{raw:?} should parse: {error}"));
            assert_eq!(config.required, expected, "{raw:?}");
        }
    }

    #[test]
    fn the_required_flag_rejects_anything_else() {
        let error = config(&[
            ("TUCANO_AUTH_REQUIRED", "maybe"),
            ("TUCANO_JWT_SECRET", SECRET_OK),
        ])
        .expect_err("unrecognised flag");
        match error {
            ConfigError::InvalidBool { value } => assert_eq!(value, "maybe"),
            other => panic!("expected a boolean error, got {other:?}"),
        }
    }

    #[test]
    fn bootstrap_credentials_are_accepted_as_a_pair() {
        let config = config(&[
            ("TUCANO_JWT_SECRET", SECRET_OK),
            ("TUCANO_BOOTSTRAP_USERNAME", "admin"),
            ("TUCANO_BOOTSTRAP_PASSWORD", "hunter2"),
        ])
        .expect("config");
        assert_eq!(config.bootstrap_username.as_deref(), Some("admin"));
        assert_eq!(config.bootstrap_password.as_deref(), Some("hunter2"));
    }

    #[test]
    fn half_a_bootstrap_pair_refuses_to_start() {
        for lone in [
            ("TUCANO_BOOTSTRAP_USERNAME", "admin"),
            ("TUCANO_BOOTSTRAP_PASSWORD", "hunter2"),
        ] {
            let error = config(&[("TUCANO_JWT_SECRET", SECRET_OK), lone]).expect_err("half a pair");
            assert!(
                matches!(error, ConfigError::IncompleteBootstrap),
                "{lone:?}: {error:?}"
            );
        }
    }
}
