//! Payload validation for create and update requests.
//!
//! The legacy handlers accepted any JSON object and persisted whatever fields it
//! happened to carry. Creating a document now has to agree with the typed models
//! in [`crate::models`]: the body must be an object, every top-level key must be
//! one the resource actually defines, and every supplied field — scalar or
//! nested collection — must deserialise into the field its model declares.
//! Storage itself stays permissive so already-stored documents are never
//! rewritten or rejected.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::models::{Milestone, Project, TestCase, TestConfiguration, TestRun, TestSuite};
use crate::storage::Resource;

use super::error::DomainError;

/// Top-level field names accepted for a resource, in the legacy camelCase
/// spelling. A body carrying anything outside this set is rejected.
pub fn known_fields(resource: Resource) -> &'static [&'static str] {
    match resource {
        Resource::Projects => &["projectId", "name", "description", "testSuites", "tags"],
        Resource::Suites => &["suiteId", "name", "description", "testCases", "tags"],
        Resource::Cases => &[
            "testCaseId",
            "title",
            "description",
            "preconditions",
            "steps",
            "expectedResult",
            "priority",
            "severity",
            "testType",
            "exploratory",
            "attachments",
            "tags",
            "version",
            "lastModified",
        ],
        Resource::Runs => &[
            "testRunId",
            "timestamp",
            "name",
            "projects",
            "testSuites",
            "testCases",
            "results",
            "tags",
            "configurations",
            "caseVersions",
        ],
        Resource::Milestones => &[
            "milestoneId",
            "name",
            "description",
            "startDate",
            "targetDate",
            "status",
            "testSuiteIds",
            "testRunIds",
        ],
        Resource::Configurations => &["configId", "name", "browser", "os", "device", "resolution"],
    }
}

/// Validates a create/update body against the resource's typed model.
///
/// Only present fields are checked, so the partial payloads the API has always
/// accepted (`{"name": "alpha"}`) keep working unchanged.
pub fn validate_payload(resource: Resource, value: &Value) -> Result<(), DomainError> {
    let object = value
        .as_object()
        .ok_or_else(|| DomainError::invalid_request("Request body must be a JSON object"))?;

    let allowed = known_fields(resource);
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(DomainError::invalid_request(format!(
                "Unknown field `{key}`"
            )));
        }
    }

    match resource {
        Resource::Projects => check_against_model::<Project>(object),
        Resource::Suites => check_against_model::<TestSuite>(object),
        Resource::Cases => check_against_model::<TestCase>(object),
        Resource::Runs => check_against_model::<TestRun>(object),
        Resource::Milestones => check_against_model::<Milestone>(object),
        Resource::Configurations => check_against_model::<TestConfiguration>(object),
    }
}

