use super::*;
use crate::storage::FileRepository;
use serde_json::json;
use tempfile::TempDir;

fn service() -> (TestService<FileRepository>, TempDir) {
    let directory = TempDir::new().expect("temporary directory");
    let repository = FileRepository::new(directory.path()).expect("repository");
    (TestService::new(repository), directory)
}

fn list(service: &TestService<FileRepository>, resource: Resource) -> Vec<String> {
    service
        .list(resource, &ListQuery::default())
        .expect("list should succeed")
}

/// Creates `checkout` and returns the parent that owns its children.
///
/// The identity the body supplies is the address the project is filed under, so
/// it names the project the same way [`derive_create_id`] would for the name.
fn project(service: &TestService<FileRepository>) -> Parent {
    service
        .create(
            Resource::Projects,
            &json!({ "projectId": "checkout.json", "name": "checkout" }),
        )
        .expect("project");
    Parent::Project("checkout.json".to_owned())
}

/// The parent of suite `smoke` inside project `checkout`.
fn smoke() -> Parent {
    Parent::Suite {
        project: "checkout.json".to_owned(),
        suite: "smoke.json".to_owned(),
    }
}

/// Creates suite `smoke` inside `project` and returns its parent.
fn add_suite(service: &TestService<FileRepository>, project: &Parent) -> Parent {
    service
        .create_in(
            Resource::Suites,
            project,
            &json!({ "suiteId": "S-1", "name": "smoke" }),
        )
        .expect("suite");
    smoke()
}

fn case(service: &TestService<FileRepository>, parent: &Parent, id: &str) {
    service
        .create_in(
            Resource::Cases,
            parent,
            &json!({ "testCaseId": id, "title": "T", "expectedResult": "E" }),
        )
        .expect("case");
}

#[test]
fn documents_round_trip_through_the_service() {
    let (service, _directory) = service();

    let created = service
        .create(Resource::Projects, &json!({ "name": "checkout" }))
        .expect("create");
    assert_eq!(created.id, "checkout.json");
    assert_eq!(list(&service, Resource::Projects), vec!["checkout.json"]);

    let stored = service
        .get(Resource::Projects, "checkout.json")
        .expect("get");
    assert_eq!(stored["name"], "checkout");

    service
        .update(
            Resource::Projects,
            "checkout.json",
            &json!({ "name": "updated" }),
        )
        .expect("update");
    assert_eq!(
        service
            .get(Resource::Projects, "checkout.json")
            .expect("get")["name"],
        "updated"
    );

    service
        .delete(Resource::Projects, "checkout.json")
        .expect("delete");
    assert!(service.get(Resource::Projects, "checkout.json").is_err());
    assert!(list(&service, Resource::Projects).is_empty());
}

#[test]
fn creating_a_duplicate_is_a_conflict() {
    let (service, _directory) = service();
    service
        .create(Resource::Projects, &json!({ "name": "checkout" }))
        .expect("create");

    let error = service
        .create(Resource::Projects, &json!({ "name": "checkout" }))
        .expect_err("duplicate");
    assert!(matches!(error, DomainError::Conflict(_)));
}

#[test]
fn an_unknown_field_is_rejected_before_anything_is_persisted() {
    let (service, _directory) = service();

    let error = service
        .create(Resource::Projects, &json!({ "name": "alpha", "sneaky": 1 }))
        .expect_err("unknown field");
    assert!(matches!(
        error,
        DomainError::InvalidRequest {
            code: "invalid_request",
            ..
        }
    ));
    assert!(
        list(&service, Resource::Projects).is_empty(),
        "a rejected payload must not be stored"
    );
}

#[test]
fn updating_a_missing_document_is_not_found() {
    let (service, _directory) = service();
    let error = service
        .update(
            Resource::Projects,
            "missing.json",
            &json!({ "name": "missing" }),
        )
        .expect_err("missing");
    assert!(matches!(error, DomainError::NotFound(_)));
}

#[test]
fn listing_filters_by_substring_and_by_tag() {
    let (service, _directory) = service();
    service
        .create(Resource::Projects, &json!({ "name": "alpha" }))
        .expect("alpha");
    service
        .create(
            Resource::Projects,
            &json!({ "name": "beta", "tags": ["Smoke"] }),
        )
        .expect("beta");

    let filtered = service
        .list(
            Resource::Projects,
            &ListQuery {
                configuration: None,
                filter: Some("ALPH".to_owned()),
                tags: None,
            },
        )
        .expect("filter");
    assert_eq!(filtered, vec!["alpha.json"]);

    let tagged = service
        .list(
            Resource::Projects,
            &ListQuery {
                configuration: None,
                filter: None,
                tags: Some(" smoke ".to_owned()),
            },
        )
        .expect("tags");
    assert_eq!(tagged, vec!["beta.json"]);

    let untagged = service
        .list(
            Resource::Projects,
            &ListQuery {
                configuration: None,
                filter: None,
                tags: Some("does-not-exist".to_owned()),
            },
        )
        .expect("tags");
    assert!(untagged.is_empty());
}

#[test]
fn runs_are_listed_by_the_configuration_they_link() {
    let (service, _directory) = service();
    let home = project(&service);
    for (id, name) in [("R-1", "nightly"), ("R-2", "weekly"), ("R-3", "release")] {
        service
            .create_in(
                Resource::Runs,
                &home,
                &json!({ "testRunId": id, "name": name, "timestamp": "1", "tags": ["ci"] }),
            )
            .expect("run");
    }
    for name in ["chrome-linux", "firefox-windows"] {
        service
            .create_in(Resource::Configurations, &home, &json!({ "name": name }))
            .expect("configuration");
    }
    for (run, configuration) in [
        ("nightly.json", "chrome-linux.json"),
        ("weekly.json", "firefox-windows.json"),
    ] {
        service
            .link_configuration_to_run(run, &json!({ "configId": configuration }))
            .expect("link");
    }

    let query = |configuration: &str| ListQuery {
        configuration: Some(configuration.to_owned()),
        ..ListQuery::default()
    };

    assert_eq!(
        service
            .list(Resource::Runs, &query("chrome-linux.json"))
            .unwrap(),
        vec!["nightly.json"]
    );
    assert_eq!(
        service
            .list(Resource::Runs, &query("firefox-windows.json"))
            .unwrap(),
        vec!["weekly.json"]
    );

    // A configuration no run links yields an empty listing rather than an
    // error.
    assert_eq!(
        service
            .list(Resource::Runs, &query("firefox-linux.json"))
            .unwrap(),
        Vec::<String>::new()
    );

    // The configuration filter composes with the substring and tag filters.
    let composed = ListQuery {
        filter: Some("NIGHT".to_owned()),
        tags: Some(" ci ".to_owned()),
        configuration: Some("chrome-linux.json".to_owned()),
    };
    assert_eq!(
        service.list(Resource::Runs, &composed).unwrap(),
        vec!["nightly.json"]
    );

    // A run that links a different configuration drops out of the composed
    // listing even though it carries the same tag.
    let tag_only = ListQuery {
        tags: Some("ci".to_owned()),
        configuration: Some("chrome-linux.json".to_owned()),
        ..ListQuery::default()
    };
    assert_eq!(
        service.list(Resource::Runs, &tag_only).unwrap(),
        vec!["nightly.json"]
    );

    // The filter says nothing about other collections, which keeps their
    // listings intact rather than emptying them.
    assert_eq!(
        service
            .list(Resource::Configurations, &query("chrome-linux.json"))
            .unwrap(),
        vec!["chrome-linux.json", "firefox-windows.json"]
    );
}

