//! Access tokens, refresh tokens, and the digest the store keeps at rest.
//!
//! The access token is a compact HS256 JWT: base64url header, base64url claims,
//! and an HMAC-SHA256 tag over `header.payload`. It is signed and verified here
//! by hand rather than through a JWT library, for two reasons. The algorithm is
//! not negotiable — [`verify_access_token`] accepts the literal `"HS256"` and
//! nothing else, so the `alg` confusion family of attacks has no purchase — and
//! the tag is compared in constant time, so verification cannot be turned into
//! an oracle.
//!
//! The refresh token carries no structure at all: it is 256 bits of randomness,
//! base64url encoded, and the server keeps only its SHA-256 digest. It is
//! therefore unguessable, revocable by deleting one record, and valueless to
//! anyone who reads the store.
//!
//! The claims include the subject's system-administrator flag, so a request can
//! be authorized without reading the account store at all: verification is a
//! signature check and a parse, with no file I/O on the authenticated path. The
//! flag is inside the signed payload, so it cannot be widened without the
//! secret; the price is that a demoted account keeps its old authority until
//! the token it already holds expires, which is why access tokens are short.
//!
//! Every function here takes the instant it should reason about as a parameter
//! rather than reading the clock, so expiry is tested exactly at the boundary
//! instead of by sleeping.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The only JWS algorithm this server signs or accepts.
const ALGORITHM: &str = "HS256";

/// The number of random bytes behind a refresh token.
pub const REFRESH_TOKEN_BYTES: usize = 32;

/// The number of random bytes behind a [`random_id`].
const ID_BYTES: usize = 16;

/// The claims an access token carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    /// `sub` — the account the token was issued to.
    pub subject: String,
    /// `iat` — when the token was issued, in Unix seconds.
    pub issued_at: u64,
    /// `exp` — when the token stops being valid, in Unix seconds.
    pub expires_at: u64,
    /// `jti` — a unique id, so one access token can be named apart from another.
    pub token_id: String,
    /// `sys` — whether the account may act outside the projects granted to it.
    ///
    /// It travels in the signed claims so that a request can be authorized
    /// without reading the store. The consequence, and the reason the access
    /// token is short-lived, is that an account demoted after a token was minted
    /// keeps its old authority until that token expires.
    pub system_admin: bool,
}

/// The claims as the wire format spells them.
///
/// `sys` is required rather than defaulted: a token that does not say whether
/// its subject is a system administrator was not minted by a server that knew
/// about them, so it is refused rather than guessed at.
#[derive(Debug, Serialize, Deserialize)]
struct RawClaims {
    sub: String,
    iat: u64,
    exp: u64,
    jti: String,
    sys: bool,
}

/// The JOSE header. Unknown members are ignored; `alg` is not.
#[derive(Debug, Deserialize)]
struct RawHeader {
    alg: String,
}

/// Why an access token was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    /// The token is not three base64url segments carrying the expected claims.
    Malformed,
    /// The tag does not match the signing secret.
    BadSignature,
    /// The token was well-formed and correctly signed, but its `exp` has passed.
    Expired,
}

/// Mints a short-lived access token for `subject`, valid from `now`.
///
/// `now` is Unix seconds. The token expires at `now + ttl`; a token is accepted
/// strictly before that instant. `system_admin` is recorded in the signed
/// claims, so the authority it grants cannot be widened without the secret.
#[must_use]
pub fn mint_access_token(
    secret: &[u8],
    subject: &str,
    system_admin: bool,
    ttl: Duration,
    now: u64,
) -> String {
    let claims = RawClaims {
        sub: subject.to_owned(),
        iat: now,
        exp: now.saturating_add(ttl.as_secs()),
        jti: random_id(),
        sys: system_admin,
    };
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).expect("a token's claims always serialize"));
    let signing_input = format!("{header}.{payload}");
    let signature = sign(secret, signing_input.as_bytes());
    format!("{signing_input}.{signature}")
}

