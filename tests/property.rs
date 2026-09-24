//! Property tests for document deserialisation and identifier sanitisation.
//!
//! Every document type in [`tucano_test::models`] is driven through
//! `serde_json::from_str`, `from_slice` and `from_value` with arbitrary input.
//! A decode must answer — `Ok` or `Err` — and never panic, no matter how
//! malformed the document is. The same contract is pinned for the parsers that
//! turn request bodies and imported reports into documents, and for every
//! helper that turns an identifier or a file name into a stored path: an
//! accepted path stays inside the data root and exposes only plain components,
//! while a refused identifier is refused before any path is built.
//!
//! These properties run in `cargo test` and stay fast by construction — the
//! generated inputs are bounded, and the long-running coverage-guided mutator
//! lives in the `fuzz/` targets instead. Both halves are described in
//! `docs/testing/fuzz-and-property-tests.md`.

use std::collections::BTreeSet;
use std::io;
use std::path::{Component, Path, PathBuf};

use proptest::collection;
use proptest::prelude::*;
use serde_json::Value;
use tucano_test::domain::{
    content_disposition, defect, import, mime_type, original_name, reports, validation,
};
use tucano_test::models::{
    Attachment, CaseHistoryEntry, CoverageReport, DefectLink, DefectLinkRequest, ImportCounts,
    ImportSummary, Milestone, MilestoneProgress, Project, StepAttachment, SuiteCoverage,
    SummaryReport, TestCase, TestCaseResult, TestCaseStep, TestConfiguration, TestRun, TestStep,
    TestSuite,
};
use tucano_test::storage::{
    Parent, Resource, attachment_path, case_dir, case_marker, ensure_within, folder_name,
    folder_wire_id, node_folder, project_dir, project_document_path, project_marker, revision_dir,
    revision_marker, root_dir, step_attachment_path, step_dir, suite_dir, suite_marker,
    validate_component, validate_document_id,
};

/// Applies `$apply!` to every document type this suite decodes.
///
/// The list is written once: the properties that decode text, bytes and JSON
/// values, the sample table, and the fuzz-target drift guard all expand from it,
/// so a model can never be covered in one place and forgotten in another.
/// `$apply!` receives the type and `$input`, and expands to an expression: the
/// separators below belong to this macro, which is what keeps the list usable
/// where a run of statements is expected.
///
/// The input is a macro argument rather than a captured name because
/// `macro_rules!` hygiene hides a binding created by the call site — a
/// generated document must be handed in, never reached for.
macro_rules! for_every_model {
    ($apply:ident, $input:expr) => {
        $apply!(Attachment, $input);
        $apply!(StepAttachment, $input);
        $apply!(TestStep, $input);
        $apply!(TestCaseStep, $input);
        $apply!(TestCase, $input);
        $apply!(TestSuite, $input);
        $apply!(Project, $input);
        $apply!(DefectLink, $input);
        $apply!(DefectLinkRequest, $input);
        $apply!(TestCaseResult, $input);
        $apply!(ImportCounts, $input);
        $apply!(ImportSummary, $input);
        $apply!(TestRun, $input);
        $apply!(TestConfiguration, $input);
        $apply!(Milestone, $input);
        $apply!(MilestoneProgress, $input);
        $apply!(CaseHistoryEntry, $input);
        $apply!(CoverageReport, $input);
        $apply!(SuiteCoverage, $input);
        $apply!(SummaryReport, $input);
    };
}