#[test]
fn reading_a_project_assembles_its_children() {
    let (service, _directory) = service();
    let project = project(&service);
    let suite = add_suite(&service, &project);
    case(&service, &suite, "TC-suite");
    case(&service, &project, "TC-direct");

    let document = service
        .get(Resource::Projects, "checkout.json")
        .expect("project");
    assert_eq!(document["testSuites"].as_array().map(Vec::len), Some(1));
    assert_eq!(document["testSuites"][0]["name"], "smoke");
    assert_eq!(
        document["testSuites"][0]["testCases"][0]["testCaseId"],
        "TC-suite"
    );
    assert_eq!(document["testCases"].as_array().map(Vec::len), Some(1));
    assert_eq!(document["testCases"][0]["testCaseId"], "TC-direct");

    let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
    assert_eq!(suite_document["testCases"][0]["testCaseId"], "TC-suite");
}

#[test]
fn a_project_without_direct_cases_omits_the_test_cases_field() {
    let (service, _directory) = service();
    project(&service);
    let document = service
        .get(Resource::Projects, "checkout.json")
        .expect("project");
    assert!(document.get("testCases").is_none());
    assert_eq!(document["testSuites"].as_array().map(Vec::len), Some(0));
}

#[test]
fn markers_store_empty_child_arrays() {
    let (service, _directory) = service();
    let project = project(&service);
    service
        .create_in(
            Resource::Suites,
            &project,
            &json!({
                "suiteId": "S-1",
                "name": "smoke",
                "testCases": [{
                    "testCaseId": "TC-ignored",
                    "title": "ignored",
                    "expectedResult": "ignored"
                }]
            }),
        )
        .expect("suite");

    let suite = Parent::Suite {
        project: "checkout.json".to_owned(),
        suite: "smoke.json".to_owned(),
    };
    case(&service, &suite, "TC-real");

    let marker = service
        .repository
        .read_at(Resource::Suites, Some(&project), "smoke.json")
        .expect("marker");
    assert_eq!(
        marker["testCases"].as_array().map(Vec::len),
        Some(0),
        "the marker must not duplicate membership"
    );

    // A nested case in the payload is not materialised as a child either.
    assert_eq!(
        list(&service, Resource::Cases),
        vec!["TC-real".to_owned()],
        "only folders are children"
    );

    let project_marker = service
        .repository
        .read_at(Resource::Projects, None, "checkout.json")
        .expect("project marker");
    assert_eq!(
        project_marker["testSuites"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn an_identifier_in_several_parents_is_a_conflict() {
    let (service, _directory) = service();
    let project = project(&service);
    let suite = add_suite(&service, &project);
    case(&service, &project, "TC-1");
    case(&service, &suite, "TC-1");

    let error = service
        .get(Resource::Cases, "TC-1")
        .expect_err("ambiguous identifier");
    assert!(matches!(error, DomainError::Conflict(ref message) if message.contains("2 parents")));
    assert_eq!(
        list(&service, Resource::Cases),
        vec!["TC-1".to_owned()],
        "lists de-duplicate rather than fail"
    );
    assert!(service.delete(Resource::Cases, "TC-1").is_err());
}

#[test]
fn cases_join_suites_by_copy_and_leave_the_source_alone() {
    let (service, _directory) = service();
    let project = project(&service);
    add_suite(&service, &project);
    case(&service, &project, "TC-001");

    service
        .add_case_to_suite("smoke.json", &json!({ "testCaseId": "TC-001" }))
        .expect("copied into the suite");
    let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
    assert_eq!(
        suite_document["testCases"].as_array().map(Vec::len),
        Some(1)
    );

    let duplicate = service
        .add_case_to_suite("smoke.json", &json!({ "testCaseId": "TC-001" }))
        .expect_err("the suite already owns the case");
    assert!(matches!(duplicate, DomainError::Conflict(_)));

    let missing = service
        .add_case_to_suite("smoke.json", &json!({}))
        .expect_err("missing field");
    assert!(matches!(missing, DomainError::InvalidRequest { .. }));

    // The identifier now names two folders, so the global route refuses it.
    assert!(matches!(
        service
            .require_test_case("TC-001")
            .expect_err("two occurrences"),
        DomainError::Conflict(_)
    ));

    service
        .remove_case_from_suite("smoke.json", "TC-001")
        .expect("remove");
    let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
    assert_eq!(
        suite_document["testCases"].as_array().map(Vec::len),
        Some(0)
    );

    let project_document = service
        .get(Resource::Projects, "checkout.json")
        .expect("project");
    assert_eq!(
        project_document["testCases"].as_array().map(Vec::len),
        Some(1),
        "the copied case left the project's own case untouched"
    );
}

#[test]
fn moving_a_case_leaves_it_with_one_home() {
    let (service, _directory) = service();
    let project = project(&service);
    let suite = add_suite(&service, &project);
    case(&service, &project, "TC-001");

    let composed = service
        .compose(
            Resource::Cases,
            &suite,
            &json!({ "testCaseId": "TC-001", "mode": "move" }),
        )
        .expect("move");
    assert_eq!(
        composed,
        Composed::Placed {
            id: "TC-001".to_owned(),
            mode: Placement::Move
        }
    );

    let document = service
        .get(Resource::Projects, "checkout.json")
        .expect("project");
    assert!(document.get("testCases").is_none(), "the old home lost it");
    let suite_document = service.get(Resource::Suites, "smoke.json").expect("suite");
    assert_eq!(
        suite_document["testCases"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(list(&service, Resource::Cases), vec!["TC-001".to_owned()]);
}

#[test]
fn a_suite_can_be_placed_into_another_project() {
    let (service, _directory) = service();
    let project = project(&service);
    add_suite(&service, &project);
    service
        .create(Resource::Projects, &json!({ "name": "other" }))
        .expect("other project");
    let other = Parent::Project("other.json".to_owned());

    service
        .compose(
            Resource::Suites,
            &other,
            &json!({ "suiteId": "smoke.json" }),
        )
        .expect("copy");

    assert_eq!(
        service
            .list_children(&other, Resource::Suites)
            .expect("children"),
        vec!["smoke.json".to_owned()]
    );
    assert_eq!(
        service
            .list_children(&project, Resource::Suites)
            .expect("children"),
        vec!["smoke.json".to_owned()],
        "a copy leaves the source in place"
    );
}

#[test]
fn composition_grammar_rejects_mixed_and_unknown_modes() {
    let (service, _directory) = service();
    let project = project(&service);

    let mixed = service
        .compose(
            Resource::Suites,
            &project,
            &json!({ "name": "smoke", "mode": "copy" }),
        )
        .expect_err("mixed");
    assert!(matches!(mixed, DomainError::InvalidRequest { .. }));

    let unknown = service
        .compose(
            Resource::Suites,
            &project,
            &json!({ "suiteId": "smoke.json", "mode": "sideways" }),
        )
        .expect_err("unknown mode");
    assert!(matches!(unknown, DomainError::InvalidRequest { .. }));

    let unnamed = service
        .compose(Resource::Suites, &project, &json!({}))
        .expect_err("nothing to create or place");
    assert!(matches!(unnamed, DomainError::InvalidRequest { .. }));
}

#[test]
fn creating_in_a_missing_parent_is_not_found() {
    let (service, _directory) = service();
    let missing = Parent::Project("ghost.json".to_owned());
    let error = service
        .create_in(
            Resource::Suites,
            &missing,
            &json!({ "suiteId": "S-1", "name": "smoke" }),
        )
        .expect_err("no project");
    assert!(matches!(error, DomainError::NotFound(ref m) if m == "Project not found"));
    assert!(list(&service, Resource::Suites).is_empty());
}

#[test]
fn deleting_a_project_cascades_through_the_tree() {
    let (service, _directory) = service();
    let project = project(&service);
    let suite = add_suite(&service, &project);
    case(&service, &suite, "TC-suite");
    case(&service, &project, "TC-direct");

    service
        .delete(Resource::Projects, "checkout.json")
        .expect("cascade");

    assert!(list(&service, Resource::Projects).is_empty());
    assert!(list(&service, Resource::Suites).is_empty());
    assert!(list(&service, Resource::Cases).is_empty());
}

#[test]
fn duplicating_a_project_stores_a_copy_under_a_new_identifier() {
    let (service, _directory) = service();
    service
        .create(
            Resource::Projects,
            &json!({ "projectId": "checkout.json", "name": "checkout" }),
        )
        .expect("create");

    let new_id = service
        .duplicate(
            &duplicate::PROJECT,
            "checkout.json",
            &json!({ "newId": "P-2.json" }),
        )
        .expect("duplicate");
    assert_eq!(new_id, "P-2.json");

    let copy = service.get(Resource::Projects, "P-2.json").expect("copy");
    assert_eq!(copy["projectId"], "P-2.json");
    assert_eq!(copy["name"], "checkout");
    assert!(
        service.get(Resource::Projects, "checkout.json").is_ok(),
        "the source must remain"
    );
}

#[test]
fn a_duplicated_suite_stays_in_its_project() {
    let (service, _directory) = service();
    let project = project(&service);
    service
        .create_in(
            Resource::Suites,
            &project,
            &json!({ "suiteId": "S-1", "name": "smoke" }),
        )
        .expect("suite");

    let new_id = service
        .duplicate(
            &duplicate::SUITE,
            "smoke.json",
            &json!({ "newId": "S-2.json" }),
        )
        .expect("duplicate");
    assert_eq!(new_id, "S-2.json");
    assert_eq!(
        service
            .list_children(&project, Resource::Suites)
            .expect("children"),
        vec!["S-2.json".to_owned(), "smoke.json".to_owned()]
    );
}

#[test]
fn duplicating_onto_an_existing_identifier_is_a_conflict() {
    let (service, _directory) = service();
    service
        .create(Resource::Projects, &json!({ "name": "checkout" }))
        .expect("source");
    service
        .create(Resource::Projects, &json!({ "name": "occupied" }))
        .expect("target");

    let error = service
        .duplicate(
            &duplicate::PROJECT,
            "checkout.json",
            &json!({ "newId": "occupied.json" }),
        )
        .expect_err("duplicate onto an occupied id");
    assert!(matches!(error, DomainError::Conflict(_)));

    let target = service
        .get(Resource::Projects, "occupied.json")
        .expect("target intact");
    assert_eq!(target["name"], "occupied");
}

#[test]
fn duplicating_a_missing_document_reports_the_named_entity() {
    let (service, _directory) = service();
    let error = service
        .duplicate(&duplicate::RUN, "missing.json", &json!({}))
        .expect_err("missing");
    assert!(matches!(error, DomainError::NotFound(message) if message == "Test run not found"));
}

#[test]
fn run_results_are_recorded_and_updated() {
    let (service, _directory) = service();
    let home = project(&service);
    service
        .create_in(
            Resource::Runs,
            &home,
            &json!({
                "testRunId": "R-1",
                "name": "nightly",
                "timestamp": "1",
                "testCases": [{ "testCaseId": "TC-1", "title": "T", "expectedResult": "E" }]
            }),
        )
        .expect("run");

    service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Passed" }),
        )
        .expect("record");
    service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Failed", "notes": "flaky" }),
        )
        .expect("update");

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    let results = run["results"].as_array().expect("results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "Failed");
    assert_eq!(results[0]["notes"], "flaky");

    let invalid = service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Nope" }),
        )
        .expect_err("bad status");
    assert!(matches!(
        invalid,
        DomainError::InvalidRequest {
            code: "invalid_status",
            ..
        }
    ));
}

#[test]
fn a_result_for_a_case_the_run_does_not_hold_is_not_found() {
    let (service, _directory) = service();
    let home = project(&service);
    // The run lists no case of its own, but it already records a result for
    // TC-1, so that case is one the run holds.
    run_in(&service, &home, "nightly");

    service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Failed" }),
        )
        .expect("a case the run already records stays writable");

    let error = service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-2", "status": "Failed" }),
        )
        .expect_err("a case the run never picked up");
    assert!(
        matches!(error, DomainError::NotFound(message) if message == "Test case not in test run")
    );
}

#[test]
fn a_result_body_is_rejected_rather_than_read_field_by_field() {
    let (service, _directory) = service();
    let home = project(&service);
    run_in(&service, &home, "nightly");

    let unknown = service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Passed", "outcome": "ok" }),
        )
        .expect_err("unknown field");
    assert!(
        matches!(unknown, DomainError::InvalidRequest { ref message, .. } if message == "Unknown field `outcome`"),
        "{unknown:?}"
    );

    for body in [
        json!({ "testCaseId": "TC-1", "status": "Passed", "notes": 3 }),
        json!({ "testCaseId": "TC-1", "status": "Passed", "durationMs": -5 }),
        json!({ "testCaseId": "TC-1", "status": "Passed", "durationMs": 1.5 }),
        json!({ "testCaseId": "TC-1", "status": "Passed", "durationMs": "5" }),
        json!({ "testCaseId": "TC-1", "status": "Passed", "timestamp": 5 }),
        json!({ "testCaseId": "TC-1", "status": "Passed", "timestamp": "" }),
        json!({ "testCaseId": "TC-1", "status": "Passed" , "status2": "x"}),
    ] {
        let error = service
            .record_run_result("nightly.json", &body)
            .expect_err("malformed field");
        assert!(
            matches!(error, DomainError::InvalidRequest { .. }),
            "{body} must be a 400, got {error:?}"
        );
    }
}