/// Type-checks each supplied top-level field against the resource's model.
///
/// The model drives the check: a template instance is serialised and each
/// present field is inserted into a copy of it, so a field whose JSON type
/// contradicts the declared Rust type fails to deserialise. Required fields the
/// body legitimately omits stay filled by the template, which is what keeps
/// partial payloads accepted. `null` counts as absent and is skipped, matching
/// how storage already reads it.
fn check_against_model<T>(object: &serde_json::Map<String, Value>) -> Result<(), DomainError>
where
    T: DeserializeOwned + Serialize + Default,
{
    let template = serde_json::to_value(T::default()).expect("a model serialises to JSON");
    let template = template
        .as_object()
        .expect("models serialise to JSON objects");

    for (field, supplied) in object {
        if supplied.is_null() {
            continue;
        }
        let mut probe = template.clone();
        probe.insert(field.clone(), supplied.clone());
        if serde_json::from_value::<T>(Value::Object(probe)).is_err() {
            return Err(DomainError::invalid_request(format!(
                "Field `{field}` is invalid"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Milestone;
    use serde_json::json;

    fn assert_invalid(resource: Resource, payload: Value) {
        let error = validate_payload(resource, &payload).expect_err("payload must be rejected");
        assert!(
            matches!(error, DomainError::InvalidRequest { .. }),
            "{error:?} should be a 400"
        );
    }

    #[test]
    fn a_non_object_body_is_rejected() {
        assert_invalid(Resource::Projects, json!(["not", "an", "object"]));
    }

    #[test]
    fn unknown_top_level_fields_are_rejected() {
        assert_invalid(Resource::Projects, json!({ "name": "alpha", "sneaky": 1 }));
        assert_invalid(
            Resource::Cases,
            json!({ "testCaseId": "TC-1", "title": "t", "expectedResult": "e", "extra": true }),
        );
    }

    #[test]
    fn wrong_typed_scalars_are_rejected() {
        assert_invalid(
            Resource::Projects,
            json!({ "name": "checkout", "tags": "smoke" }),
        );
        assert_invalid(
            Resource::Projects,
            json!({ "name": "checkout", "description": 7 }),
        );
        assert_invalid(
            Resource::Projects,
            json!({ "name": "checkout", "tags": { "smoke": true } }),
        );
        assert_invalid(
            Resource::Runs,
            json!({ "testRunId": "R-1", "timestamp": 123 }),
        );
        assert_invalid(
            Resource::Cases,
            json!({
                "testCaseId": "TC-1",
                "title": "t",
                "expectedResult": "e",
                "exploratory": "yes"
            }),
        );
        assert_invalid(
            Resource::Cases,
            json!({
                "testCaseId": "TC-1",
                "title": "t",
                "expectedResult": "e",
                "priority": 3
            }),
        );
        assert_invalid(
            Resource::Milestones,
            json!({ "name": "v1.0", "testSuiteIds": "S-1" }),
        );
    }

    #[test]
    fn the_rejected_field_is_named_in_the_message() {
        let error = validate_payload(
            Resource::Projects,
            &json!({ "name": "checkout", "tags": "smoke" }),
        )
        .expect_err("a wrong-typed scalar must be rejected");
        assert!(
            matches!(
                &error,
                DomainError::InvalidRequest { message, .. } if message == "Field `tags` is invalid"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn correctly_typed_scalars_are_accepted() {
        assert!(validate_payload(Resource::Projects, &json!({ "name": "a", "tags": [] })).is_ok());
        assert!(
            validate_payload(
                Resource::Projects,
                &json!({ "name": "a", "tags": ["smoke", "regression"] })
            )
            .is_ok()
        );
        assert!(
            validate_payload(
                Resource::Cases,
                &json!({
                    "testCaseId": "TC-1",
                    "title": "t",
                    "expectedResult": "e",
                    "exploratory": true,
                    "priority": "High"
                })
            )
            .is_ok()
        );
    }

    #[test]
    fn null_scalars_are_treated_as_absent() {
        assert!(
            validate_payload(
                Resource::Projects,
                &json!({ "name": "alpha", "tags": null, "description": null })
            )
            .is_ok()
        );
    }

    #[test]
    fn partial_payloads_used_by_the_api_are_accepted() {
        assert!(validate_payload(Resource::Projects, &json!({ "name": "alpha" })).is_ok());
        assert!(validate_payload(Resource::Suites, &json!({ "name": "smoke" })).is_ok());
        assert!(validate_payload(Resource::Runs, &json!({ "name": "nightly" })).is_ok());
        assert!(validate_payload(Resource::Milestones, &json!({ "name": "v1.0" })).is_ok());
        assert!(validate_payload(Resource::Configurations, &json!({ "name": "chrome" })).is_ok());
    }

    #[test]
    fn nested_collections_are_validated_against_their_models() {
        let case = json!({
            "testCaseId": "TC-001",
            "title": "Upload evidence",
            "expectedResult": "Attachment stored",
            "steps": ["Open the upload form", { "action": "Pick a file" }],
            "attachments": [{
                "filename": "1-shot.png",
                "originalName": "shot.png",
                "mimeType": "image/png",
                "size": 2048
            }]
        });
        assert!(validate_payload(Resource::Cases, &case).is_ok());

        let suite = json!({
            "suiteId": "S-001",
            "name": "smoke",
            "testCases": [{
                "testCaseId": "TC-002",
                "title": "Sign in",
                "expectedResult": "Dashboard"
            }]
        });
        assert!(validate_payload(Resource::Suites, &suite).is_ok());

        let project = json!({ "projectId": "P-001", "name": "checkout", "testSuites": [] });
        assert!(validate_payload(Resource::Projects, &project).is_ok());

        let configuration = json!({ "configId": "C-1", "name": "Chrome", "browser": "Chrome" });
        let run = json!({
            "testRunId": "R-001",
            "timestamp": "1",
            "testSuites": [],
            "results": [{ "testCaseId": "TC-001", "status": "Passed", "timestamp": "1" }],
            "configurations": [configuration]
        });
        assert!(validate_payload(Resource::Runs, &run).is_ok());
    }

    #[test]
    fn structured_steps_are_validated_for_step_attachments() {
        let case = json!({
            "testCaseId": "TC-001",
            "title": "Attach evidence to a step",
            "expectedResult": "Stored",
            "steps": ["Open the form", {
                "action": "Pick a file",
                "attachments": [{
                    "filename": "1-shot.png",
                    "originalName": "shot.png",
                    "mimeType": "image/png",
                    "size": 2048
                }]
            }]
        });
        assert!(validate_payload(Resource::Cases, &case).is_ok());

        assert_invalid(
            Resource::Cases,
            json!({
                "testCaseId": "TC-001",
                "title": "t",
                "expectedResult": "e",
                "steps": [{ "action": "Pick a file", "attachments": [{ "filename": "1.png" }] }]
            }),
        );
        assert_invalid(
            Resource::Cases,
            json!({
                "testCaseId": "TC-001",
                "title": "t",
                "expectedResult": "e",
                "steps": [{ "action": "Pick a file", "attachments": [{ "sneaky": true }] }]
            }),
        );
    }

    #[test]
    fn malformed_nested_collections_are_rejected() {
        assert_invalid(
            Resource::Projects,
            json!({ "name": "alpha", "testSuites": "not a list" }),
        );
        assert_invalid(
            Resource::Projects,
            json!({ "name": "alpha", "testSuites": [{ "name": "no id" }] }),
        );
        assert_invalid(
            Resource::Runs,
            json!({ "testRunId": "R-1", "timestamp": "1", "results": [{ "status": "Passed" }] }),
        );
        assert_invalid(
            Resource::Cases,
            json!({
                "testCaseId": "TC-1",
                "title": "t",
                "expectedResult": "e",
                "steps": [{ "unexpected": true }]
            }),
        );
    }

    #[test]
    fn null_nested_fields_are_treated_as_absent() {
        assert!(
            validate_payload(
                Resource::Projects,
                &json!({ "name": "alpha", "testSuites": null })
            )
            .is_ok()
        );
    }

    #[test]
    fn known_fields_cover_every_field_the_models_serialise() {
        let instances: [(Resource, Value); 6] = [
            (
                Resource::Projects,
                serde_json::to_value(Project {
                    project_id: "P-1".to_owned(),
                    name: "project".to_owned(),
                    description: None,
                    test_suites: Vec::new(),
                    tags: None,
                })
                .expect("serialisable project"),
            ),
            (
                Resource::Suites,
                serde_json::to_value(TestSuite {
                    suite_id: "S-1".to_owned(),
                    name: "suite".to_owned(),
                    description: None,
                    test_cases: Vec::new(),
                    tags: None,
                })
                .expect("serialisable suite"),
            ),
            (
                Resource::Cases,
                serde_json::to_value(TestCase {
                    test_case_id: "TC-1".to_owned(),
                    title: "case".to_owned(),
                    description: None,
                    preconditions: None,
                    steps: None,
                    expected_result: "expected".to_owned(),
                    priority: None,
                    severity: None,
                    test_type: None,
                    exploratory: None,
                    attachments: None,
                    tags: None,
                    version: Some(1),
                    last_modified: Some("1970-01-01T00:00:00Z".to_owned()),
                })
                .expect("serialisable case"),
            ),
            (
                Resource::Runs,
                serde_json::to_value(crate::models::TestRun {
                    test_run_id: "R-1".to_owned(),
                    timestamp: "1".to_owned(),
                    name: None,
                    projects: None,
                    test_suites: None,
                    test_cases: None,
                    results: None,
                    tags: None,
                    configurations: None,
                    case_versions: Some(std::collections::HashMap::from([("TC-1".to_owned(), 1)])),
                })
                .expect("serialisable run"),
            ),
            (
                Resource::Milestones,
                serde_json::to_value(Milestone {
                    milestone_id: "M-1".to_owned(),
                    name: "milestone".to_owned(),
                    description: None,
                    start_date: None,
                    target_date: None,
                    status: None,
                    test_suite_ids: None,
                    test_run_ids: None,
                })
                .expect("serialisable milestone"),
            ),
            (
                Resource::Configurations,
                serde_json::to_value(TestConfiguration {
                    config_id: "C-1".to_owned(),
                    name: "configuration".to_owned(),
                    browser: None,
                    os: None,
                    device: None,
                    resolution: None,
                })
                .expect("serialisable configuration"),
            ),
        ];

        for (resource, document) in instances {
            let allowed = known_fields(resource);
            let keys = document.as_object().expect("models serialise to objects");
            assert!(!keys.is_empty());
            for key in keys.keys() {
                assert!(
                    allowed.contains(&key.as_str()),
                    "`{}` is serialised by the model but not listed in known_fields",
                    key
                );
            }
        }
    }
}
