//! Duplication rules.
//!
//! Every `/…/{id}/duplicate` endpoint follows the same shape: load the source,
//! override a handful of fields from the request body, then store the copy under
//! a new identifier. What differs per resource is captured by [`DuplicateSpec`].

use serde_json::{Value, json};

use crate::storage::{Resource, unique_suffix};

use super::current_timestamp_string;

/// Everything that distinguishes one duplication endpoint from another.
#[derive(Debug, Clone, Copy)]
pub struct DuplicateSpec {
    /// Resource the copies are stored in.
    pub resource: Resource,
    /// Field holding the document's own identifier.
    pub id_field: &'static str,
    /// Message for a source document that does not exist.
    pub not_found_message: &'static str,
    /// Optional `(body key, document field)` rename applied to the copy.
    pub rename: Option<(&'static str, &'static str)>,
    /// Whether run-specific state (timestamp, results) is reset on the copy.
    pub reset_run_state: bool,
    /// Message returned when the copy is created.
    pub duplicated_message: &'static str,
    /// Message returned when a document already occupies the new identifier.
    pub already_exists_message: &'static str,
}

/// Duplication of a project.
pub const PROJECT: DuplicateSpec = DuplicateSpec {
    resource: Resource::Projects,
    id_field: "projectId",
    not_found_message: "Project not found",
    rename: Some(("newName", "name")),
    reset_run_state: false,
    duplicated_message: "Project duplicated",
    already_exists_message: "Project already exists",
};

/// Duplication of a test suite.
pub const SUITE: DuplicateSpec = DuplicateSpec {
    resource: Resource::Suites,
    id_field: "suiteId",
    not_found_message: "Test suite not found",
    rename: Some(("newName", "name")),
    reset_run_state: false,
    duplicated_message: "Test suite duplicated",
    already_exists_message: "Test suite already exists",
};

/// Duplication of a test case.
pub const CASE: DuplicateSpec = DuplicateSpec {
    resource: Resource::Cases,
    id_field: "testCaseId",
    not_found_message: "Test case not found",
    rename: Some(("newTitle", "title")),
    reset_run_state: false,
    duplicated_message: "Test case duplicated",
    already_exists_message: "Test case already exists",
};

/// Duplication of a test run, which starts life without results.
pub const RUN: DuplicateSpec = DuplicateSpec {
    resource: Resource::Runs,
    id_field: "testRunId",
    not_found_message: "Test run not found",
    rename: None,
    reset_run_state: true,
    duplicated_message: "Test run duplicated",
    already_exists_message: "Test run already exists",
};

/// Duplication of a milestone, which keeps the runs it references and therefore
/// derives the same progress as the source.
pub const MILESTONE: DuplicateSpec = DuplicateSpec {
    resource: Resource::Milestones,
    id_field: "milestoneId",
    not_found_message: "Milestone not found",
    rename: None,
    reset_run_state: false,
    duplicated_message: "Milestone duplicated",
    already_exists_message: "Milestone already exists",
};

/// Applies the request-body overrides to a duplicated document and returns the
/// identifier the copy should be stored under.
///
/// A body without `newId` derives `{source base}-copy-{suffix}`. File-backed
/// resources address their documents through an identifier ending in `.json`,
/// so the derived identifier carries one; test cases keep their bare
/// identifier. An explicit `newId` is used verbatim.
pub fn apply_overrides(
    spec: &DuplicateSpec,
    source_id: &str,
    body: &Value,
    document: &mut Value,
) -> String {
    let new_id = match body.get("newId").and_then(Value::as_str) {
        Some(new_id) => new_id.to_owned(),
        None => {
            let derived = format!(
                "{}-copy-{}",
                source_id.trim_end_matches(".json"),
                unique_suffix()
            );
            if spec.resource.id_requires_json_suffix() {
                format!("{derived}.json")
            } else {
                derived
            }
        }
    };
    set_field(document, spec.id_field, json!(new_id));

    if let Some((body_key, document_field)) = spec.rename
        && let Some(new_name) = body.get(body_key).and_then(Value::as_str)
    {
        set_field(document, document_field, json!(new_name));
    }

    if spec.reset_run_state {
        set_field(document, "timestamp", json!(current_timestamp_string()));
        if let Some(object) = document.as_object_mut() {
            object.remove("results");
        }
    }

    document
        .get(spec.id_field)
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}