#[test]
fn a_re_recorded_result_keeps_what_the_request_leaves_out() {
    let (service, _directory) = service();
    let home = project(&service);
    run_in(&service, &home, "nightly");

    service
        .record_run_result(
            "nightly.json",
            &json!({
                "testCaseId": "TC-1",
                "status": "Failed",
                "notes": "flaky on CI",
                "durationMs": 1200
            }),
        )
        .expect("record");

    service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Passed" }),
        )
        .expect("update");

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    let result = &run["results"][0];
    assert_eq!(result["status"], "Passed");
    assert_eq!(result["notes"], "flaky on CI");
    assert_eq!(result["durationMs"], 1200);

    service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Passed", "notes": null, "durationMs": null }),
        )
        .expect("clear");

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    assert_eq!(run["results"][0].get("notes"), None);
    assert_eq!(run["results"][0].get("durationMs"), None);
}

#[test]
fn a_result_is_replaced_by_the_case_the_route_addresses() {
    let (service, _directory) = service();
    let home = project(&service);
    run_in(&service, &home, "nightly");

    // The path names the case, so a body that describes only what changed is
    // complete, and it rewrites the stored result rather than adding one.
    service
        .replace_run_result(
            "nightly.json",
            "TC-1",
            &json!({ "status": "Failed", "timestamp": "2", "notes": "flaky on CI" }),
        )
        .expect("replace");

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    let results = run["results"].as_array().expect("results");
    assert_eq!(results.len(), 1, "a replacement rewrites in place: {run}");
    assert_eq!(results[0]["status"], "Failed");
    assert_eq!(results[0]["timestamp"], "2");
    assert_eq!(results[0]["notes"], "flaky on CI");

    // A body may repeat the case as long as it agrees with the path, and an
    // explicit null clears a field it no longer describes.
    service
        .replace_run_result(
            "nightly.json",
            "TC-1",
            &json!({ "testCaseId": "TC-1", "status": "Passed", "timestamp": "3", "notes": null }),
        )
        .expect("replace again");

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    assert_eq!(run["results"][0]["status"], "Passed");
    assert_eq!(run["results"][0]["timestamp"], "3");
    assert_eq!(run["results"][0].get("notes"), None);

    // A body that points the replacement elsewhere is refused rather than
    // retargeted, and nothing is written.
    let error = service
        .replace_run_result(
            "nightly.json",
            "TC-1",
            &json!({ "testCaseId": "TC-2", "status": "Passed" }),
        )
        .expect_err("a body identifier that disagrees with the path");
    assert!(
        matches!(error, DomainError::InvalidRequest { ref message, .. }
            if message == "Field `testCaseId` must match the case in the path"),
        "{error:?}"
    );
    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    assert_eq!(run["results"][0]["status"], "Passed", "nothing was written");
}

