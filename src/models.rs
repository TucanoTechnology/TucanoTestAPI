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
pub struct TestCase {
    pub test_case_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<String>>,
    pub expected_result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exploratory: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestSuite {
    pub suite_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub test_cases: Vec<TestCase>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub project_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub test_suites: Vec<TestSuite>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestRun {
    pub test_run_id: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<Project>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_suites: Option<Vec<TestSuite>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_cases: Option<Vec<TestCase>>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn unknown_fields_are_rejected() {
        let result = serde_json::from_str::<TestRun>(
            r#"{"testRunId":"R-001","timestamp":"2026-09-02T00:00:00Z","unexpected":true}"#,
        );
        assert!(result.is_err());
    }
}
