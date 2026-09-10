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

/// Applies the request-body overrides to a duplicated document and returns the
/// identifier the copy should be stored under.
///
/// The derived identifier deliberately omits the `.json` suffix — that is the
/// long-standing behaviour issue #68 tracks, and changing it here would be a
/// silent breaking change.
pub fn apply_overrides(
    spec: &DuplicateSpec,
    source_id: &str,
    body: &Value,
    document: &mut Value,
) -> String {
    let new_id = match body.get("newId").and_then(Value::as_str) {
        Some(new_id) => new_id.to_owned(),
        None => format!(
            "{}-copy-{}",
            source_id.trim_end_matches(".json"),
            unique_suffix()
        ),
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
        assert_eq!(document["projectId"], id);
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