#[test]
fn replacing_or_removing_an_absent_result_is_not_found() {
    let (service, _directory) = service();
    let home = project(&service);
    run_in(&service, &home, "nightly");

    // A removal takes the result out of the run...
    service
        .delete_run_result("nightly.json", "TC-1")
        .expect("remove");
    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    let results = run["results"].as_array().expect("results");
    assert!(results.is_empty(), "the removed result is gone: {run}");

    // ...and the same case cannot be removed twice, nor replaced once there is
    // no result to replace: a replacement never creates.
    for outcome in [
        service.delete_run_result("nightly.json", "TC-1").err(),
        service
            .replace_run_result("nightly.json", "TC-1", &json!({ "status": "Passed" }))
            .err(),
    ] {
        assert!(
            matches!(outcome, Some(DomainError::NotFound(ref message))
                if message == "Test result not found in test run"),
            "{outcome:?}"
        );
    }

    // A run that records no results at all answers the same way, and a run that
    // does not exist is not found as a run.
    service
        .create_in(
            Resource::Runs,
            &home,
            &json!({ "name": "empty", "timestamp": "1" }),
        )
        .expect("empty run");
    for outcome in [
        service.delete_run_result("empty.json", "TC-1").err(),
        service
            .replace_run_result("empty.json", "TC-1", &json!({ "status": "Passed" }))
            .err(),
    ] {
        assert!(
            matches!(outcome, Some(DomainError::NotFound(ref message))
                if message == "Test result not found in test run"),
            "{outcome:?}"
        );
    }

    let error = service
        .delete_run_result("missing.json", "TC-1")
        .expect_err("unknown run");
    assert!(matches!(error, DomainError::NotFound(message) if message == "Test run not found"));
}