/// One valid document per model, keyed by the model's own name.
///
/// Every sample must decode — `every_sample_decodes_as_its_model` enforces it,
/// and `the_samples_cover_exactly_the_models_this_suite_decodes` keeps the
/// labels and the model list from drifting apart. The samples are the seed
/// corpus of the mutation property: mutating a document that is known to parse
/// reaches the typed decoders, where mutating random noise mostly reaches the
/// JSON syntax error path.
const SAMPLES: &[(&str, &str)] = &[
    (
        "Attachment",
        r#"{"filename":"1726000000000000-notes.txt","originalName":"notes.txt","mimeType":"text/plain","size":12,"uploadedAt":"2026-09-02T00:00:00Z"}"#,
    ),
    (
        "StepAttachment",
        r#"{"filename":"1726000000000000-shot.png","originalName":"shot.png","mimeType":"image/png","size":2048}"#,
    ),
    (
        "TestStep",
        r#"{"action":"Click Pay Now","expectedResult":"Payment processed","attachments":[{"filename":"1-shot.png","originalName":"shot.png","mimeType":"image/png","size":2048}]}"#,
    ),
    ("TestCaseStep", r#""Navigate to /checkout""#),
    (
        "TestCase",
        r#"{"testCaseId":"TC-001","title":"Login","description":"Sign in","preconditions":"An account exists","steps":["Navigate to /login",{"action":"Click Submit","expectedResult":"Dashboard shown"}],"expectedResult":"Authenticated","priority":"High","severity":"Critical","testType":"Functional","exploratory":false,"attachments":[{"filename":"1-shot.png","originalName":"shot.png","mimeType":"image/png","size":2048}],"tags":["smoke"],"version":2,"lastModified":"2026-09-04T12:00:00Z"}"#,
    ),
    (
        "TestSuite",
        r#"{"suiteId":"S-001.json","name":"smoke","description":"Smoke suite","testCases":[{"testCaseId":"TC-001","title":"Login","expectedResult":"Authenticated"}],"tags":["smoke"]}"#,
    ),
    (
        "Project",
        r#"{"projectId":"P-001.json","name":"checkout","description":"Storefront","testSuites":[{"suiteId":"S-001.json","name":"smoke","testCases":[{"testCaseId":"TC-001","title":"Login","expectedResult":"Authenticated"}]}],"tags":["smoke"]}"#,
    ),
    (
        "DefectLink",
        r#"{"linkId":"L-001","defectId":"BUG-42","defectUrl":"https://jira.example.com/browse/BUG-42","trackerType":"jira","title":"Login fails under load","status":"Open","linkedAt":"2026-09-10T12:00:00Z"}"#,
    ),
    (
        "DefectLinkRequest",
        r#"{"defectId":"BUG-42","defectUrl":"https://jira.example.com/browse/BUG-42","trackerType":"jira","title":"Login fails under load"}"#,
    ),
    (
        "TestCaseResult",
        r#"{"testCaseId":"TC-001.json","status":"Failed","timestamp":"2026-09-10T12:00:00Z","notes":"Regression","durationMs":1200,"defectLinks":[{"linkId":"L-001","defectId":"BUG-42","defectUrl":"https://jira.example.com/browse/BUG-42","trackerType":"jira","linkedAt":"2026-09-10T12:00:00Z"}]}"#,
    ),
    ("ImportCounts", r#"{"passed":1,"failed":1,"blocked":0}"#),
    (
        "ImportSummary",
        r#"{"imported":2,"skipped":1,"errors":0,"duplicates":1,"summary":{"passed":1,"failed":1,"blocked":0}}"#,
    ),
    (
        "TestRun",
        r#"{"testRunId":"R-001.json","timestamp":"2026-09-04T12:00:00Z","name":"nightly","results":[{"testCaseId":"TC-001.json","status":"Passed","timestamp":"2026-09-04T12:05:00Z"}],"tags":["smoke"],"configurations":[{"configId":"C-1.json","name":"Chrome"}],"caseVersions":{"TC-001.json":2}}"#,
    ),
    (
        "TestConfiguration",
        r#"{"configId":"C-1.json","name":"Chrome on Linux","browser":"Chrome","os":"Linux","device":"Desktop","resolution":"1920x1080"}"#,
    ),
    (
        "Milestone",
        r#"{"milestoneId":"M-001.json","name":"Sprint 42","description":"Release 1.0","startDate":"2026-09-01","targetDate":"2026-09-15","status":"Open","testSuiteIds":["S-001.json"],"testRunIds":["R-001.json"]}"#,
    ),
    (
        "MilestoneProgress",
        r#"{"milestoneId":"M-001.json","totalCases":3,"passed":1,"failed":1,"blocked":0,"untested":1,"retest":0,"passPercentage":33.333333}"#,
    ),
    (
        "CaseHistoryEntry",
        r#"{"version":2,"lastModified":"2026-09-04T12:00:00Z","changedFields":["title","steps"]}"#,
    ),
    (
        "CoverageReport",
        r#"{"projectId":"P-001.json","totalCases":2,"suites":[{"suiteId":"S-001.json","name":"smoke","caseCount":2}]}"#,
    ),
    (
        "SuiteCoverage",
        r#"{"suiteId":"S-001.json","name":"smoke","caseCount":2}"#,
    ),
    (
        "SummaryReport",
        r#"{"total":3,"passed":1,"failed":1,"blocked":0,"untested":1,"passPercentage":33.333333,"totalDurationMs":1200}"#,
    ),
];

/// Field names the models declare, in their camelCase wire spelling.
///
/// Objects built from these keys reach the typed decoders of every model
/// instead of being rejected as unknown fields before any field is read.
const MODEL_KEYS: &[&str] = &[
    "filename",
    "originalName",
    "mimeType",
    "size",
    "uploadedAt",
    "action",
    "expectedResult",
    "attachments",
    "testCaseId",
    "title",
    "description",
    "preconditions",
    "steps",
    "priority",
    "severity",
    "testType",
    "exploratory",
    "tags",
    "version",
    "lastModified",
    "suiteId",
    "name",
    "testCases",
    "projectId",
    "testSuites",
    "linkId",
    "defectId",
    "defectUrl",
    "trackerType",
    "status",
    "linkedAt",
    "defectLinks",
    "notes",
    "durationMs",
    "passed",
    "failed",
    "blocked",
    "imported",
    "skipped",
    "errors",
    "duplicates",
    "summary",
    "testRunId",
    "timestamp",
    "projects",
    "results",
    "configurations",
    "caseVersions",
    "configId",
    "browser",
    "os",
    "device",
    "resolution",
    "milestoneId",
    "startDate",
    "targetDate",
    "testSuiteIds",
    "testRunIds",
    "totalCases",
    "untested",
    "retest",
    "passPercentage",
    "changedFields",
    "caseCount",
    "total",
    "totalDurationMs",
];

/// A JSON scalar, including the numbers JSON cannot spell (`NaN`, the
/// infinities) so the value strategies stay total.
fn scalar() -> BoxedStrategy<Value> {
    prop_oneof![
        2 => Just(Value::Null),
        2 => any::<bool>().prop_map(Value::Bool),
        3 => any::<i64>().prop_map(|number| Value::Number(number.into())),
        2 => any::<f64>().prop_map(|number| {
            serde_json::Number::from_f64(number).map_or(Value::Null, Value::Number)
        }),
        4 => any::<String>().prop_map(Value::String),
    ]
    .boxed()
}

/// One key the models know, so the object strategies carry decodable fields.
fn model_key() -> BoxedStrategy<String> {
    (0..MODEL_KEYS.len())
        .prop_map(|index| MODEL_KEYS[index].to_owned())
        .boxed()
}

/// A JSON value nested `depth` levels deep, built only from model field names.
fn json_value(depth: u32) -> BoxedStrategy<Value> {
    if depth == 0 {
        return scalar();
    }
    let nested = json_value(depth - 1);
    let array = collection::vec(nested.clone(), 0..4).prop_map(Value::Array);
    let object = collection::vec((model_key(), nested), 0..6)
        .prop_map(|pairs| Value::Object(pairs.into_iter().collect()));
    prop_oneof![
        4 => scalar(),
        2 => array,
        1 => object,
    ]
    .boxed()
}

/// A full document-shaped value: three levels of nesting is enough to reach
/// every collection and every untagged enum a document carries.
fn json_document() -> BoxedStrategy<Value> {
    json_value(3)
}

/// Text a decoder may be handed: arbitrary UTF-8, arbitrary bytes read as
/// UTF-8, and arbitrary JSON.
fn arbitrary_text() -> BoxedStrategy<String> {
    prop_oneof![
        4 => any::<String>(),
        2 => collection::vec(any::<u8>(), 0..96)
            .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        2 => json_document().prop_map(|value| value.to_string()),
    ]
    .boxed()
}

/// A document that is known to parse, with a handful of bytes replaced or
/// truncated. The mutation keeps the seed's shape, so the mutator spends its
/// cases inside the typed decoders rather than in the JSON grammar.
fn mutated_sample() -> BoxedStrategy<String> {
    (
        0..SAMPLES.len(),
        collection::vec((any::<bool>(), any::<usize>(), any::<u8>()), 1..6),
    )
        .prop_map(|(index, mutations)| {
            let mut bytes = SAMPLES[index].1.as_bytes().to_vec();
            for (truncate, offset, replacement) in mutations {
                if bytes.is_empty() {
                    break;
                }
                let at = offset % bytes.len();
                if truncate {
                    bytes.truncate(at);
                } else {
                    bytes[at] = replacement;
                }
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
        .boxed()
}

/// Every resource the storage layout knows, for the identifier properties.
fn any_resource() -> BoxedStrategy<Resource> {
    (0..Resource::ALL.len())
        .prop_map(|index| Resource::ALL[index])
        .boxed()
}

/// The document types this suite decodes, in the order the visitor lists them.
fn model_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    macro_rules! record {
        ($model:ty, $input:expr) => {
            names.push(stringify!($model))
        };
    }
    for_every_model!(record, ());
    names
}

/// The input is threaded in as an argument, never captured: a proptest body
/// binds its case value in a scope the expansion cannot see.
macro_rules! decode_text {
    ($model:ty, $input:expr) => {{
        let outcome: Result<$model, serde_json::Error> = serde_json::from_str($input);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "{} did not answer for {:?}",
            stringify!($model),
            $input
        );
    }};
}

macro_rules! decode_bytes {
    ($model:ty, $input:expr) => {{
        let outcome: Result<$model, serde_json::Error> = serde_json::from_slice($input);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "{} did not answer for {:?}",
            stringify!($model),
            $input
        );
    }};
}

macro_rules! decode_value {
    ($model:ty, $input:expr) => {{
        let outcome: Result<$model, serde_json::Error> = serde_json::from_value($input);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "{} did not answer for {}",
            stringify!($model),
            $input
        );
    }};
}

/// Checks one path builder: an accepted path is inside the data root, as deep
/// as the layout says, and built only from plain components; a refused input is
/// refused by the identifier guard, not by the filesystem afterwards.
fn check_builder(
    root: &Path,
    label: &str,
    expected_depth: usize,
    built: io::Result<PathBuf>,
) -> Result<(), TestCaseError> {
    match built {
        Ok(path) => {
            prop_assert!(
                path.starts_with(root),
                "{} built {:?}, which is outside {:?}",
                label,
                path,
                root
            );
            prop_assert!(
                ensure_within(root, &path).is_ok(),
                "{} built {:?}, which the containment guard refuses",
                label,
                path
            );
            let relative = path
                .strip_prefix(root)
                .expect("a path that starts with the root is strippable");
            let components: Vec<Component<'_>> = relative.components().collect();
            prop_assert_eq!(
                components.len(),
                expected_depth,
                "{} built {:?}, which is {} components below the data root",
                label,
                path,
                components.len()
            );
            prop_assert!(
                components
                    .iter()
                    .all(|component| matches!(component, Component::Normal(_))),
                "{} built {:?}, which is not made of plain components",
                label,
                path
            );
            prop_assert!(
                path.file_name().is_some_and(|name| !name.is_empty()),
                "{} built {:?}, which has no file name",
                label,
                path
            );
            Ok(())
        }
        Err(error) => {
            prop_assert_eq!(
                error.kind(),
                io::ErrorKind::InvalidInput,
                "{} refused its input with {:?} instead of a component rejection",
                label,
                error.kind()
            );
            Ok(())
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Every `from_str` entry point answers arbitrary text without panicking.
    #[test]
    fn every_model_decodes_arbitrary_text_without_panicking(text in arbitrary_text()) {
        for_every_model!(decode_text, &text);
    }

    /// Every `from_slice` entry point answers arbitrary bytes without panicking.
    #[test]
    fn every_model_decodes_arbitrary_bytes_without_panicking(
        bytes in collection::vec(any::<u8>(), 0..96)
    ) {
        for_every_model!(decode_bytes, &bytes);
    }

    /// Every `from_value` entry point answers arbitrary values without panicking.
    #[test]
    fn every_model_decodes_arbitrary_json_values_without_panicking(value in json_document()) {
        for_every_model!(decode_value, value.clone());
    }

    /// A document that parses, with a few bytes changed, still only ever
    /// answers — the mutator cannot reach a panic through a valid seed.
    #[test]
    fn every_model_decodes_mutations_of_valid_documents_without_panicking(
        text in mutated_sample()
    ) {
        for_every_model!(decode_text, &text);
    }

    /// A component is accepted exactly when it names one plain path element.
    #[test]
    fn validate_component_accepts_exactly_single_plain_components(component in any::<String>()) {
        let accepted = !component.is_empty()
            && component != "."
            && component != ".."
            && !component.contains('/')
            && !component.contains('\\')
            && !component.contains('\0');
        prop_assert_eq!(
            validate_component(&component).is_ok(),
            accepted,
            "validate_component({:?})",
            component
        );
    }

    /// A component carrying a separator or a NUL is refused however it is
    /// dressed up around that character.
    #[test]
    fn a_component_containing_a_forbidden_character_is_refused(
        head in any::<String>(),
        tail in any::<String>(),
        forbidden in prop_oneof![Just('/'), Just('\\'), Just('\0')],
    ) {
        let component = format!("{head}{forbidden}{tail}");
        prop_assert!(
            validate_component(&component).is_err(),
            "validate_component({component:?}) accepted a forbidden character"
        );
    }

    /// A project or suite folder name round-trips back to its own wire id,
    /// whatever the folder is called.
    #[test]
    fn a_wire_id_round_trips_through_its_folder_name(folder in any::<String>()) {
        let wire_id = folder_wire_id(&folder);
        prop_assert_eq!(folder_name(&wire_id), folder.as_str());
    }

    /// A hierarchy node is accepted exactly when the layout can store the
    /// identifier, and what it returns is always a single plain name.
    #[test]
    fn node_folder_accepts_exactly_the_identifiers_the_layout_can_store(
        resource in any_resource(),
        id in any::<String>(),
    ) {
        let expected = match resource {
            Resource::Cases => validate_component(&id).is_ok(),
            Resource::Projects | Resource::Suites => id
                .strip_suffix(".json")
                .is_some_and(|folder| validate_component(folder).is_ok()),
            Resource::Runs | Resource::Milestones | Resource::Configurations => false,
        };
        prop_assert_eq!(
            node_folder(resource, &id).is_ok(),
            expected,
            "node_folder({:?}, {:?})",
            resource,
            id
        );
        if let Ok(folder) = node_folder(resource, &id) {
            prop_assert!(
                validate_component(folder).is_ok(),
                "node_folder({:?}, {:?}) returned {folder:?}",
                resource,
                id
            );
            prop_assert!(
                !folder.contains('/') && !folder.contains('\\') && !folder.contains('\0'),
                "node_folder({:?}, {:?}) returned {folder:?}",
                resource,
                id
            );
            prop_assert_ne!(folder, "..", "node_folder({:?}, {:?})", resource, id);
        }
    }

    /// A document identifier is a plain component that carries the suffix its
    /// resource requires — nothing else is accepted.
    #[test]
    fn validate_document_id_accepts_exactly_a_component_with_its_suffix(
        resource in any_resource(),
        id in any::<String>(),
    ) {
        let expected = validate_component(&id).is_ok()
            && (!resource.id_requires_json_suffix() || id.ends_with(".json"));
        prop_assert_eq!(
            validate_document_id(resource, &id).is_ok(),
            expected,
            "validate_document_id({:?}, {:?})",
            resource,
            id
        );
    }

    /// Every path builder refuses a hostile component before it builds a path,
    /// and every path it does build stays inside the data root at the depth the
    /// layout documents.
    #[test]
    fn every_path_builder_stays_inside_the_data_root(
        project in any::<String>(),
        suite in any::<String>(),
        case in any::<String>(),
        filename in any::<String>(),
        run in any::<String>(),
    ) {
        let directory = tempfile::tempdir().expect("a temporary data root");
        let root = directory.path();
        let project_wire = format!("{project}.json");
        let suite_wire = format!("{suite}.json");
        let run_wire = format!("{run}.json");
        let project_parent = Parent::Project(project_wire.clone());
        let suite_parent = Parent::Suite {
            project: project_wire.clone(),
            suite: suite_wire.clone(),
        };

        check_builder(root, "root_dir", 1, root_dir(root, Resource::Projects))?;
        check_builder(root, "project_dir", 2, project_dir(root, &project_wire))?;
        check_builder(root, "project_marker", 3, project_marker(root, &project_wire))?;
        check_builder(
            root,
            "project_document_path",
            4,
            project_document_path(root, &project_wire, Resource::Runs, &run_wire),
        )?;
        check_builder(root, "suite_dir", 3, suite_dir(root, &project_wire, &suite_wire))?;
        check_builder(
            root,
            "suite_marker",
            4,
            suite_marker(root, &project_wire, &suite_wire),
        )?;
        check_builder(root, "case_dir in a project", 3, case_dir(root, &project_parent, &case))?;
        check_builder(root, "case_dir in a suite", 4, case_dir(root, &suite_parent, &case))?;
        check_builder(root, "case_marker", 4, case_marker(root, &project_parent, &case))?;
        check_builder(root, "revision_dir", 4, revision_dir(root, &project_parent, &case))?;
        check_builder(
            root,
            "revision_marker",
            5,
            revision_marker(root, &project_parent, &case, 3),
        )?;
        check_builder(
            root,
            "attachment_path",
            4,
            attachment_path(root, &project_parent, &case, &filename),
        )?;
        check_builder(root, "step_dir", 5, step_dir(root, &project_parent, &case, 2))?;
        check_builder(
            root,
            "step_attachment_path",
            6,
            step_attachment_path(root, &project_parent, &case, 2, &filename),
        )?;

        if let Ok(path) = attachment_path(root, &project_parent, &case, &filename) {
            prop_assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(filename.as_str()),
                "an accepted attachment keeps its file name"
            );
        }
    }

    /// The JUnit parser answers arbitrary text without panicking.
    #[test]
    fn the_junit_parser_never_panics(xml in arbitrary_text()) {
        let outcome = import::parse(&xml);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "the JUnit parser did not answer for {xml:?}"
        );
    }

    /// The JSON import parser answers arbitrary bytes without panicking.
    #[test]
    fn the_json_import_parser_never_panics(bytes in collection::vec(any::<u8>(), 0..96)) {
        let outcome = import::parse_json(&bytes);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "the JSON import parser did not answer for {bytes:?}"
        );
    }

    /// Payload validation answers every resource and every body without
    /// panicking.
    #[test]
    fn payload_validation_never_panics(resource in any_resource(), body in json_document()) {
        let outcome = validation::validate_payload(resource, &body);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "validate_payload({:?}, {})",
            resource,
            body
        );
    }

    /// The defect request parser answers arbitrary bodies without panicking.
    #[test]
    fn the_defect_request_parser_never_panics(body in json_document()) {
        let outcome = defect::parse_request(&body);
        prop_assert!(matches!(outcome, Ok(_) | Err(_)), "parse_request({body})");
    }

    /// The defect URL validator answers arbitrary trackers and URLs without
    /// panicking.
    #[test]
    fn the_defect_url_validator_never_panics(tracker in any::<String>(), url in any::<String>()) {
        let outcome = defect::validate_url(&tracker, &url);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "validate_url({tracker:?}, {url:?})"
        );
    }

    /// The same holds for the four trackers the API documents, which route into
    /// the per-tracker URL shapes.
    #[test]
    fn every_known_tracker_url_is_validated_without_panicking(
        index in 0..defect::TRACKER_TYPES.len(),
        url in any::<String>(),
    ) {
        let tracker = defect::TRACKER_TYPES[index];
        let outcome = defect::validate_url(tracker, &url);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "validate_url({tracker:?}, {url:?})"
        );
    }

    /// The date filter parser answers arbitrary text without panicking.
    #[test]
    fn the_date_filter_parser_never_panics(value in any::<String>()) {
        let outcome = reports::parse_date_filter(&value);
        prop_assert!(
            matches!(outcome, Ok(_) | Err(_)),
            "parse_date_filter({value:?})"
        );
    }

    /// A name made only of the characters RFC 5987 allows unencoded travels in
    /// both `filename` and `filename*` form, so the header carrying it is a
    /// ready-made HTTP header value. The argument is read as a *stored* name, so
    /// what travels is the original name recovered from it: a leading run of
    /// digits and one hyphen is the storage prefix, and only what follows it
    /// reaches the header.
    #[test]
    fn a_plain_stored_name_needs_no_rfc5987_form(name in "[a-zA-Z0-9._^-]{1,32}") {
        let recovered = original_name(&name);
        let disposition = content_disposition(&name);
        let expected = format!("attachment; filename=\"{recovered}\"");
        prop_assert_eq!(&disposition, &expected);
        prop_assert!(
            axum::http::HeaderValue::from_str(&disposition).is_ok(),
            "{disposition:?} is not a header value"
        );
    }

    /// Whatever the uploader called the file, the disposition header it is
    /// downloaded with is printable ASCII with no line breaks or NULs, and is
    /// always a valid HTTP header value.
    #[test]
    fn a_content_disposition_is_always_a_safe_ascii_header_value(name in any::<String>()) {
        let disposition = content_disposition(&name);
        prop_assert!(disposition.is_ascii(), "{disposition:?} is not ASCII");
        prop_assert!(
            !disposition
                .chars()
                .any(|character| matches!(character, '\r' | '\n' | '\0')),
            "{disposition:?} carries a control character"
        );
        prop_assert!(
            axum::http::HeaderValue::from_str(&disposition).is_ok(),
            "{disposition:?} is not a header value"
        );
    }

    /// A stored name is `<generated prefix>-<original>`, and the original comes
    /// back unchanged for any original name at all.
    #[test]
    fn a_generated_stored_name_recovers_its_original(
        original in any::<String>(),
        prefix in "[0-9]{1,20}",
    ) {
        let stored = format!("{prefix}-{original}");
        prop_assert_eq!(original_name(&stored), original.as_str());
    }

    /// A name that was not generated is reported as itself, so every result is
    /// a tail of the stored name.
    #[test]
    fn a_recovered_name_is_always_a_tail_of_the_stored_name(stored in any::<String>()) {
        prop_assert!(
            stored.ends_with(original_name(&stored)),
            "original_name({stored:?}) is not a tail of it"
        );
    }

    /// The recorded media type depends only on the lowercased extension, and is
    /// always one of the types the API knows about.
    #[test]
    fn the_media_type_depends_only_on_the_lowercased_extension(extension in any::<String>()) {
        let lower = extension.to_ascii_lowercase();
        let recorded = mime_type(&format!("evidence.{extension}"));
        prop_assert_eq!(recorded, mime_type(&format!("evidence.{lower}")));
        prop_assert!(
            matches!(
                recorded,
                "image/png"
                    | "image/jpeg"
                    | "image/gif"
                    | "application/pdf"
                    | "text/plain"
                    | "application/json"
                    | "application/octet-stream"
            ),
            "the extension {extension:?} was not answered with a known media type"
        );
    }
}

