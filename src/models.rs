use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub filename: String,
    pub original_name: String,
    pub mime_type: String,
    pub size: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepAttachment {
    pub filename: String,
    pub original_name: String,
    pub mime_type: String,
    pub size: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestStep {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<StepAttachment>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum TestCaseStep {
    Simple(String),
    Structured(TestStep),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestCase {
    pub test_case_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preconditions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<TestCaseStep>>,
    pub expected_result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exploratory: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestSuite {
    pub suite_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub test_cases: Vec<TestCase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub project_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub test_suites: Vec<TestSuite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// A reference from a run result to the defect a failure raised.
///
/// `tracker_type` names the system the defect lives in — `jira`, `github`,
/// `gitlab` or `custom` — and `defect_url` is the address a human follows to
/// reach it. `link_id` is the link's own identity, so the same defect can be
/// linked, unlinked and relinked without depending on its position in the list.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefectLink {
    pub link_id: String,
    pub defect_id: String,
    pub defect_url: String,
    pub tracker_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub linked_at: String,
}

/// The client-supplied half of a [`DefectLink`].
///
/// The API derives `link_id` and `linked_at`, so a request that carries either
/// is rejected rather than silently ignored: a client that thinks it is naming
/// the link would otherwise never learn that its identifier was thrown away.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefectLinkRequest {
    pub defect_id: String,
    pub defect_url: String,
    pub tracker_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestCaseResult {
    pub test_case_id: String,
    pub status: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defect_links: Option<Vec<DefectLink>>,
}

/// Counts of the results one import wrote, split by the status it mapped them
/// to. `passed + failed + blocked` is the import's `imported` count.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportCounts {
    pub passed: usize,
    pub failed: usize,
    pub blocked: usize,
}

/// What one import did: how many results it wrote or left alone, and how the
/// imported results split by status.
///
/// `skipped` is the testcases the import deliberately did not write — those the
/// run already recorded (`duplicates`) plus those it could not map (`errors`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
    pub duplicates: usize,
    pub summary: ImportCounts,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestRun {
    pub test_run_id: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<Project>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_suites: Option<Vec<TestSuite>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_cases: Option<Vec<TestCase>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<TestCaseResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configurations: Option<Vec<TestConfiguration>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_versions: Option<HashMap<String, u64>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestConfiguration {
    pub config_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Milestone {
    pub milestone_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_suite_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_run_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneProgress {
    pub milestone_id: String,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub untested: usize,
    pub retest: usize,
    pub pass_percentage: f64,
}

/// One entry of a case's revision history: the snapshot's own version and
/// timestamp, and the qualifying fields an update changed after it. A legacy
/// snapshot written before versioning carries no timestamp.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CaseHistoryEntry {
    pub version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    pub changed_fields: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_round_trips_and_omits_missing_upload_time() {
        let input = r#"{
            "filename": "1700000000-notes.txt",
            "originalName": "notes.txt",
            "mimeType": "text/plain",
            "size": 12
        }"#;

        let attachment: Attachment = serde_json::from_str(input).expect("valid attachment");
        assert_eq!(attachment.original_name, "notes.txt");
        assert!(attachment.uploaded_at.is_none());

        let output = serde_json::to_value(&attachment).expect("serializable attachment");
        assert_eq!(output["mimeType"], "text/plain");
        assert_eq!(output["size"], 12.0);
        assert!(output.get("uploadedAt").is_none());
    }

    #[test]
    fn test_case_preserves_attachment_metadata() {
        let input = r#"{
            "testCaseId": "TC-001",
            "title": "Upload evidence",
            "expectedResult": "Attachment stored",
            "attachments": [{
                "filename": "1700000000-shot.png",
                "originalName": "shot.png",
                "mimeType": "image/png",
                "size": 2048,
                "uploadedAt": "2026-09-02T00:00:00Z"
            }]
        }"#;

        let case: TestCase = serde_json::from_str(input).expect("valid test case");
        let attachments = case.attachments.as_ref().expect("attachments present");
        assert_eq!(attachments.len(), 1);
        assert_eq!(
            attachments[0].uploaded_at.as_deref(),
            Some("2026-09-02T00:00:00Z")
        );

        let output = serde_json::to_value(&case).expect("serializable test case");
        assert_eq!(output["attachments"][0]["originalName"], "shot.png");
    }

    #[test]
    fn test_case_supports_structured_steps_preconditions_severity_and_test_type() {
        let input = r#"{
            "testCaseId": "TC-002",
            "title": "Rich Test Case",
            "preconditions": "User has active account",
            "priority": "High",
            "severity": "Critical",
            "testType": "Functional",
            "expectedResult": "Order confirmed",
            "steps": [
                "Navigate to /checkout",
                {
                    "action": "Click Pay Now",
                    "expectedResult": "Payment processed"
                }
            ]
        }"#;

        let case: TestCase = serde_json::from_str(input).expect("valid rich test case");
        assert_eq!(
            case.preconditions.as_deref(),
            Some("User has active account")
        );
        assert_eq!(case.severity.as_deref(), Some("Critical"));
        assert_eq!(case.test_type.as_deref(), Some("Functional"));

        let steps = case.steps.as_ref().expect("steps present");
        assert_eq!(steps.len(), 2);
        assert_eq!(
            steps[0],
            TestCaseStep::Simple("Navigate to /checkout".to_string())
        );
        assert_eq!(
            steps[1],
            TestCaseStep::Structured(TestStep {
                action: "Click Pay Now".to_string(),
                expected_result: Some("Payment processed".to_string()),
                attachments: None,
            })
        );

        let output = serde_json::to_value(&case).expect("serializable case");
        assert_eq!(output["preconditions"], "User has active account");
        assert_eq!(output["severity"], "Critical");
        assert_eq!(output["testType"], "Functional");
        assert_eq!(output["steps"][1]["action"], "Click Pay Now");
    }

    #[test]
    fn test_case_carries_version_and_last_modified() {
        let input = r#"{
            "testCaseId": "TC-001",
            "title": "Login",
            "expectedResult": "Authenticated",
            "version": 3,
            "lastModified": "2023-11-14T22:13:20Z"
        }"#;

        let case: TestCase = serde_json::from_str(input).expect("valid versioned test case");
        assert_eq!(case.version, Some(3));
        assert_eq!(case.last_modified.as_deref(), Some("2023-11-14T22:13:20Z"));

        let output = serde_json::to_value(&case).expect("serializable case");
        assert_eq!(output["version"], 3);
        assert_eq!(output["lastModified"], "2023-11-14T22:13:20Z");
    }

    #[test]
    fn test_case_without_version_fields_still_round_trips() {
        let input = r#"{"testCaseId":"TC-001","title":"Legacy","expectedResult":"Pass"}"#;

        let case: TestCase = serde_json::from_str(input).expect("legacy test case");
        assert!(case.version.is_none());
        assert!(case.last_modified.is_none());

        let output = serde_json::to_value(&case).expect("serializable case");
        assert!(
            output.get("version").is_none(),
            "an unversioned case omits `version` from the wire"
        );
        assert!(
            output.get("lastModified").is_none(),
            "an unversioned case omits `lastModified` from the wire"
        );
    }

    #[test]
    fn structured_steps_carry_step_attachment_metadata() {
        let input = r#"{
            "testCaseId": "TC-001",
            "title": "Login",
            "expectedResult": "User is authenticated",
            "steps": [
                "Navigate to /login",
                {
                    "action": "Click Submit",
                    "expectedResult": "Dashboard shown",
                    "attachments": [{
                        "filename": "1726000000000000-screenshot.png",
                        "originalName": "screenshot.png",
                        "mimeType": "image/png",
                        "size": 2048
                    }]
                }
            ]
        }"#;

        let case: TestCase = serde_json::from_str(input).expect("valid case with step attachment");
        let steps = case.steps.as_ref().expect("steps present");
        let attachments = match &steps[1] {
            TestCaseStep::Structured(step) => {
                step.attachments.as_ref().expect("step attachments present")
            }
            TestCaseStep::Simple(_) => panic!("second step must be structured"),
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].original_name, "screenshot.png");
        assert_eq!(attachments[0].mime_type, "image/png");

        let output = serde_json::to_value(&case).expect("serializable case");
        assert_eq!(
            output["steps"][1]["attachments"][0]["filename"],
            "1726000000000000-screenshot.png"
        );
        assert!(
            output["steps"][0].as_str() == Some("Navigate to /login"),
            "simple steps stay untouched"
        );
    }

    #[test]
    fn structured_steps_omit_absent_step_attachments() {
        let step = TestStep {
            action: "Click Submit".to_string(),
            expected_result: None,
            attachments: None,
        };

        let output = serde_json::to_value(&step).expect("serializable step");
        assert!(output.get("attachments").is_none());
    }

    #[test]
    fn test_suite_round_trips_nested_test_cases() {
        let input = r#"{
            "suiteId": "S-001",
            "name": "Regression",
            "testCases": [{
                "testCaseId": "TC-001",
                "title": "Login",
                "expectedResult": "User is authenticated"
            }]
        }"#;

        let suite: TestSuite = serde_json::from_str(input).expect("valid suite");
        assert_eq!(suite.test_cases.len(), 1);

        let output = serde_json::to_value(&suite).expect("serializable suite");
        assert_eq!(output["suiteId"], "S-001");
        assert_eq!(output["testCases"][0]["testCaseId"], "TC-001");
        assert!(output.get("description").is_none());
    }

    #[test]
    fn test_run_round_trips_all_optional_collections() {
        let input = r#"{
            "testRunId": "R-001",
            "timestamp": "2026-09-02T00:00:00Z",
            "projects": [],
            "testSuites": [],
            "testCases": []
        }"#;

        let run: TestRun = serde_json::from_str(input).expect("valid run");
        assert!(run.projects.as_ref().is_some_and(|items| items.is_empty()));

        let output = serde_json::to_value(&run).expect("serializable run");
        assert_eq!(output["testRunId"], "R-001");
        assert!(output["testSuites"].is_array());
    }

    #[test]
    fn test_run_omits_absent_collections() {
        let run: TestRun =
            serde_json::from_str(r#"{"testRunId":"R-002","timestamp":"2026-09-02T00:00:00Z"}"#)
                .expect("valid minimal run");

        let output = serde_json::to_value(&run).expect("serializable run");
        assert!(output.get("projects").is_none());
        assert!(output.get("testSuites").is_none());
        assert!(output.get("testCases").is_none());
        assert!(output.get("results").is_none());
        assert!(output.get("caseVersions").is_none());
    }

    #[test]
    fn test_run_round_trips_the_versions_it_pinned() {
        let input = r#"{
            "testRunId": "R-004",
            "timestamp": "2026-09-05T00:00:00Z",
            "caseVersions": {"TC-001": 2, "TC-002": 1}
        }"#;

        let run: TestRun = serde_json::from_str(input).expect("valid run");
        let pinned = run.case_versions.as_ref().expect("pinned versions");
        assert_eq!(pinned.get("TC-001"), Some(&2));
        assert_eq!(pinned.get("TC-002"), Some(&1));

        let output = serde_json::to_value(&run).expect("serializable run");
        assert_eq!(output["caseVersions"]["TC-001"], 2);
    }

    #[test]
    fn test_run_round_trips_test_case_results() {
        let input = r#"{
            "testRunId": "R-003",
            "timestamp": "2026-09-04T12:00:00Z",
            "results": [{
                "testCaseId": "TC-001.json",
                "status": "Passed",
                "timestamp": "2026-09-04T12:05:00Z",
                "notes": "Verified login form"
            }]
        }"#;

        let run: TestRun = serde_json::from_str(input).expect("valid run with results");
        let results = run.results.as_ref().expect("results present");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].test_case_id, "TC-001.json");
        assert_eq!(results[0].status, "Passed");

        let output = serde_json::to_value(&run).expect("serializable run");
        assert_eq!(output["results"][0]["status"], "Passed");
    }

    #[test]
    fn defect_link_round_trips_and_omits_optional_fields() {
        let input = r#"{
            "linkId": "L-001",
            "defectId": "BUG-42",
            "defectUrl": "https://jira.example.com/browse/BUG-42",
            "trackerType": "jira",
            "title": "Login fails under load",
            "status": "Open",
            "linkedAt": "2026-09-10T12:00:00Z"
        }"#;

        let link: DefectLink = serde_json::from_str(input).expect("valid defect link");
        assert_eq!(link.link_id, "L-001");
        assert_eq!(link.tracker_type, "jira");
        assert_eq!(link.title.as_deref(), Some("Login fails under load"));

        let output = serde_json::to_value(&link).expect("serializable defect link");
        assert_eq!(output["linkId"], "L-001");
        assert_eq!(
            output["defectUrl"],
            "https://jira.example.com/browse/BUG-42"
        );
        assert_eq!(output["trackerType"], "jira");
        assert_eq!(output["status"], "Open");

        let minimal: DefectLink = serde_json::from_str(
            r#"{
                "linkId": "L-002",
                "defectId": "123",
                "defectUrl": "https://github.com/org/repo/issues/123",
                "trackerType": "github",
                "linkedAt": "2026-09-10T12:00:00Z"
            }"#,
        )
        .expect("valid minimal defect link");
        assert!(minimal.title.is_none());
        assert!(minimal.status.is_none());

        let output = serde_json::to_value(&minimal).expect("serializable minimal link");
        assert!(output.get("title").is_none());
        assert!(output.get("status").is_none());
    }

    #[test]
    fn test_case_result_carries_defect_links_and_omits_absent_ones() {
        let input = r#"{
            "testCaseId": "TC-001.json",
            "status": "Failed",
            "timestamp": "2026-09-10T12:00:00Z",
            "defectLinks": [{
                "linkId": "L-001",
                "defectId": "BUG-42",
                "defectUrl": "https://jira.example.com/browse/BUG-42",
                "trackerType": "jira",
                "linkedAt": "2026-09-10T12:00:00Z"
            }]
        }"#;

        let result: TestCaseResult = serde_json::from_str(input).expect("valid result");
        let links = result.defect_links.as_ref().expect("defect links present");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].defect_id, "BUG-42");

        let output = serde_json::to_value(&result).expect("serializable result");
        assert_eq!(output["defectLinks"][0]["defectId"], "BUG-42");

        let bare: TestCaseResult = serde_json::from_str(
            r#"{"testCaseId":"TC-002.json","status":"Passed","timestamp":"2026-09-10T12:00:00Z"}"#,
        )
        .expect("valid result without links");
        assert!(bare.defect_links.is_none());
        let output = serde_json::to_value(&bare).expect("serializable bare result");
        assert!(output.get("defectLinks").is_none());
    }

    #[test]
    fn milestone_round_trips_and_omits_optional_fields() {
        let input = r#"{
            "milestoneId": "M-001.json",
            "name": "Sprint 42",
            "startDate": "2026-09-01",
            "targetDate": "2026-09-15",
            "status": "Open",
            "testRunIds": ["RUN-1.json"]
        }"#;

        let milestone: Milestone = serde_json::from_str(input).expect("valid milestone");
        assert_eq!(milestone.name, "Sprint 42");
        assert_eq!(milestone.target_date.as_deref(), Some("2026-09-15"));
        assert!(milestone.test_suite_ids.is_none());

        let output = serde_json::to_value(&milestone).expect("serializable milestone");
        assert_eq!(output["milestoneId"], "M-001.json");
        assert_eq!(output["testRunIds"][0], "RUN-1.json");
        assert!(output.get("description").is_none());
    }

    #[test]
    fn snake_case_field_names_are_rejected() {
        let result = serde_json::from_str::<TestCase>(
            r#"{"test_case_id":"TC-001","title":"Legacy","expected_result":"Pass"}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn project_round_trips_legacy_json() {
        let input = r#"{
            "projectId": "P-001",
            "name": "Checkout",
            "description": "Purchase flow",
            "testSuites": [{
                "suiteId": "S-001",
                "name": "Happy path",
                "testCases": [{
                    "testCaseId": "TC-001",
                    "title": "Buy an item",
                    "steps": ["Add item", "Pay"],
                    "expectedResult": "Order is created",
                    "priority": "high",
                    "exploratory": false
                }]
            }]
        }"#;

        let project: Project = serde_json::from_str(input).expect("valid project");
        let output = serde_json::to_value(project).expect("serializable project");

        assert_eq!(output["projectId"], "P-001");
        assert_eq!(
            output["testSuites"][0]["testCases"][0]["expectedResult"],
            "Order is created"
        );
        assert!(output["testSuites"][0]["description"].is_null());
    }

    #[test]
    fn missing_required_fields_are_rejected() {
        let result =
            serde_json::from_str::<TestCase>(r#"{"testCaseId":"TC-001","title":"Incomplete"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn missing_required_fields_are_rejected_for_every_resource() {
        assert!(
            serde_json::from_str::<Project>(r#"{"projectId":"P-001","name":"No suites"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<TestSuite>(r#"{"suiteId":"S-001","name":"No cases"}"#).is_err()
        );
        assert!(serde_json::from_str::<TestRun>(r#"{"testRunId":"R-001"}"#).is_err());
        assert!(
            serde_json::from_str::<Attachment>(
                r#"{"filename":"a.txt","originalName":"a.txt","mimeType":"text/plain"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let result = serde_json::from_str::<TestRun>(
            r#"{"testRunId":"R-001","timestamp":"2026-09-02T00:00:00Z","unexpected":true}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn unknown_fields_are_rejected_for_every_resource() {
        assert!(
            serde_json::from_str::<Project>(
                r#"{"projectId":"P-001","name":"X","testSuites":[],"rogue":1}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<TestSuite>(
                r#"{"suiteId":"S-001","name":"X","testCases":[],"rogue":1}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<TestCase>(
                r#"{"testCaseId":"TC-001","title":"X","expectedResult":"Y","rogue":1}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<DefectLink>(
                r#"{"linkId":"L-001","defectId":"B-1","defectUrl":"https://x/1","trackerType":"jira","linkedAt":"2026-09-10T12:00:00Z","rogue":1}"#
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(serde_json::from_str::<Project>("{ not json }").is_err());
    }
}