#[test]
fn a_run_embeds_a_snapshot_of_the_case_it_selected() {
    let (service, _directory) = service();
    let project = project(&service);
    case(&service, &project, "TC-1");
    service
        .create_in(
            Resource::Runs,
            &project,
            &json!({ "testRunId": "R-1", "name": "nightly", "timestamp": "1" }),
        )
        .expect("run");

    service
        .add_case_to_run("nightly.json", &json!({ "testCaseId": "TC-1" }))
        .expect("add");

    // Editing the source afterwards must not rewrite the recorded run.
    service
        .update(
            Resource::Cases,
            "TC-1",
            &json!({
                "testCaseId": "TC-1",
                "title": "changed",
                "expectedResult": "changed"
            }),
        )
        .expect("update");

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    assert_eq!(run["testCases"][0]["title"], "T");
}

#[test]
fn milestone_progress_reads_the_referenced_runs() {
    let (service, _directory) = service();
    let home = project(&service);
    service
        .create_in(
            Resource::Runs,
            &home,
            &json!({
                "testRunId": "R-1",
                "name": "nightly",
                "timestamp": "1",
                "results": [
                    { "testCaseId": "TC-1", "status": "Passed", "timestamp": "1" },
                    { "testCaseId": "TC-2", "status": "Failed", "timestamp": "1" },
                    { "testCaseId": "TC-3", "status": "Blocked", "timestamp": "1" }
                ]
            }),
        )
        .expect("run");
    service
        .create_in(
            Resource::Milestones,
            &home,
            &json!({ "milestoneId": "M-1", "name": "v1.0", "testRunIds": ["nightly.json"] }),
        )
        .expect("milestone");

    let progress = service.milestone_progress("M-1.json").expect("progress");
    assert_eq!(progress.milestone_id, "M-1");
    assert_eq!(progress.total_cases, 3);
    assert_eq!(progress.passed, 1);
    assert_eq!(progress.pass_percentage, 33.33333333333333);
}

#[test]
fn milestone_progress_counts_every_case_a_run_holds_once() {
    let (service, _directory) = service();
    let home = project(&service);
    let suite = add_suite(&service, &home);
    for id in ["TC-1", "TC-2", "TC-3"] {
        case(&service, &suite, id);
    }

    service
        .create_in(
            Resource::Runs,
            &home,
            &json!({
                "testRunId": "R-1",
                "name": "nightly",
                "timestamp": "1",
                "testCases": [{ "testCaseId": "TC-1", "title": "T", "expectedResult": "E" }]
            }),
        )
        .expect("run");
    service
        .add_suite_to_run("nightly.json", &json!({ "suiteId": "smoke.json" }))
        .expect("link suite");
    for (id, status) in [("TC-2", "Passed"), ("TC-3", "Failed")] {
        service
            .record_run_result(
                "nightly.json",
                &json!({ "testCaseId": id, "status": status, "timestamp": "1" }),
            )
            .expect("record");
    }
    service
        .create_in(
            Resource::Milestones,
            &home,
            &json!({ "milestoneId": "M-1", "name": "v1.0", "testRunIds": ["nightly.json"] }),
        )
        .expect("milestone");

    // The declared case, the three the suite embeds and the two recorded
    // outcomes describe three distinct cases, not six.
    let progress = service.milestone_progress("M-1.json").expect("progress");
    assert_eq!(progress.total_cases, 3);
    assert_eq!(progress.passed, 1);
    assert_eq!(progress.failed, 1);
    assert_eq!(progress.blocked, 0);
    assert_eq!(progress.untested, 1);
    assert_eq!(progress.retest, 0);
    assert_eq!(
        progress.passed + progress.failed + progress.blocked + progress.untested + progress.retest,
        progress.total_cases
    );
    assert_eq!(progress.pass_percentage, 33.33333333333333);
}

#[test]
fn progress_for_a_missing_milestone_is_not_found() {
    let (service, _directory) = service();
    let error = service
        .milestone_progress("missing.json")
        .expect_err("missing");
    assert!(matches!(error, DomainError::NotFound(message) if message == "Milestone not found"));
}

#[test]
fn attachments_are_recorded_in_the_case_and_round_trip() {
    let (service, _directory) = service();
    let project = project(&service);
    case(&service, &project, "TC-1");

    let parent = service.require_test_case("TC-1").expect("parent");
    let stored = service
        .store_attachment(&parent, "TC-1", "notes.txt", b"evidence")
        .expect("store");
    assert_eq!(stored.original_name, "notes.txt");
    assert_eq!(stored.size, 8);
    assert!(stored.filename.ends_with("-notes.txt"));

    assert_eq!(
        service
            .read_attachment(&parent, "TC-1", &stored.filename)
            .expect("read"),
        b"evidence"
    );

    let document = service.get(Resource::Cases, "TC-1").expect("case");
    assert_eq!(
        document["attachments"][0]["filename"],
        json!(stored.filename)
    );
    assert_eq!(document["attachments"][0]["originalName"], "notes.txt");
    assert_eq!(document["attachments"][0]["mimeType"], "text/plain");

    service
        .delete_attachment(&parent, "TC-1", &stored.filename)
        .expect("delete");
    assert!(
        service
            .read_attachment(&parent, "TC-1", &stored.filename)
            .is_err()
    );
    let document = service.get(Resource::Cases, "TC-1").expect("case");
    assert!(document.get("attachments").is_none());
}

#[test]
fn requiring_a_missing_test_case_is_not_found() {
    let (service, _directory) = service();
    assert!(matches!(
        service.require_test_case("TC-1").expect_err("missing"),
        DomainError::NotFound(_)
    ));
}

// --- project homes -------------------------------------------------

/// Creates a project named `name` beside `checkout` and returns its parent.
fn another_project(service: &TestService<FileRepository>, name: &str) -> Parent {
    service
        .create(Resource::Projects, &json!({ "name": name }))
        .expect("project");
    Parent::Project(format!("{name}.json"))
}

/// Stores a run called `name` in `home`, recording one passed result.
fn run_in(service: &TestService<FileRepository>, home: &Parent, name: &str) {
    service
        .create_in(
            Resource::Runs,
            home,
            &json!({
                "name": name,
                "timestamp": "1",
                "results": [{ "testCaseId": "TC-1", "status": "Passed", "timestamp": "1" }]
            }),
        )
        .expect("run");
}

