//! Identifier derivation for create requests.
//!
//! Which field names a new document, and whether a `.json` suffix is appended,
//! varies per resource. Reproducing those rules in one place keeps storage paths
//! and domain rules in agreement.

use serde_json::Value;

use crate::storage::Resource;

use super::error::DomainError;
use super::required_string;

/// Derives the storage identifier for a create request.
///
/// Test cases are named by their own `testCaseId` and required to carry a title
/// and expected result; milestones fall back to their name for the identifier;
/// everything else is `<name>.json`.
pub fn derive_create_id(resource: Resource, value: &Value) -> Result<String, DomainError> {
    let missing = || DomainError::invalid_request("Required fields are missing");

    match resource {
        Resource::Cases => {
            let id = required_string(value, "testCaseId");
            let title = required_string(value, "title");
            let expected_result = required_string(value, "expectedResult");
            match (id, title, expected_result) {
                (Some(id), Some(_), Some(_)) => Ok(id),
                _ => Err(missing()),
            }
        }
        Resource::Milestones => {
            let name = required_string(value, "name").ok_or_else(missing)?;
            let candidate = required_string(value, "milestoneId").unwrap_or(name);
            Ok(if candidate.ends_with(".json") {
                candidate
            } else {
                format!("{candidate}.json")
            })
        }
        Resource::Projects | Resource::Suites | Resource::Runs | Resource::Configurations => {
            let name = required_string(value, "name").ok_or_else(missing)?;
            Ok(format!("{name}.json"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn id(resource: Resource, payload: Value) -> String {
        derive_create_id(resource, &payload).expect("identifier should be derivable")
    }

    #[test]
    fn named_resources_are_suffixed_with_json() {
        assert_eq!(
            id(Resource::Projects, json!({ "name": "checkout" })),
            "checkout.json"
        );
        assert_eq!(
            id(Resource::Suites, json!({ "name": "smoke" })),
            "smoke.json"
        );
        assert_eq!(
            id(Resource::Runs, json!({ "name": "nightly" })),
            "nightly.json"
        );
        assert_eq!(
            id(Resource::Configurations, json!({ "name": "chrome" })),
            "chrome.json"
        );
    }

    #[test]
    fn test_cases_are_named_by_their_own_identifier() {
        assert_eq!(
            id(
                Resource::Cases,
                json!({
                    "testCaseId": "TC-001",
                    "title": "Upload",
                    "expectedResult": "Stored"
                })
            ),
            "TC-001"
        );
    }

    #[test]
    fn test_cases_require_identifier_title_and_expected_result() {
        for payload in [
            json!({ "title": "Upload", "expectedResult": "Stored" }),
            json!({ "testCaseId": "TC-001", "expectedResult": "Stored" }),
            json!({ "testCaseId": "TC-001", "title": "Upload" }),
            json!({ "testCaseId": "", "title": "Upload", "expectedResult": "Stored" }),
        ] {
            assert!(
                derive_create_id(Resource::Cases, &payload).is_err(),
                "{payload} should be rejected"
            );
        }
    }

    #[test]
    fn milestones_default_to_the_name_and_suffix_json() {
        assert_eq!(
            id(Resource::Milestones, json!({ "name": "v1.0-RC1" })),
            "v1.0-RC1.json"
        );
        assert_eq!(
            id(
                Resource::Milestones,
                json!({ "milestoneId": "M-001", "name": "v1.0-RC1" })
            ),
            "M-001.json"
        );
        assert_eq!(
            id(
                Resource::Milestones,
                json!({ "milestoneId": "M-001.json", "name": "v1.0-RC1" })
            ),
            "M-001.json"
        );
    }

    #[test]
    fn a_missing_name_is_a_bad_request() {
        let error = derive_create_id(Resource::Projects, &json!({ "description": "no name" }))
            .expect_err("name is required");
        assert!(matches!(error, DomainError::InvalidRequest { .. }));
        assert!(derive_create_id(Resource::Milestones, &json!({ "name": "" })).is_err());
    }
}