/// Each sample decodes as the model it is labelled with, so the seed corpus of
/// the mutation property can never rot into a corpus of invalid documents.
#[test]
fn every_sample_decodes_as_its_model() {
    macro_rules! check_sample {
        ($model:ty, $input:expr) => {{
            let label = stringify!($model);
            let sample = SAMPLES
                .iter()
                .find(|(name, _)| *name == label)
                .map(|(_, document)| *document)
                .unwrap_or_else(|| panic!("SAMPLES carries no document for {label}"));
            serde_json::from_str::<$model>(sample)
                .unwrap_or_else(|error| panic!("the {label} sample does not decode: {error}"));
        }};
    }
    for_every_model!(check_sample, ());
}

/// The sample table and the model list name exactly the same types, and no type
/// is listed twice.
#[test]
fn the_samples_cover_exactly_the_models_this_suite_decodes() {
    let listed = model_names();
    let decoded: BTreeSet<&str> = listed.iter().copied().collect();
    let labelled: BTreeSet<&str> = SAMPLES.iter().map(|(label, _)| *label).collect();
    assert_eq!(
        listed.len(),
        decoded.len(),
        "the decoded model list names a model more than once"
    );
    assert_eq!(
        labelled, decoded,
        "SAMPLES and the decoded model list have drifted apart"
    );
}

/// The fuzz target decodes the same models this suite decodes: adding a model
/// here without adding it there fails `cargo test`.
#[test]
fn the_fuzz_target_decodes_every_model_this_suite_decodes() {
    let target =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/fuzz_targets/deserialize_models.rs");
    let source = std::fs::read_to_string(&target)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", target.display()));

    let mut missing: Vec<&str> = Vec::new();
    macro_rules! require_model {
        ($model:ty, $input:expr) => {
            if !source.contains(stringify!($model)) {
                missing.push(stringify!($model));
            }
        };
    }
    for_every_model!(require_model, ());

    assert!(
        missing.is_empty(),
        "the deserialisation fuzz target does not decode {missing:?}"
    );
}