#[test]
fn a_home_preferred_identifier_resolves_the_occurrence_it_names() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    run_in(&service, &checkout, "nightly");
    run_in(&service, &billing, "nightly");

    // A bare identifier resolves globally, so two occurrences are refused.
    let error = service
        .resolve(Resource::Runs, "nightly.json", None, "Test run not found")
        .expect_err("two homes");
    assert_eq!(
        error.to_string(),
        ambiguous(Resource::Runs, &[checkout.clone(), billing.clone()]).to_string(),
        "the global rule must not pick one of the two"
    );

    // Either home says which occurrence is meant.
    assert_eq!(
        service
            .resolve(
                Resource::Runs,
                "nightly.json",
                Some(&billing),
                "Test run not found"
            )
            .expect("billing's run"),
        billing
    );
    assert_eq!(
        service
            .resolve(
                Resource::Runs,
                "nightly.json",
                Some(&checkout),
                "Test run not found"
            )
            .expect("checkout's run"),
        checkout
    );

    // A home that does not hold the identifier falls back to the global
    // rule, so a unique identifier still resolves from an unrelated home.
    run_in(&service, &checkout, "weekly");
    assert_eq!(
        service
            .resolve(
                Resource::Runs,
                "weekly.json",
                Some(&billing),
                "Test run not found"
            )
            .expect("one home only"),
        checkout
    );
}

#[test]
fn an_ambiguous_identifier_names_both_homes() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    run_in(&service, &checkout, "nightly");
    run_in(&service, &billing, "nightly");

    let error = service
        .get(Resource::Runs, "nightly.json")
        .expect_err("ambiguous");
    match error {
        DomainError::Conflict(message) => {
            assert!(message.contains("2 parents"), "{message}");
            assert!(message.contains("billing.json"), "{message}");
            assert!(message.contains("checkout.json"), "{message}");
            assert!(
                message.contains("POST /projects/{id}/test_runs"),
                "the conflict must name the parent-scoped route: {message}"
            );
        }
        other => panic!("expected a conflict, got {other:?}"),
    }

    // A write is refused too, rather than landing in an arbitrary home.
    assert!(matches!(
        service
            .record_run_result(
                "nightly.json",
                &json!({ "testCaseId": "TC-9", "status": "Passed" })
            )
            .expect_err("ambiguous"),
        DomainError::Conflict(_)
    ));
}

#[test]
fn the_conflict_names_the_parent_scoped_routes_of_its_own_resource() {
    let homes = [
        Parent::Project("billing.json".to_owned()),
        Parent::Project("checkout.json".to_owned()),
    ];
    for (resource, endpoint) in [
        (
            Resource::Runs,
            "POST /projects/{id}/test_runs, or the matching /{run_id} delete",
        ),
        (
            Resource::Milestones,
            "POST /projects/{id}/milestones, or the matching /{milestone_id} delete",
        ),
        (
            Resource::Configurations,
            "POST /projects/{id}/configurations, or the matching /{config_id} delete",
        ),
        (
            Resource::Suites,
            "POST /projects/{id}/test_suites, or the matching /{suite_id} delete",
        ),
        (
            Resource::Cases,
            "POST /projects/{id}/test_cases, POST /test_suites/{id}/test_cases, the matching /{case_id} delete, or the parent-scoped attachment routes /projects/{id}/test_cases/{case_id}/attachments and /test_suites/{id}/test_cases/{case_id}/attachments",
        ),
    ] {
        let message = ambiguous(resource, &homes).to_string();
        assert!(message.contains(endpoint), "{resource:?}: {message}");
        assert!(
            message.contains("2 parents (project billing.json, project checkout.json)"),
            "{resource:?}: {message}"
        );
    }
}

#[test]
fn the_conflict_labels_a_suite_home_so_the_list_counts() {
    let homes = [
        Parent::Project("billing.json".to_owned()),
        Parent::Suite {
            project: "payments.json".to_owned(),
            suite: "smoke.checkout.json".to_owned(),
        },
    ];

    let message = ambiguous(Resource::Cases, &homes).to_string();
    assert!(
        message.contains(
            "2 parents (project billing.json, suite smoke.checkout.json in project payments.json)"
        ),
        "{message}"
    );
}

#[test]
fn the_retired_flat_creates_name_their_replacement() {
    let (service, _directory) = service();
    project(&service);

    for (resource, body, message) in [
        (
            Resource::Runs,
            json!({ "name": "nightly", "timestamp": "1" }),
            "Test runs are created inside a project: POST /projects/{id}/test_runs",
        ),
        (
            Resource::Milestones,
            json!({ "name": "v1.0" }),
            "Milestones are created inside a project: POST /projects/{id}/milestones",
        ),
        (
            Resource::Configurations,
            json!({ "name": "chrome" }),
            "Configurations are created inside a project: POST /projects/{id}/configurations",
        ),
    ] {
        let error = service.create(resource, &body).expect_err("retired route");
        match error {
            DomainError::InvalidRequest { code, message: got } => {
                assert_eq!(code, "invalid_request", "{resource:?}");
                assert_eq!(got, message, "{resource:?}");
            }
            other => panic!("{resource:?} produced {other:?}"),
        }
        assert!(
            list(&service, resource).is_empty(),
            "{resource:?}: a retired route must store nothing"
        );
    }
}

#[test]
fn an_identifier_no_project_holds_is_not_found() {
    let (service, _directory) = service();
    project(&service);

    for resource in [
        Resource::Runs,
        Resource::Milestones,
        Resource::Configurations,
    ] {
        let error = service
            .resolve(
                resource,
                "missing.json",
                None,
                entity_missing_message(resource),
            )
            .expect_err("nothing holds it");
        assert!(
            matches!(&error, DomainError::NotFound(message)
                    if message == entity_missing_message(resource)),
            "{resource:?} produced {error:?}"
        );
    }

    assert!(matches!(
        service
            .milestone_progress("missing.json")
            .expect_err("missing"),
        DomainError::NotFound(message) if message == "Milestone not found"
    ));
}

#[test]
fn duplicating_a_run_stores_the_copy_in_its_own_home() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    run_in(&service, &billing, "nightly");

    let new_id = service
        .duplicate(&duplicate::RUN, "nightly.json", &json!({}))
        .expect("duplicate");

    let copies = service
        .list_children(&billing, Resource::Runs)
        .expect("billing's runs");
    assert_eq!(copies.len(), 2, "{copies:?}");
    assert!(copies.contains(&new_id), "{copies:?}");
    assert!(
        service
            .list_children(&checkout, Resource::Runs)
            .expect("checkout's runs")
            .is_empty(),
        "a duplicate must not move or copy into another project"
    );
}