fn set_field(document: &mut Value, field: &str, value: Value) {
    if let Some(object) = document.as_object_mut() {
        object.insert(field.to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_new_id_is_used_verbatim() {
        let mut document = json!({ "projectId": "P-1", "name": "checkout" });
        let id = apply_overrides(
            &PROJECT,
            "checkout.json",
            &json!({ "newId": "P-2" }),
            &mut document,
        );

        assert_eq!(id, "P-2");
        assert_eq!(document["projectId"], "P-2");
    }

    #[test]
    fn a_derived_id_strips_the_extension_and_appends_a_suffix() {
        let mut document = json!({ "projectId": "P-1", "name": "checkout" });
        let id = apply_overrides(&PROJECT, "checkout.json", &json!({}), &mut document);

        assert!(
            id.starts_with("checkout-copy-"),
            "unexpected derived id {id}"
        );
        assert!(
            id.ends_with(".json"),
            "derived id must be addressable: {id}"
        );
        assert_eq!(document["projectId"], id);
    }

    #[test]
    fn derived_ids_carry_the_suffix_the_storage_layer_requires() {
        for spec in [PROJECT, SUITE, RUN, MILESTONE] {
            let mut document = json!({});
            let id = apply_overrides(&spec, "source.json", &json!({}), &mut document);

            assert!(id.starts_with("source-copy-"), "{spec:?} derived {id}");
            assert!(id.ends_with(".json"), "{spec:?} derived {id}");
            assert_eq!(document[spec.id_field], id);
        }

        let mut case_document = json!({ "testCaseId": "TC-1" });
        let id = apply_overrides(&CASE, "TC-1", &json!({}), &mut case_document);

        assert!(id.starts_with("TC-1-copy-"), "unexpected derived id {id}");
        assert!(!id.ends_with(".json"), "test cases keep bare ids: {id}");
        assert_eq!(case_document["testCaseId"], id);
    }

    #[test]
    fn a_milestone_copy_keeps_every_field_it_references() {
        let mut document = json!({
            "milestoneId": "M-1.json",
            "name": "Sprint 42",
            "startDate": "2026-09-01",
            "targetDate": "2026-09-15",
            "status": "Open",
            "testSuiteIds": ["S-1.json"],
            "testRunIds": ["RUN-1.json"]
        });
        let id = apply_overrides(&MILESTONE, "M-1.json", &json!({}), &mut document);

        assert!(id.starts_with("M-1-copy-"), "unexpected derived id {id}");
        assert_eq!(document["milestoneId"], id);
        assert_eq!(document["name"], "Sprint 42");
        assert_eq!(document["startDate"], "2026-09-01");
        assert_eq!(document["targetDate"], "2026-09-15");
        assert_eq!(document["status"], "Open");
        assert_eq!(document["testSuiteIds"], json!(["S-1.json"]));
        assert_eq!(document["testRunIds"], json!(["RUN-1.json"]));
    }

    #[test]
    fn renames_are_applied_only_when_the_body_supplies_them() {
        let mut document = json!({ "suiteId": "S-1", "name": "smoke" });
        apply_overrides(
            &SUITE,
            "smoke.json",
            &json!({ "newName": "regression" }),
            &mut document,
        );
        assert_eq!(document["name"], "regression");

        let mut untouched = json!({ "suiteId": "S-1", "name": "smoke" });
        apply_overrides(&SUITE, "smoke.json", &json!({}), &mut untouched);
        assert_eq!(untouched["name"], "smoke");
    }

    #[test]
    fn test_cases_are_renamed_through_new_title() {
        let mut document = json!({ "testCaseId": "TC-1", "title": "Old" });
        apply_overrides(&CASE, "TC-1", &json!({ "newTitle": "New" }), &mut document);

        assert_eq!(document["title"], "New");
        assert!(
            document.get("name").is_none(),
            "cases must not gain a name field"
        );
    }

    #[test]
    fn run_copies_lose_their_results_and_gain_a_fresh_timestamp() {
        let mut document = json!({
            "testRunId": "R-1",
            "timestamp": "1",
            "results": [{ "testCaseId": "TC-1", "status": "Passed", "timestamp": "1" }]
        });
        let id = apply_overrides(
            &RUN,
            "nightly.json",
            &json!({ "newId": "R-2" }),
            &mut document,
        );

        assert_eq!(id, "R-2");
        assert!(document.get("results").is_none());
        assert_ne!(document["timestamp"], json!("1"));
    }

    #[test]
    fn other_resources_keep_their_results() {
        let mut document = json!({
            "suiteId": "S-1",
            "name": "smoke",
            "results": ["kept"]
        });
        apply_overrides(&SUITE, "smoke.json", &json!({}), &mut document);
        assert_eq!(document["results"], json!(["kept"]));
    }

    #[test]
    fn a_document_that_is_not_an_object_reports_unknown() {
        let mut document = json!(["not", "an", "object"]);
        let id = apply_overrides(&PROJECT, "checkout.json", &json!({}), &mut document);
        assert_eq!(id, "unknown");
    }
}