/// Verifies an access token and returns its claims, as of `now` (Unix seconds).
///
/// # Errors
///
/// [`TokenError::Malformed`] when the token is not a three-segment HS256 JWT
/// carrying the expected claims, [`TokenError::BadSignature`] when the tag does
/// not match, and [`TokenError::Expired`] when `exp` has passed.
pub fn verify_access_token(secret: &[u8], token: &str, now: u64) -> Result<Claims, TokenError> {
    let mut segments = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return Err(TokenError::Malformed);
    };

    // The `alg` is read from data that is not yet authenticated, so it is only
    // ever compared against the one value this server uses. Nothing from the
    // claims is trusted until the tag over `header.payload` has been checked,
    // which keeps the parser away from unauthenticated input.
    let header_json = URL_SAFE_NO_PAD
        .decode(header)
        .map_err(|_| TokenError::Malformed)?;
    let parsed_header: RawHeader =
        serde_json::from_slice(&header_json).map_err(|_| TokenError::Malformed)?;
    if parsed_header.alg != ALGORITHM {
        return Err(TokenError::Malformed);
    }

    let expected = sign(secret, format!("{header}.{payload}").as_bytes());
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err(TokenError::BadSignature);
    }

    let payload_json = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| TokenError::Malformed)?;
    let claims: RawClaims =
        serde_json::from_slice(&payload_json).map_err(|_| TokenError::Malformed)?;
    if claims.exp <= now {
        return Err(TokenError::Expired);
    }

    Ok(Claims {
        subject: claims.sub,
        issued_at: claims.iat,
        expires_at: claims.exp,
        token_id: claims.jti,
        system_admin: claims.sys,
    })
}

/// Mints an opaque refresh token: 256 bits of randomness, base64url encoded.
#[must_use]
pub fn mint_refresh_token() -> String {
    random_b64::<REFRESH_TOKEN_BYTES>()
}