#[test]
fn the_summary_report_counts_two_runs_sharing_an_identifier() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    run_in(&service, &checkout, "nightly");
    run_in(&service, &billing, "nightly");

    // A global listing de-duplicates to one `nightly.json`; the report
    // walks projects instead, so both runs contribute their result.
    assert_eq!(
        list(&service, Resource::Runs),
        vec!["nightly.json".to_owned()],
        "the listing still de-duplicates"
    );

    let report = service
        .summary_report(&reports::SummaryFilters::default(), None)
        .expect("report");
    assert_eq!(report.total, 2, "one result per project's run");
    assert_eq!(report.passed, 2);
}

#[test]
fn milestone_progress_prefers_its_own_home_for_a_shared_identifier() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    // The milestone's home holds no run of this identifier, and two other
    // projects do, so the reference cannot be resolved.
    run_in(&service, &checkout, "nightly");
    run_in(&service, &billing, "nightly");
    let platform = another_project(&service, "platform");
    service
        .create_in(
            Resource::Milestones,
            &platform,
            &json!({ "name": "v1.0", "testRunIds": ["nightly.json"] }),
        )
        .expect("milestone");

    let error = service
        .milestone_progress("v1.0.json")
        .expect_err("two runs answer to the reference");
    assert!(
        matches!(&error, DomainError::Conflict(_)),
        "an arbitrary pick would report a wrong number, got {error:?}"
    );
}

#[test]
fn milestone_progress_resolves_a_shared_identifier_from_its_home() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    run_in(&service, &checkout, "nightly");
    run_in(&service, &billing, "nightly");
    // The milestone lives in `billing`, so its reference means that run
    // even though `checkout` holds the same identifier.
    service
        .create_in(
            Resource::Milestones,
            &billing,
            &json!({ "name": "v1.0", "testRunIds": ["nightly.json"] }),
        )
        .expect("milestone");

    let progress = service.milestone_progress("v1.0.json").expect("progress");
    assert_eq!(progress.total_cases, 1);
    assert_eq!(progress.passed, 1);
}

#[test]
fn milestone_progress_skips_a_reference_no_project_holds() {
    let (service, _directory) = service();
    let checkout = project(&service);
    run_in(&service, &checkout, "nightly");
    service
        .create_in(
            Resource::Milestones,
            &checkout,
            &json!({
                "name": "v1.0",
                "testRunIds": ["nightly.json", "deleted.json"]
            }),
        )
        .expect("milestone");

    let progress = service.milestone_progress("v1.0.json").expect("progress");
    assert_eq!(
        progress.total_cases, 1,
        "progress recomputes over the runs that still exist"
    );
}

