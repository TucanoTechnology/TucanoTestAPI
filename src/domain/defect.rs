//! Rules for linking a run result to a defect in an external tracker.
//!
//! A link records where a defect lives; the tracker it claims to live in is the
//! only thing the API can check without asking that tracker, so the URL is
//! matched against the shape the named tracker uses. The URL is parsed by hand —
//! the crate carries no URL dependency — and only the parts the patterns look at
//! are extracted: the host and the path segments, with the query string and
//! fragment dropped because a tracker link is identified by where it points, not
//! by the browsing state appended to it.

use serde_json::Value;

use crate::models::{DefectLink, DefectLinkRequest};

use super::error::DomainError;

/// Tracker types a defect link may name.
pub const TRACKER_TYPES: [&str; 4] = ["jira", "github", "gitlab", "custom"];

const ATLASSIAN_SUFFIX: &str = ".atlassian.net";
const GITHUB_HOST: &str = "github.com";
const GITLAB_HOST: &str = "gitlab.com";

/// Reads a link request body, rejecting anything the API will not store.
///
/// The typed model drives the structural checks: a missing or wrong-typed field
/// and a field the model does not define are all rejected. What the model
/// cannot express — an empty identifier, a tracker outside the supported set,
/// and a URL the tracker would not recognise — is checked here.
pub fn parse_request(body: &Value) -> Result<DefectLinkRequest, DomainError> {
    let request: DefectLinkRequest = serde_json::from_value(body.clone()).map_err(|error| {
        DomainError::invalid_request(format!("Defect link request is invalid: {error}"))
    })?;

    for (field, value) in [
        ("defectId", &request.defect_id),
        ("defectUrl", &request.defect_url),
        ("trackerType", &request.tracker_type),
    ] {
        if value.is_empty() {
            return Err(DomainError::invalid_request(format!(
                "Required field {field} is missing"
            )));
        }
    }

    if !TRACKER_TYPES.contains(&request.tracker_type.as_str()) {
        return Err(DomainError::invalid_request(format!(
            "Unsupported trackerType `{}`; expected one of {}",
            request.tracker_type,
            TRACKER_TYPES.join(", ")
        )));
    }
    validate_url(&request.tracker_type, &request.defect_url)?;

    Ok(request)
}

/// Builds the stored link from a validated request.
///
/// The client supplies where the defect lives; the API supplies the link's own
/// identity and the moment it was made, so a client never has to invent either.
pub fn new_link(request: DefectLinkRequest, link_id: String, linked_at: String) -> DefectLink {
    DefectLink {
        link_id,
        defect_id: request.defect_id,
        defect_url: request.defect_url,
        tracker_type: request.tracker_type,
        title: request.title,
        status: request.status,
        linked_at,
    }
}

/// Checks that `url` is the shape `tracker_type` uses for defect links.
///
/// `custom` accepts any well-formed `https://` URL: a tracker the API does not
/// know by name can still be linked, it just cannot be checked.
pub fn validate_url(tracker_type: &str, url: &str) -> Result<(), DomainError> {
    let parsed = split_url(url)?;
    let accepted = match tracker_type {
        "jira" => is_jira(&parsed),
        "github" => is_github(&parsed),
        "gitlab" => is_gitlab(&parsed),
        "custom" => true,
        _ => false,
    };
    if accepted {
        Ok(())
    } else {
        Err(DomainError::invalid_request(format!(
            "defectUrl is not a valid {tracker_type} issue URL"
        )))
    }
}

/// The parts of a URL the tracker patterns look at.
struct Url<'a> {
    host: &'a str,
    segments: Vec<&'a str>,
}

fn invalid_url() -> DomainError {
    DomainError::invalid_request("defectUrl must be a well-formed https:// URL")
}

fn split_url(url: &str) -> Result<Url<'_>, DomainError> {
    let rest = match url.get(..8) {
        Some(prefix) if prefix.eq_ignore_ascii_case("https://") => &url[8..],
        _ => return Err(invalid_url()),
    };
    if url.chars().any(char::is_whitespace) {
        return Err(invalid_url());
    }

    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.split(['?', '#']).next().unwrap_or_default();

    // Credentials belong in no tracker link, so a userinfo section is refused
    // rather than stripped: `https://github.com@evil.example/…` is not GitHub.
    if authority.contains('@') {
        return Err(invalid_url());
    }
    let host = match authority.rsplit_once(':') {
        Some((name, port))
            if !port.is_empty() && port.chars().all(|byte| byte.is_ascii_digit()) =>
        {
            name
        }
        _ => authority,
    };
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') {
        return Err(invalid_url());
    }

    Ok(Url {
        host,
        segments: path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect(),
    })
}

/// `https://<site>.atlassian.net/browse/<KEY>`
fn is_jira(url: &Url<'_>) -> bool {
    let Some(subdomain) = url
        .host
        .len()
        .checked_sub(ATLASSIAN_SUFFIX.len())
        .filter(|length| *length > 0)
    else {
        return false;
    };
    if !url.host[subdomain..].eq_ignore_ascii_case(ATLASSIAN_SUFFIX) {
        return false;
    }
    matches!(url.segments.as_slice(), ["browse", key] if !key.is_empty())
}

