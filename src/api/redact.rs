//! Keeps credentials out of everything the API writes to a log.
//!
//! A request line and its headers carry credentials that must never reach a
//! log file: a bearer token in `Authorization`, a session in `Cookie`, a
//! `?token=` or `?api_key=` in the query string of a URL a client pasted into
//! a browser. The storage-security rules forbid logging secrets, and a log is
//! read by more people than a response is, so the redaction happens where the
//! log line is built rather than where a response is rendered.
//!
//! Redaction preserves shape: a redacted value is replaced, never dropped, so
//! an operator can still see which parameters and headers a request carried
//! without seeing what they held. Nothing here inspects a request body — the
//! API never logs one — and no attachment content ever passes through.

use axum::http::HeaderMap;

/// What a secret value is replaced with.
pub const REDACTED: &str = "[redacted]";

/// Header names whose value is a credential, matched case-insensitively.
const SECRET_HEADERS: [&str; 6] = [
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "proxy-authorization",
    "x-auth-token",
];

/// Query-string keys whose value is a credential, matched case-insensitively.
const SECRET_QUERY_KEYS: [&str; 8] = [
    "token",
    "api_key",
    "apikey",
    "access_token",
    "refresh_token",
    "password",
    "secret",
    "key",
];

/// Whether a header name carries a credential.
pub fn is_secret_header(name: &str) -> bool {
    SECRET_HEADERS
        .iter()
        .any(|secret| name.eq_ignore_ascii_case(secret))
}

/// Whether a query-string parameter names a credential.
pub fn is_secret_query_key(key: &str) -> bool {
    SECRET_QUERY_KEYS
        .iter()
        .any(|secret| key.eq_ignore_ascii_case(secret))
}

/// The value to write for a header or parameter called `name`.
///
/// Any other name is returned unchanged: only the names on the two lists are
/// secrets, and a value the lists do not know is the caller's to log.
pub fn value<'a>(name: &str, value: &'a str) -> &'a str {
    if is_secret_header(name) || is_secret_query_key(name) {
        REDACTED
    } else {
        value
    }
}

/// Renders a request's headers with every credential value replaced.
///
/// Only the names are reported for headers — a header line's value is of no
/// use in a log and several of them are long — so the output is the sorted,
/// comma-separated list of names, with a credential name followed by
/// [`REDACTED`] rather than its value.
pub fn headers(headers: &HeaderMap) -> String {
    let mut names: Vec<&str> = headers.keys().map(|name| name.as_str()).collect();
    names.sort_unstable();
    names
        .into_iter()
        .map(|name| {
            if is_secret_header(name) {
                format!("{name}={REDACTED}")
            } else {
                name.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Renders a query string with every credential value replaced.
///
/// Separators, ordering and the `key=` of every pair survive, so the rendering
/// still names the parameters the request carried. A pair with no `=` is a
/// bare flag and is kept verbatim; a credential key is replaced whether or not
/// its value is empty.
pub fn query(raw: &str) -> String {
    let mut rendered = String::with_capacity(raw.len());
    for (index, pair) in raw.split('&').enumerate() {
        if index > 0 {
            rendered.push('&');
        }
        match pair.split_once('=') {
            Some((key, _)) if is_secret_query_key(key) => {
                rendered.push_str(key);
                rendered.push('=');
                rendered.push_str(REDACTED);
            }
            _ => rendered.push_str(pair),
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    #[test]
    fn a_secret_query_value_is_replaced_and_the_rest_kept() {
        assert_eq!(
            query("filter=smoke&token=abc123"),
            "filter=smoke&token=[redacted]"
        );
        assert_eq!(query("api_key=secret"), "api_key=[redacted]");
        assert_eq!(
            query("access_token=a&refresh_token=b&page=2"),
            "access_token=[redacted]&refresh_token=[redacted]&page=2"
        );
    }

    #[test]
    fn a_secret_key_is_recognised_whatever_its_case() {
        assert_eq!(query("TOKEN=abc"), "TOKEN=[redacted]");
        assert_eq!(query("Api_Key=abc"), "Api_Key=[redacted]");
    }

    #[test]
    fn a_redacted_key_keeps_its_equals_sign_and_an_ordinary_pair_is_untouched() {
        assert_eq!(query("password="), "password=[redacted]");
        assert_eq!(query("filter=a=b"), "filter=a=b");
        assert_eq!(query("verbose"), "verbose");
        assert_eq!(query(""), "");
    }

    #[test]
    fn a_secret_header_value_is_redacted_and_the_name_kept() {
        let mut map = HeaderMap::new();
        map.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer super-secret"),
        );
        map.insert(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("application/json"),
        );
        assert_eq!(value("authorization", "Bearer super-secret"), REDACTED);
        assert_eq!(value("Authorization", "Bearer super-secret"), REDACTED);
        assert_eq!(value("accept", "application/json"), "application/json");
        assert_eq!(headers(&map), "accept,authorization=[redacted]");
    }

    #[test]
    fn a_credential_never_survives_a_rendering() {
        let secret = "hunter2-correct-horse";
        assert!(!query(&format!("token={secret}&page=2")).contains(secret));
        assert!(!query(&format!("key={secret}")).contains(secret));

        let mut map = HeaderMap::new();
        map.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(secret).expect("visible ASCII"),
        );
        assert!(!headers(&map).contains(secret));
    }
}