/// The at-rest form of a refresh token: the lower-case hex SHA-256 digest.
///
/// The store keeps this, never the token, so a copy of the data directory gives
/// an attacker nothing to present.
#[must_use]
pub fn hash_refresh_token(token: &str) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(token.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

/// Mints a short random identifier — a token id, or an account id.
#[must_use]
pub fn random_id() -> String {
    random_b64::<ID_BYTES>()
}

fn random_b64<const BYTES: usize>() -> String {
    let mut bytes = [0_u8; BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The base64url HMAC-SHA256 tag over `message`.
fn sign(secret: &[u8], message: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts a key of any length");
    mac.update(message);
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// Compares two byte strings without returning early on the first difference.
///
/// Signature comparison must not reveal where two tags diverge, so the loop
/// folds every byte in and tests the accumulator once at the end. The lengths
/// are compared first, which is safe here because both sides are always the
/// 43-character base64url of a 32-byte tag.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"an-integration-test-secret-of-32-plus!";
    const OTHER_SECRET: &[u8] = b"a-different-signing-secret-32-plus!!!";
    const ISSUED: u64 = 1_700_000_000;

    fn segments(token: &str) -> Vec<&str> {
        token.split('.').collect()
    }

    /// Signs an arbitrary header and payload with the real secret, so a test can
    /// present a token whose only flaw is what it claims about itself.
    fn forge(header_json: &[u8], payload_json: &[u8]) -> String {
        let header = URL_SAFE_NO_PAD.encode(header_json);
        let payload = URL_SAFE_NO_PAD.encode(payload_json);
        let signing_input = format!("{header}.{payload}");
        format!("{signing_input}.{}", sign(SECRET, signing_input.as_bytes()))
    }

    #[test]
    fn an_access_token_round_trips() {
        let token = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        let claims = verify_access_token(SECRET, &token, ISSUED).expect("token verifies");
        assert_eq!(claims.subject, "user-1");
        assert_eq!(claims.issued_at, ISSUED);
        assert_eq!(claims.expires_at, ISSUED + 900);
        assert!(!claims.token_id.is_empty());
        assert!(!claims.system_admin, "an ordinary account is not an admin");
        assert_eq!(segments(&token).len(), 3);
    }

    #[test]
    fn an_access_token_carries_the_system_administrator_flag() {
        let token = mint_access_token(SECRET, "user-1", true, Duration::from_secs(900), ISSUED);
        let claims = verify_access_token(SECRET, &token, ISSUED).expect("token verifies");
        assert!(claims.system_admin);
    }

    #[test]
    fn two_tokens_for_one_subject_differ() {
        let first = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        let second = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        assert_ne!(first, second, "each token carries its own jti");
        let first_claims = verify_access_token(SECRET, &first, ISSUED).expect("first");
        let second_claims = verify_access_token(SECRET, &second, ISSUED).expect("second");
        assert_ne!(first_claims.token_id, second_claims.token_id);
    }

    #[test]
    fn an_access_token_is_valid_until_the_moment_it_expires() {
        let token = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        assert!(verify_access_token(SECRET, &token, ISSUED + 899).is_ok());
        assert_eq!(
            verify_access_token(SECRET, &token, ISSUED + 900),
            Err(TokenError::Expired),
            "exp is exclusive"
        );
        assert_eq!(
            verify_access_token(SECRET, &token, ISSUED + 10_000),
            Err(TokenError::Expired)
        );
    }

    #[test]
    fn another_secret_cannot_verify_the_token() {
        let token = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        assert_eq!(
            verify_access_token(OTHER_SECRET, &token, ISSUED),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn a_tampered_payload_fails_its_signature() {
        let token = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        let parts = segments(&token);
        // The same claims, with the system-administrator flag flipped. It is not
        // the subject a forger would reach for, but the authority.
        let payload = URL_SAFE_NO_PAD
            .encode(br#"{"sub":"user-1","iat":1700000000,"exp":1700000900,"jti":"x","sys":true}"#);
        let forged = format!("{}.{payload}.{}", parts[0], parts[2]);
        assert_eq!(
            verify_access_token(SECRET, &forged, ISSUED),
            Err(TokenError::BadSignature),
            "raising your own authority invalidates the tag"
        );
    }

    #[test]
    fn a_tampered_signature_fails() {
        let token = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        let parts = segments(&token);
        let forged = format!("{}.{}.{}", parts[0], parts[1], "A".repeat(43));
        assert_eq!(
            verify_access_token(SECRET, &forged, ISSUED),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn a_token_that_is_not_three_segments_is_malformed() {
        for token in ["", "a", "a.b", "a.b.c.d", "a.b.c."] {
            assert_eq!(
                verify_access_token(SECRET, token, ISSUED),
                Err(TokenError::Malformed),
                "{token:?}"
            );
        }
    }

    #[test]
    fn a_segment_that_is_not_base64url_is_malformed() {
        let token = mint_access_token(SECRET, "user-1", false, Duration::from_secs(900), ISSUED);
        let parts = segments(&token);
        let forged = format!("not*base64*url.{}.{}", parts[1], parts[2]);
        assert_eq!(
            verify_access_token(SECRET, &forged, ISSUED),
            Err(TokenError::Malformed)
        );
    }

    #[test]
    fn a_correctly_signed_token_with_a_foreign_algorithm_is_refused() {
        let token = forge(
            br#"{"alg":"none","typ":"JWT"}"#,
            br#"{"sub":"root","iat":0,"exp":4294967295,"jti":"x","sys":true}"#,
        );
        assert_eq!(
            verify_access_token(SECRET, &token, ISSUED),
            Err(TokenError::Malformed),
            "only HS256 is accepted, however well signed the rest is"
        );
    }

    #[test]
    fn a_correctly_signed_token_without_the_system_administrator_claim_is_malformed() {
        let token = forge(
            br#"{"alg":"HS256","typ":"JWT"}"#,
            br#"{"sub":"root","iat":0,"exp":4294967295,"jti":"x"}"#,
        );
        assert_eq!(
            verify_access_token(SECRET, &token, ISSUED),
            Err(TokenError::Malformed),
            "a token that does not say whether its subject is an admin is not guessed at"
        );
    }

    #[test]
    fn a_correctly_signed_token_with_unexpected_claims_is_malformed() {
        let token = forge(br#"{"alg":"HS256","typ":"JWT"}"#, br#"{"sub":"root"}"#);
        assert_eq!(
            verify_access_token(SECRET, &token, ISSUED),
            Err(TokenError::Malformed)
        );
    }

    #[test]
    fn a_correctly_signed_token_with_unparseable_claims_is_malformed() {
        let token = forge(br#"{"alg":"HS256","typ":"JWT"}"#, b"not json");
        assert_eq!(
            verify_access_token(SECRET, &token, ISSUED),
            Err(TokenError::Malformed)
        );
    }

    #[test]
    fn refresh_tokens_are_unique_and_hash_deterministically() {
        let first = mint_refresh_token();
        let second = mint_refresh_token();
        assert_ne!(first, second);
        assert_eq!(first.len(), 43, "32 bytes of base64url, unpadded");
        assert!(!first.contains('='), "base64url without padding");

        let digest = hash_refresh_token(&first);
        assert_eq!(digest, hash_refresh_token(&first));
        assert_ne!(digest, hash_refresh_token(&second));
        assert_eq!(digest.len(), 64, "sha-256 as lower-case hex");
        assert!(
            digest
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
        assert!(
            !digest.contains(&first),
            "the digest must not contain the token it covers"
        );
    }

    #[test]
    fn identifiers_are_unique() {
        assert_ne!(random_id(), random_id());
        assert_eq!(random_id().len(), 22, "16 bytes of base64url, unpadded");
    }

    #[test]
    fn constant_time_comparison_agrees_with_equality() {
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
    }
}