#[test]
fn a_run_links_the_configuration_its_own_home_holds() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    for home in [&checkout, &billing] {
        service
            .create_in(Resource::Configurations, home, &json!({ "name": "chrome" }))
            .expect("configuration");
    }
    run_in(&service, &billing, "nightly");

    // The identifier is ambiguous globally, but the run's home decides it.
    assert!(matches!(
        service
            .document(Resource::Configurations, "chrome.json")
            .expect_err("two homes"),
        DomainError::Conflict(_)
    ));
    service
        .link_configuration_to_run("nightly.json", &json!({ "configId": "chrome.json" }))
        .expect("the run's own configuration");

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    assert_eq!(run["configurations"][0]["configId"], "chrome.json");

    service
        .unlink_configuration_from_run("nightly.json", "chrome.json")
        .expect("unlink");
    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    assert!(run["configurations"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn a_write_goes_back_to_the_home_the_read_resolved() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    run_in(&service, &billing, "nightly");

    service
        .record_run_result(
            "nightly.json",
            &json!({ "testCaseId": "TC-1", "status": "Failed" }),
        )
        .expect("record");

    assert_eq!(
        service
            .list_children(&billing, Resource::Runs)
            .expect("billing's runs"),
        vec!["nightly.json".to_owned()],
        "the write must not create a second occurrence elsewhere"
    );
    assert!(
        service
            .list_children(&checkout, Resource::Runs)
            .expect("checkout's runs")
            .is_empty()
    );

    let run = service.get(Resource::Runs, "nightly.json").expect("run");
    let results = run["results"].as_array().expect("results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "Failed");
}

#[test]
fn a_parent_addressed_read_never_answers_a_conflict() {
    let (service, _directory) = service();
    let checkout = project(&service);
    let billing = another_project(&service, "billing");
    for (home, marker) in [(&checkout, "in-checkout"), (&billing, "in-billing")] {
        service
            .create_in(
                Resource::Runs,
                home,
                &json!({
                    "name": "nightly",
                    "testRunId": marker,
                    "timestamp": "1"
                }),
            )
            .expect("run");
    }

    // The global read cannot choose, but a named home can.
    assert!(matches!(
        service
            .document(Resource::Runs, "nightly.json")
            .expect_err("two homes"),
        DomainError::Conflict(_)
    ));
    for (home, marker) in [(&checkout, "in-checkout"), (&billing, "in-billing")] {
        let document = service
            .document_in(Resource::Runs, home, "nightly.json", "Test run not found")
            .expect("the named home's occurrence");
        assert_eq!(document["testRunId"], marker, "{home:?}");
    }

    // A home that does not hold the identifier reports the caller's own
    // missing message, and never a conflict.
    let platform = another_project(&service, "platform");
    let error = service
        .document_in(
            Resource::Runs,
            &platform,
            "nightly.json",
            "Test run not found",
        )
        .expect_err("this home does not hold it");
    assert!(
        matches!(&error, DomainError::NotFound(message) if message == "Test run not found"),
        "{error:?}"
    );

    // So does an identifier nothing holds anywhere.
    assert!(matches!(
        service
            .document_in(Resource::Runs, &billing, "ghost.json", "Test run not found")
            .expect_err("absent"),
        DomainError::NotFound(_)
    ));
}

// ---------------------------------------------------------------------------
// Unit tests for pure helper functions
// ---------------------------------------------------------------------------

// --- merged_document -------------------------------------------------------

#[test]
fn merged_document_replaces_only_supplied_fields() {
    let stored = json!({ "title": "old", "steps": "s", "priority": "high" });
    let body = json!({ "title": "new" });
    let merged = merged_document(&stored, &body).expect("merge");
    assert_eq!(merged["title"], "new");
    assert_eq!(merged["steps"], "s");
    assert_eq!(merged["priority"], "high");
}

#[test]
fn merged_document_null_keeps_stored_value() {
    let stored = json!({ "title": "old", "steps": "s" });
    let body = json!({ "title": null });
    let merged = merged_document(&stored, &body).expect("merge");
    assert_eq!(merged["title"], "old");
    assert_eq!(merged["steps"], "s");
}

#[test]
fn merged_document_non_object_stored_returns_error() {
    let stored = json!("just a string");
    let body = json!({ "title": "new" });
    assert!(matches!(
        merged_document(&stored, &body),
        Err(DomainError::Internal(_))
    ));
}

#[test]
fn merged_document_empty_body_returns_stored_unchanged() {
    let stored = json!({ "title": "old", "steps": "s" });
    let body = json!({});
    let merged = merged_document(&stored, &body).expect("merge");
    assert_eq!(merged, stored);
}

// --- case_content_changed --------------------------------------------------

#[test]
fn case_content_changed_detects_title_change() {
    let stored = json!({ "title": "a", "steps": "s" });
    let merged = json!({ "title": "b", "steps": "s" });
    assert!(case_content_changed(&stored, &merged));
}

#[test]
fn case_content_changed_detects_steps_change() {
    let stored = json!({ "title": "a", "steps": "s1" });
    let merged = json!({ "title": "a", "steps": "s2" });
    assert!(case_content_changed(&stored, &merged));
}

#[test]
fn case_content_changed_detects_preconditions_change() {
    let stored = json!({ "title": "a", "preconditions": "p1" });
    let merged = json!({ "title": "a", "preconditions": "p2" });
    assert!(case_content_changed(&stored, &merged));
}

#[test]
fn case_content_changed_detects_expected_result_change() {
    let stored = json!({ "title": "a", "expectedResult": "e1" });
    let merged = json!({ "title": "a", "expectedResult": "e2" });
    assert!(case_content_changed(&stored, &merged));
}

#[test]
fn case_content_changed_ignores_non_qualifying_fields() {
    let stored = json!({ "title": "a", "steps": "s", "tags": ["old"], "name": "x" });
    let merged = json!({ "title": "a", "steps": "s", "tags": ["new"], "name": "y" });
    assert!(!case_content_changed(&stored, &merged));
}

#[test]
fn case_content_changed_false_when_identical() {
    let doc = json!({ "title": "a", "steps": "s", "preconditions": "p", "expectedResult": "e" });
    assert!(!case_content_changed(&doc, &doc));
}

// --- stamp_case_creation ---------------------------------------------------

#[test]
fn stamp_case_creation_sets_version_and_last_modified() {
    let mut doc = json!({ "title": "new case" });
    stamp_case_creation(&mut doc);
    assert_eq!(doc["version"], 1);
    assert!(doc["lastModified"].is_string());
    assert!(!doc["lastModified"].as_str().unwrap().is_empty());
}

#[test]
fn stamp_case_creation_overwrites_client_supplied_values() {
    let mut doc = json!({ "title": "x", "version": 99, "lastModified": "client-time" });
    stamp_case_creation(&mut doc);
    assert_eq!(doc["version"], 1);
    assert_ne!(doc["lastModified"], "client-time");
}

#[test]
fn stamp_case_creation_noop_for_non_object() {
    let mut doc = json!("not an object");
    stamp_case_creation(&mut doc);
    assert_eq!(doc, json!("not an object"));
}

// --- normalise_marker ------------------------------------------------------

#[test]
fn normalise_marker_project_sets_suite_collection_and_id() {
    let mut doc = json!({ "name": "p" });
    normalise_marker(Resource::Projects, "p.json", &mut doc);
    assert_eq!(doc["projectId"], "p.json");
    assert_eq!(doc["testSuites"], json!([]));
}

#[test]
fn normalise_marker_suite_sets_case_collection_and_id() {
    let mut doc = json!({ "name": "s" });
    normalise_marker(Resource::Suites, "s.json", &mut doc);
    assert_eq!(doc["suiteId"], "s.json");
    assert_eq!(doc["testCases"], json!([]));
}

#[test]
fn normalise_marker_run_sets_id_and_timestamp() {
    let mut doc = json!({ "name": "r" });
    normalise_marker(Resource::Runs, "r.json", &mut doc);
    assert_eq!(doc["testRunId"], "r.json");
    assert!(doc["timestamp"].is_string());
    assert!(!doc["timestamp"].as_str().unwrap().is_empty());
}

#[test]
fn normalise_marker_milestone_sets_id() {
    let mut doc = json!({ "name": "m" });
    normalise_marker(Resource::Milestones, "m.json", &mut doc);
    assert_eq!(doc["milestoneId"], "m.json");
}

#[test]
fn normalise_marker_configuration_sets_id() {
    let mut doc = json!({ "name": "c" });
    normalise_marker(Resource::Configurations, "c.json", &mut doc);
    assert_eq!(doc["configId"], "c.json");
}

#[test]
fn normalise_marker_configuration_takes_the_id_over_a_supplied_identity() {
    let mut doc = json!({ "name": "c", "configId": "somewhere/else" });
    normalise_marker(Resource::Configurations, "c.json", &mut doc);
    assert_eq!(doc["configId"], "c.json");
}

#[test]
fn normalise_marker_case_is_identity() {
    let mut doc = json!({ "testCaseId": "C-1", "title": "t" });
    let original = doc.clone();
    normalise_marker(Resource::Cases, "C-1", &mut doc);
    assert_eq!(doc, original);
}

#[test]
fn normalise_marker_does_not_overwrite_client_identity() {
    let mut doc = json!({ "projectId": "client-id", "name": "p" });
    normalise_marker(Resource::Projects, "p.json", &mut doc);
    assert_eq!(doc["projectId"], "client-id");
}

// --- Composed::message -----------------------------------------------------

#[test]
fn composed_message_created() {
    let composed = Composed::Created(Created {
        id: "x.json".to_owned(),
    });
    assert_eq!(composed.message("Test suite"), "Test suite created");
}

#[test]
fn composed_message_copied() {
    let composed = Composed::Placed {
        id: "x.json".to_owned(),
        mode: Placement::Copy,
    };
    assert_eq!(composed.message("Test case"), "Test case copied");
}

#[test]
fn composed_message_moved() {
    let composed = Composed::Placed {
        id: "x.json".to_owned(),
        mode: Placement::Move,
    };
    assert_eq!(composed.message("Test suite"), "Test suite moved");
}

// --- Composed::id ----------------------------------------------------------

#[test]
fn composed_id_created() {
    let composed = Composed::Created(Created {
        id: "new.json".to_owned(),
    });
    assert_eq!(composed.id(), "new.json");
}

#[test]
fn composed_id_placed() {
    let composed = Composed::Placed {
        id: "placed.json".to_owned(),
        mode: Placement::Copy,
    };
    assert_eq!(composed.id(), "placed.json");
}