/// `https://github.com/<owner>/<repo>/issues/<number>`
fn is_github(url: &Url<'_>) -> bool {
    url.host.eq_ignore_ascii_case(GITHUB_HOST)
        && matches!(
            url.segments.as_slice(),
            [owner, repo, "issues", number]
                if !owner.is_empty() && !repo.is_empty() && !number.is_empty()
        )
}

/// `https://gitlab.com/<group>/…/<project>/-/issues/<number>`
///
/// GitLab nests a project under any number of groups, so at least two segments
/// must precede the `-` separator; requiring the separator is what keeps a group
/// named `issues` from being read as the project.
fn is_gitlab(url: &Url<'_>) -> bool {
    if !url.host.eq_ignore_ascii_case(GITLAB_HOST) {
        return false;
    }
    let segments = &url.segments;
    let Some(separator) = segments.iter().position(|segment| *segment == "-") else {
        return false;
    };
    separator >= 2
        && segments.get(separator + 1) == Some(&"issues")
        && segments
            .get(separator + 2)
            .is_some_and(|number| !number.is_empty())
        && separator + 3 == segments.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_invalid(tracker_type: &str, url: &str) {
        let error = validate_url(tracker_type, url).expect_err("url must be rejected");
        assert!(
            matches!(error, DomainError::InvalidRequest { .. }),
            "{error:?} should be a 400"
        );
    }

    #[test]
    fn a_jira_link_must_be_an_atlassian_browse_url() {
        for url in [
            "https://acme.atlassian.net/browse/BUG-42",
            "https://acme.atlassian.net/browse/PROJ-1?filter=recent",
            "https://acme.atlassian.net/browse/PROJ-1#comment-7",
        ] {
            assert!(
                validate_url("jira", url).is_ok(),
                "{url} should be accepted"
            );
        }

        for url in [
            "https://acme.atlassian.net/BUG-42",
            "https://atlassian.net/browse/BUG-42",
            "https://acme.atlassian.net/browse/",
            "https://acme.atlassian.net/browse/BUG-42/comments",
            "https://not-atlassian.example/browse/BUG-42",
            "http://acme.atlassian.net/browse/BUG-42",
        ] {
            assert_invalid("jira", url);
        }
    }

    #[test]
    fn a_github_link_must_be_an_issue_url() {
        for url in [
            "https://github.com/acme/checkout/issues/42",
            "https://github.com/acme/checkout/issues/42#issuecomment-1",
        ] {
            assert!(
                validate_url("github", url).is_ok(),
                "{url} should be accepted"
            );
        }

        for url in [
            "https://github.com/acme/issues/42",
            "https://github.com/acme/checkout/pulls/42",
            "https://github.com/acme/checkout/issues/",
            "https://github.com/issues/42",
            "https://gitlab.com/acme/checkout/issues/42",
            "https://gh.example/acme/checkout/issues/42",
        ] {
            assert_invalid("github", url);
        }
    }

    #[test]
    fn a_gitlab_link_must_be_an_issue_url() {
        for url in [
            "https://gitlab.com/acme/checkout/-/issues/42",
            "https://gitlab.com/acme/platform/checkout/-/issues/42",
        ] {
            assert!(
                validate_url("gitlab", url).is_ok(),
                "{url} should be accepted"
            );
        }

        for url in [
            "https://gitlab.com/acme/checkout/issues/42",
            "https://gitlab.com/-/issues/42",
            "https://gitlab.com/acme/checkout/-/issues/",
            "https://gitlab.com/acme/checkout/-/issues/42/notes",
            "https://github.com/acme/checkout/-/issues/42",
        ] {
            assert_invalid("gitlab", url);
        }
    }

    #[test]
    fn a_custom_link_only_has_to_be_a_well_formed_https_url() {
        for url in [
            "https://tracker.example/BUG-42",
            "https://example.com",
            "https://localhost:8080/defects/1",
            "https://tracker.example/issues?q=BUG-42",
        ] {
            assert!(
                validate_url("custom", url).is_ok(),
                "{url} should be accepted"
            );
        }

        for url in [
            "http://tracker.example/BUG-42",
            "tracker.example/BUG-42",
            "//tracker.example/BUG-42",
            "https:///BUG-42",
            "https://user@tracker.example/BUG-42",
            "https://tracker.example/BUG 42",
            "https://tracker.example./BUG-42",
        ] {
            assert_invalid("custom", url);
        }
    }

    #[test]
    fn a_tracker_document_is_matched_whatever_the_case_of_its_scheme_and_host() {
        assert!(validate_url("github", "HTTPS://GitHub.com/acme/checkout/issues/42").is_ok());
        assert!(validate_url("jira", "https://ACME.Atlassian.NET/browse/BUG-42").is_ok());
    }

    #[test]
    fn an_unknown_tracker_type_is_rejected_whatever_the_url() {
        let error = validate_url("bugzilla", "https://tracker.example/BUG-42")
            .expect_err("an unsupported tracker must be rejected");
        assert!(
            matches!(error, DomainError::InvalidRequest { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_request_needs_the_three_fields_the_client_supplies() {
        let complete = json!({
            "defectId": "BUG-42",
            "defectUrl": "https://acme.atlassian.net/browse/BUG-42",
            "trackerType": "jira",
        });
        assert!(parse_request(&complete).is_ok());

        for body in [
            json!({"defectUrl": "https://acme.atlassian.net/browse/BUG-42", "trackerType": "jira"}),
            json!({"defectId": "BUG-42", "trackerType": "jira"}),
            json!({"defectId": "BUG-42", "defectUrl": "https://acme.atlassian.net/browse/BUG-42"}),
            json!({"defectId": "", "defectUrl": "https://acme.atlassian.net/browse/BUG-42", "trackerType": "jira"}),
            json!({"defectId": "BUG-42", "defectUrl": "", "trackerType": "jira"}),
            json!({"defectId": "BUG-42", "defectUrl": "https://acme.atlassian.net/browse/BUG-42", "trackerType": ""}),
        ] {
            let error = parse_request(&body).expect_err("an incomplete request must be rejected");
            assert!(
                matches!(error, DomainError::InvalidRequest { .. }),
                "{error:?} should be a 400"
            );
        }
    }

    #[test]
    fn a_request_carrying_fields_the_api_derives_is_rejected() {
        for body in [
            json!({
                "defectId": "BUG-42",
                "defectUrl": "https://acme.atlassian.net/browse/BUG-42",
                "trackerType": "jira",
                "linkId": "L-1",
            }),
            json!({
                "defectId": "BUG-42",
                "defectUrl": "https://acme.atlassian.net/browse/BUG-42",
                "trackerType": "jira",
                "linkedAt": "1",
            }),
            json!({
                "defectId": "BUG-42",
                "defectUrl": "https://acme.atlassian.net/browse/BUG-42",
                "trackerType": "jira",
                "sneaky": true,
            }),
        ] {
            let error = parse_request(&body).expect_err("unknown fields must be rejected");
            assert!(
                matches!(error, DomainError::InvalidRequest { .. }),
                "{error:?} should be a 400"
            );
        }
    }

    #[test]
    fn a_request_carrying_a_wrong_typed_field_is_rejected() {
        let error = parse_request(&json!({
            "defectId": "BUG-42",
            "defectUrl": "https://acme.atlassian.net/browse/BUG-42",
            "trackerType": "jira",
            "title": 7,
        }))
        .expect_err("a wrong-typed field must be rejected");
        assert!(
            matches!(error, DomainError::InvalidRequest { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_url_the_named_tracker_would_not_use_is_rejected() {
        let error = parse_request(&json!({
            "defectId": "BUG-42",
            "defectUrl": "https://gitlab.com/acme/checkout/issues/42",
            "trackerType": "jira",
        }))
        .expect_err("a mismatched tracker and URL must be rejected");
        assert!(
            matches!(
                &error,
                DomainError::InvalidRequest { message, .. }
                    if message == "defectUrl is not a valid jira issue URL"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn an_unknown_tracker_name_in_a_request_is_named_in_the_message() {
        let error = parse_request(&json!({
            "defectId": "BUG-42",
            "defectUrl": "https://tracker.example/BUG-42",
            "trackerType": "bugzilla",
        }))
        .expect_err("an unsupported tracker must be rejected");
        assert!(
            matches!(
                &error,
                DomainError::InvalidRequest { message, .. }
                    if message == "Unsupported trackerType `bugzilla`; expected one of jira, github, gitlab, custom"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_new_link_carries_the_clients_fields_under_an_api_derived_identity() {
        let request = parse_request(&json!({
            "defectId": "BUG-42",
            "defectUrl": "https://github.com/acme/checkout/issues/42",
            "trackerType": "github",
            "title": "Card is declined twice",
            "status": "Open",
        }))
        .expect("a complete request is accepted");

        let link = new_link(request, "link-7".to_owned(), "1234".to_owned());
        assert_eq!(
            link,
            DefectLink {
                link_id: "link-7".to_owned(),
                defect_id: "BUG-42".to_owned(),
                defect_url: "https://github.com/acme/checkout/issues/42".to_owned(),
                tracker_type: "github".to_owned(),
                title: Some("Card is declined twice".to_owned()),
                status: Some("Open".to_owned()),
                linked_at: "1234".to_owned(),
            }
        );
    }

    #[test]
    fn a_new_link_leaves_the_optional_fields_absent_when_the_client_omits_them() {
        let request = parse_request(&json!({
            "defectId": "BUG-42",
            "defectUrl": "https://tracker.example/BUG-42",
            "trackerType": "custom",
        }))
        .expect("a minimal request is accepted");

        let link = new_link(request, "link-7".to_owned(), "1234".to_owned());
        assert_eq!(link.title, None);
        assert_eq!(link.status, None);
        assert_eq!(link.tracker_type, "custom");
    }
}
