use super::*;
use serde_json::json;
use tempfile::TempDir;

fn repository() -> (TempDir, FileRepository) {
    let directory = TempDir::new().expect("temp dir");
    let repository = FileRepository::new(directory.path()).expect("repository");
    (directory, repository)
}

fn project(id: &str) -> Parent {
    Parent::Project(id.to_owned())
}

fn suite(project_id: &str, suite_id: &str) -> Parent {
    Parent::Suite {
        project: project_id.to_owned(),
        suite: suite_id.to_owned(),
    }
}

fn create_project(repository: &FileRepository, id: &str) {
    repository
        .write_at(Resource::Projects, None, id, &json!({"name": id}))
        .expect("project");
}

#[test]
fn new_creates_only_the_root_collections() {
    let (directory, _repository) = repository();
    for resource in Resource::ROOT_DIRS {
        let name = resource.dir_name().expect("root dir");
        assert!(directory.path().join(name).is_dir(), "{name}");
    }
    assert!(!directory.path().join("test_suites").exists());
    assert!(!directory.path().join("test_cases").exists());
    for name in RESERVED_PROJECT_CHILDREN {
        assert!(
            !directory.path().join(name).exists(),
            "{name} is a project collection, not a root collection"
        );
    }
}

#[test]
fn a_fresh_root_holds_only_projects() {
    let (directory, _repository) = repository();

    let mut created = fs::read_dir(directory.path())
        .expect("root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    created.sort();
    assert_eq!(created, vec!["projects".to_owned()]);
}

#[test]
fn empty_legacy_collections_do_not_refuse_the_root() {
    let directory = TempDir::new().expect("temp dir");
    for name in RESERVED_PROJECT_CHILDREN {
        fs::create_dir_all(directory.path().join(name)).expect("legacy collection");
    }

    FileRepository::new(directory.path()).expect("an empty legacy collection is not an error");
}

#[test]
fn a_legacy_document_refuses_the_root_and_names_only_its_collection() {
    let directory = TempDir::new().expect("temp dir");
    let legacy = directory.path().join("test_runs");
    fs::create_dir_all(&legacy).expect("legacy collection");
    fs::write(
        legacy.join("nightly.json"),
        b"{\"testRunId\": \"nightly.json\"}\n",
    )
    .expect("legacy document");

    let error = FileRepository::new(directory.path())
        .err()
        .expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let message = error.to_string();
    assert_eq!(
        message,
        "legacy flat storage layout detected: test_runs/ holds 1 document(s); layout v3 \
         stores runs, milestones and configurations inside their project folder — move each \
         document into projects/<project>/<collection>/ and restart \
         (docs/deployment/deployment-guide.md)"
    );
    assert!(!message.contains("milestones/"), "{message}");
    assert!(!message.contains("configurations/"), "{message}");
}

#[test]
fn a_legacy_document_in_two_collections_names_both_in_layout_order() {
    let directory = TempDir::new().expect("temp dir");
    fs::create_dir_all(directory.path().join("test_runs")).expect("legacy collection");
    fs::create_dir_all(directory.path().join("milestones")).expect("legacy collection");
    fs::write(
        directory.path().join("test_runs/nightly.json"),
        b"{\"testRunId\": \"nightly.json\"}\n",
    )
    .expect("legacy document");
    fs::write(
        directory.path().join("test_runs/weekly.json"),
        b"{\"testRunId\": \"weekly.json\"}\n",
    )
    .expect("legacy document");
    fs::write(
        directory.path().join("milestones/v1.0.json"),
        b"{\"milestoneId\": \"v1.0.json\"}\n",
    )
    .expect("legacy document");

    let error = FileRepository::new(directory.path())
        .err()
        .expect("refused");
    let message = error.to_string();
    assert!(
        message.contains("test_runs/ holds 2 document(s), milestones/ holds 1 document(s)"),
        "{message}"
    );
    assert!(!message.contains("configurations/"), "{message}");
}

#[test]
fn a_refused_root_leaves_the_legacy_document_untouched() {
    let directory = TempDir::new().expect("temp dir");
    let document = directory.path().join("test_runs/nightly.json");
    let contents = b"{\n  \"testRunId\": \"nightly.json\"\n}\n";
    fs::create_dir_all(document.parent().expect("parent")).expect("legacy collection");
    fs::write(&document, contents).expect("legacy document");

    FileRepository::new(directory.path())
        .err()
        .expect("refused");

    assert_eq!(
        fs::read(&document).expect("still on disk"),
        contents,
        "the refusal is read-only"
    );
}

#[test]
fn a_legacy_temporary_file_does_not_refuse_the_root() {
    let directory = TempDir::new().expect("temp dir");
    fs::create_dir_all(directory.path().join("test_runs")).expect("legacy collection");
    fs::write(
        directory
            .path()
            .join("test_runs/.tucano-1700000000000000000.tmp"),
        b"{\"testRunId\": \"half-written.json\"}\n",
    )
    .expect("temporary file");

    FileRepository::new(directory.path())
        .expect("an atomic-write temporary file is not a document");
}

#[test]
fn a_subdirectory_in_a_legacy_collection_does_not_refuse_the_root() {
    let directory = TempDir::new().expect("temp dir");
    let nested = directory.path().join("test_runs/nested");
    fs::create_dir_all(&nested).expect("nested directory");
    fs::write(
        nested.join("nightly.json"),
        b"{\"testRunId\": \"nightly.json\"}\n",
    )
    .expect("nested document");

    FileRepository::new(directory.path()).expect("a directory is not a document");
}

#[test]
fn a_project_is_a_folder_holding_a_marker() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");

    assert!(
        directory
            .path()
            .join("projects/checkout/project.json")
            .is_file()
    );
    assert_eq!(
        repository
            .read_at(Resource::Projects, None, "checkout.json")
            .expect("read")["name"],
        "checkout.json"
    );
    assert_eq!(
        repository.list(Resource::Projects).expect("list"),
        vec!["checkout.json".to_owned()]
    );
}

#[test]
fn suites_and_cases_live_inside_their_parents() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&project("checkout.json")),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");
    repository
        .write_at(
            Resource::Cases,
            Some(&project("checkout.json")),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("direct case");
    repository
        .write_at(
            Resource::Cases,
            Some(&suite("checkout.json", "smoke.json")),
            "TC-002",
            &json!({"testCaseId": "TC-002"}),
        )
        .expect("suite case");

    assert!(
        directory
            .path()
            .join("projects/checkout/smoke/suite.json")
            .is_file()
    );
    assert!(
        directory
            .path()
            .join("projects/checkout/TC-001/test-case.json")
            .is_file()
    );
    assert!(
        directory
            .path()
            .join("projects/checkout/smoke/TC-002/test-case.json")
            .is_file()
    );

    assert_eq!(
        repository.list(Resource::Suites).expect("suites"),
        vec!["smoke.json".to_owned()]
    );
    assert_eq!(
        repository.list(Resource::Cases).expect("cases"),
        vec!["TC-001".to_owned(), "TC-002".to_owned()]
    );
}

#[test]
fn list_de_duplicates_the_same_identifier_in_several_homes() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    create_project(&repository, "billing.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&project("checkout.json")),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");
    repository
        .write_at(
            Resource::Suites,
            Some(&project("billing.json")),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");

    assert_eq!(
        repository.list(Resource::Suites).expect("suites"),
        vec!["smoke.json".to_owned()],
        "a list never repeats an identifier"
    );
}

#[test]
fn locate_reports_every_home_of_an_identifier() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&project("checkout.json")),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");
    repository
        .write_at(
            Resource::Cases,
            Some(&project("checkout.json")),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("direct case");
    repository
        .write_at(
            Resource::Cases,
            Some(&suite("checkout.json", "smoke.json")),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("suite case");

    assert_eq!(
        repository.locate(Resource::Cases, "TC-001").expect("homes"),
        vec![
            project("checkout.json"),
            suite("checkout.json", "smoke.json"),
        ]
    );
    assert_eq!(
        repository
            .locate(Resource::Suites, "smoke.json")
            .expect("homes"),
        vec![project("checkout.json")]
    );
    assert!(
        repository
            .locate(Resource::Cases, "unknown")
            .expect("homes")
            .is_empty()
    );
}

#[test]
fn list_children_reports_only_the_parents_own_children() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    create_project(&repository, "billing.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&project("checkout.json")),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");
    repository
        .write_at(
            Resource::Cases,
            Some(&project("checkout.json")),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("case");

    assert_eq!(
        repository
            .list_children(&project("checkout.json"), Resource::Suites)
            .expect("suites"),
        vec!["smoke.json".to_owned()]
    );
    assert_eq!(
        repository
            .list_children(&project("checkout.json"), Resource::Cases)
            .expect("cases"),
        vec!["TC-001".to_owned()]
    );
    assert!(
        repository
            .list_children(&project("billing.json"), Resource::Cases)
            .expect("cases")
            .is_empty()
    );
    assert!(
        repository
            .list_children(&project("checkout.json"), Resource::Runs)
            .expect("runs")
            .is_empty(),
        "a project that holds no run lists none, and its collection folder is not created"
    );
    assert!(
        !directory
            .path()
            .join("projects/checkout/test_runs")
            .exists()
    );
}

#[test]
fn project_scoped_documents_live_in_their_project_collection() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");

    for (resource, id) in [
        (Resource::Runs, "nightly.json"),
        (Resource::Milestones, "v1.0.json"),
        (Resource::Configurations, "chrome.json"),
    ] {
        repository
            .write_at(resource, Some(&home), id, &json!({ "name": id }))
            .expect("write");
        assert_eq!(
            repository.read_at(resource, Some(&home), id).expect("read")["name"],
            json!(id),
            "{resource:?}"
        );
        assert!(
            repository
                .exists_at(resource, Some(&home), id)
                .expect("exists"),
            "{resource:?}"
        );
        assert_eq!(
            repository.list_children(&home, resource).expect("children"),
            vec![id.to_owned()],
            "{resource:?}"
        );
        assert_eq!(
            repository.list(resource).expect("list"),
            vec![id.to_owned()],
            "{resource:?}"
        );
    }

    assert!(
        directory
            .path()
            .join("projects/checkout/test_runs/nightly.json")
            .is_file()
    );
    assert!(
        directory
            .path()
            .join("projects/checkout/milestones/v1.0.json")
            .is_file()
    );
    assert!(
        directory
            .path()
            .join("projects/checkout/configurations/chrome.json")
            .is_file()
    );
}

#[test]
fn a_project_scoped_document_requires_a_project_parent() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&home),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");
    let source = suite("checkout.json", "smoke.json");

    for resource in [
        Resource::Runs,
        Resource::Milestones,
        Resource::Configurations,
    ] {
        assert!(
            repository.read_at(resource, None, "nightly.json").is_err(),
            "{resource:?} is never addressed without a parent"
        );
        for parent in [Some(&source), None] {
            let error = repository
                .write_at(resource, parent, "nightly.json", &json!({}))
                .expect_err("a suite is not a home");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
        }
        let error = repository
            .list_children(&source, resource)
            .expect_err("a suite owns no collection");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
        let error = repository
            .locate(resource, "nightly")
            .expect_err("identifier without the suffix");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
    }
}

#[test]
fn locate_reports_every_project_holding_the_same_identifier() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    create_project(&repository, "billing.json");
    for parent in [project("checkout.json"), project("billing.json")] {
        repository
            .write_at(
                Resource::Runs,
                Some(&parent),
                "nightly.json",
                &json!({"name": "nightly"}),
            )
            .expect("run");
    }

    assert_eq!(
        repository
            .locate(Resource::Runs, "nightly.json")
            .expect("homes"),
        vec![project("billing.json"), project("checkout.json")],
        "homes come back in the stable order the project listing has"
    );
    assert_eq!(
        repository.list(Resource::Runs).expect("runs"),
        vec!["nightly.json".to_owned()],
        "a global listing de-duplicates, exactly as it does for suites"
    );
    assert_eq!(
        repository
            .list_children(&project("checkout.json"), Resource::Runs)
            .expect("runs"),
        vec!["nightly.json".to_owned()],
        "a parent-scoped listing names that project's own occurrence"
    );
    assert!(
        repository
            .locate(Resource::Runs, "missing.json")
            .expect("homes")
            .is_empty()
    );
}

#[test]
fn locate_refuses_an_unusable_identifier_with_no_project_to_hold_it() {
    let (_directory, repository) = repository();
    for resource in [
        Resource::Runs,
        Resource::Milestones,
        Resource::Configurations,
    ] {
        for id in [
            "nightly",
            "",
            ".",
            "..",
            "../escape.json",
            "nested/child.json",
        ] {
            let error = repository.locate(resource, id).expect_err("refused");
            assert_eq!(
                error.kind(),
                io::ErrorKind::InvalidInput,
                "{resource:?} {id:?} is refused rather than reported missing"
            );
        }
        assert!(
            repository
                .locate(resource, "nightly.json")
                .expect("a usable identifier")
                .is_empty()
        );
    }
}

#[test]
fn a_project_reserves_the_names_of_its_collections() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");

    for name in RESERVED_PROJECT_CHILDREN {
        let error = repository
            .write_at(
                Resource::Suites,
                Some(&home),
                &format!("{name}.json"),
                &json!({"name": name}),
            )
            .expect_err("a suite may not take a collection name");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{name}");

        let error = repository
            .write_at(
                Resource::Cases,
                Some(&home),
                name,
                &json!({"testCaseId": name}),
            )
            .expect_err("a case may not take a collection name");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{name}");
    }
}

#[test]
fn the_reservation_only_guards_the_project_folder() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&home),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");

    repository
        .write_at(
            Resource::Cases,
            Some(&suite("checkout.json", "smoke.json")),
            "test_runs",
            &json!({"testCaseId": "test_runs"}),
        )
        .expect("inside a suite the name is free");
    assert_eq!(
        repository
            .list_children(&suite("checkout.json", "smoke.json"), Resource::Cases)
            .expect("cases"),
        vec!["test_runs".to_owned()]
    );
}

#[test]
fn placing_a_reserved_name_into_a_project_is_refused() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    create_project(&repository, "billing.json");
    // A folder that predates the reservation, sitting where checkout's run
    // collection lives.
    let legacy = directory.path().join("projects/checkout/test_runs");
    fs::create_dir_all(&legacy).expect("folder");
    fs::write(legacy.join("suite.json"), b"{\"name\": \"legacy\"}").expect("marker");

    let error = repository
        .place(
            Resource::Suites,
            &project("checkout.json"),
            "test_runs.json",
            &project("billing.json"),
            Placement::Move,
        )
        .expect_err("reserved target name");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn a_folder_cannot_hold_two_kinds_of_child() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&project("checkout.json")),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");

    let error = repository
        .write_at(
            Resource::Cases,
            Some(&project("checkout.json")),
            "smoke",
            &json!({"testCaseId": "smoke"}),
        )
        .expect_err("collision");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn a_case_cannot_shadow_its_parent_marker() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&project("checkout.json")),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");

    let error = repository
        .write_at(
            Resource::Cases,
            Some(&project("checkout.json")),
            "project.json",
            &json!({"testCaseId": "project.json"}),
        )
        .expect_err("shadows the project marker");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);

    let error = repository
        .write_at(
            Resource::Cases,
            Some(&suite("checkout.json", "smoke.json")),
            "suite.json",
            &json!({"testCaseId": "suite.json"}),
        )
        .expect_err("shadows the suite marker");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn place_copies_a_subtree_without_touching_the_source() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    create_project(&repository, "billing.json");
    let home = project("checkout.json");
    let source = suite("checkout.json", "smoke.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&home),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");
    repository
        .write_at(
            Resource::Cases,
            Some(&source),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("case");
    repository
        .save_attachment(
            &source,
            "TC-001",
            "notes.txt",
            &json!({"filename": "notes.txt"}),
            b"evidence",
        )
        .expect("attachment");

    repository
        .place(
            Resource::Suites,
            &home,
            "smoke.json",
            &project("billing.json"),
            Placement::Copy,
        )
        .expect("copy");

    let copy = suite("billing.json", "smoke.json");
    assert_eq!(
        repository
            .read_attachment(&copy, "TC-001", "notes.txt")
            .expect("attachment copied"),
        b"evidence"
    );
    assert_eq!(
        repository
            .locate(Resource::Suites, "smoke.json")
            .expect("homes"),
        vec![project("billing.json"), project("checkout.json")]
    );

    repository
        .delete_at(
            Resource::Suites,
            Some(&project("billing.json")),
            "smoke.json",
        )
        .expect("delete the copy");
    assert_eq!(
        repository
            .read_attachment(&source, "TC-001", "notes.txt")
            .expect("source survives"),
        b"evidence"
    );
}

#[test]
fn place_moves_a_subtree_away_from_its_old_parent() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    create_project(&repository, "billing.json");
    let source = project("checkout.json");
    repository
        .write_at(
            Resource::Cases,
            Some(&source),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("case");

    repository
        .place(
            Resource::Cases,
            &source,
            "TC-001",
            &project("billing.json"),
            Placement::Move,
        )
        .expect("move");

    assert_eq!(
        repository.locate(Resource::Cases, "TC-001").expect("homes"),
        vec![project("billing.json")]
    );
    assert!(
        !repository
            .list_children(&project("checkout.json"), Resource::Cases)
            .expect("cases")
            .contains(&"TC-001".to_owned())
    );
}

#[test]
fn place_onto_an_existing_child_is_rejected() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    create_project(&repository, "billing.json");
    for parent in [project("checkout.json"), project("billing.json")] {
        repository
            .write_at(
                Resource::Cases,
                Some(&parent),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");
    }

    let error = repository
        .place(
            Resource::Cases,
            &project("checkout.json"),
            "TC-001",
            &project("billing.json"),
            Placement::Copy,
        )
        .expect_err("target is taken");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn delete_cascades_through_the_subtree() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let project_id = project("checkout.json");
    let suite_id = suite("checkout.json", "smoke.json");
    repository
        .write_at(
            Resource::Suites,
            Some(&project_id),
            "smoke.json",
            &json!({"name": "smoke"}),
        )
        .expect("suite");
    repository
        .write_at(
            Resource::Cases,
            Some(&suite_id),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("case");
    repository
        .save_attachment(
            &suite_id,
            "TC-001",
            "notes.txt",
            &json!({"filename": "notes.txt"}),
            b"evidence",
        )
        .expect("attachment");
    repository
        .write_at(
            Resource::Runs,
            Some(&project_id),
            "nightly.json",
            &json!({"name": "nightly"}),
        )
        .expect("run");

    repository
        .delete_at(Resource::Projects, None, "checkout.json")
        .expect("delete project");

    assert!(!directory.path().join("projects/checkout").exists());
    assert!(repository.list(Resource::Cases).expect("cases").is_empty());
    assert!(
        repository.list(Resource::Runs).expect("runs").is_empty(),
        "a project's runs go with it"
    );
}

#[test]
fn attachments_round_trip_and_update_the_case_document() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let parent = project("checkout.json");
    repository
        .write_at(
            Resource::Cases,
            Some(&parent),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("case");

    let entry = json!({"filename": "1-notes.txt", "originalName": "notes.txt"});
    repository
        .save_attachment(&parent, "TC-001", "1-notes.txt", &entry, b"evidence")
        .expect("save");

    assert_eq!(
        repository
            .read_attachment(&parent, "TC-001", "1-notes.txt")
            .expect("read"),
        b"evidence"
    );
    let stored = repository
        .read_at(Resource::Cases, Some(&parent), "TC-001")
        .expect("case document");
    assert_eq!(stored["attachments"][0], entry);

    repository
        .delete_attachment(&parent, "TC-001", "1-notes.txt")
        .expect("delete");
    assert!(
        repository
            .read_attachment(&parent, "TC-001", "1-notes.txt")
            .is_err()
    );
    let stored = repository
        .read_at(Resource::Cases, Some(&parent), "TC-001")
        .expect("case document");
    assert!(stored.get("attachments").is_none());
}

#[test]
fn attachments_reject_traversal_and_duplicate_names() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let parent = project("checkout.json");
    repository
        .write_at(
            Resource::Cases,
            Some(&parent),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("case");

    let entry = json!({"filename": "notes.txt"});
    assert!(
        repository
            .save_attachment(&parent, "TC-001", "../escape.txt", &entry, b"x")
            .is_err()
    );
    assert!(
        repository
            .read_attachment(&parent, "TC-001", "../../etc/passwd")
            .is_err()
    );

    repository
        .save_attachment(&parent, "TC-001", "notes.txt", &entry, b"first")
        .expect("first");
    assert!(
        repository
            .save_attachment(&parent, "TC-001", "notes.txt", &entry, b"second")
            .is_err(),
        "a second file with the same name must be rejected"
    );
}

#[test]
fn attachments_need_an_existing_case() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let error = repository
        .save_attachment(
            &project("checkout.json"),
            "missing",
            "notes.txt",
            &json!({"filename": "notes.txt"}),
            b"evidence",
        )
        .expect_err("missing case");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn revision_snapshots_are_listed_ascending_and_read_back_by_version() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    repository
        .write_at(
            Resource::Cases,
            Some(&project("checkout.json")),
            "TC-001",
            &json!({"testCaseId": "TC-001"}),
        )
        .expect("case");

    // A case that has never been revised has no `revisions/` folder, and
    // that is an empty history rather than an error.
    assert!(
        repository
            .list_revisions(&project("checkout.json"), "TC-001")
            .expect("history")
            .is_empty()
    );

    // Written out of order, so the listing has to sort rather than trust
    // the directory's order.
    for version in [3, 1, 2] {
        repository
            .save_revision(
                &project("checkout.json"),
                "TC-001",
                version,
                &json!({"testCaseId": "TC-001", "version": version}),
            )
            .expect("snapshot");
    }

    // Only `v{number}.json` files are snapshots: a stray file this API did
    // not write and a directory that happens to share the name are not.
    let revisions = directory.path().join("projects/checkout/TC-001/revisions");
    std::fs::write(revisions.join("notes.txt"), b"not a snapshot").expect("stray file");
    std::fs::create_dir(revisions.join("v4.json")).expect("a directory is not a snapshot");

    assert_eq!(
        repository
            .list_revisions(&project("checkout.json"), "TC-001")
            .expect("history"),
        vec![1, 2, 3]
    );
    assert_eq!(
        repository
            .read_revision(&project("checkout.json"), "TC-001", 2)
            .expect("snapshot")["version"],
        json!(2)
    );

    // A snapshot is immutable, so re-saving a version leaves the first one.
    repository
        .save_revision(
            &project("checkout.json"),
            "TC-001",
            1,
            &json!({"testCaseId": "TC-001", "version": 99}),
        )
        .expect("re-save");
    assert_eq!(
        repository
            .read_revision(&project("checkout.json"), "TC-001", 1)
            .expect("snapshot")["version"],
        json!(1)
    );

    // A version the case never recorded is a missing document.
    assert!(
        repository
            .read_revision(&project("checkout.json"), "TC-001", 4)
            .is_err()
    );

    // A snapshot needs its case, so an unknown one cannot record a revision.
    assert!(
        repository
            .save_revision(
                &project("checkout.json"),
                "TC-404",
                1,
                &json!({"testCaseId": "TC-404"}),
            )
            .is_err()
    );
}

#[test]
fn write_leaves_no_temporary_files_behind() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");

    let leftovers = fs::read_dir(directory.path().join("projects/checkout"))
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn write_overwrites_an_existing_document() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    repository
        .write_at(
            Resource::Projects,
            None,
            "checkout.json",
            &json!({"name": "renamed"}),
        )
        .expect("overwrite");

    assert_eq!(
        repository
            .read_at(Resource::Projects, None, "checkout.json")
            .expect("read")["name"],
        "renamed"
    );
}

#[test]
fn read_reports_missing_documents() {
    let (_directory, repository) = repository();
    let error = repository
        .read_at(Resource::Projects, None, "missing.json")
        .expect_err("miss");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn read_reports_corrupted_json_as_invalid_data() {
    let (directory, repository) = repository();
    fs::create_dir_all(directory.path().join("projects/broken")).expect("folder");
    fs::write(
        directory.path().join("projects/broken/project.json"),
        b"{ not json",
    )
    .expect("corrupt file");

    let error = repository
        .read_at(Resource::Projects, None, "broken.json")
        .expect_err("corrupt");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn exists_reflects_stored_documents() {
    let (_directory, repository) = repository();
    assert!(
        !repository
            .exists_at(Resource::Projects, None, "missing.json")
            .expect("miss")
    );
    create_project(&repository, "checkout.json");
    assert!(
        repository
            .exists_at(Resource::Projects, None, "checkout.json")
            .expect("hit")
    );
}

#[test]
fn project_scoped_documents_keep_their_document_lifecycle() {
    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");
    let value = json!({"testRunId": "R-001"});
    repository
        .write_at(Resource::Runs, Some(&home), "nightly.json", &value)
        .expect("write");
    assert_eq!(
        repository
            .read_at(Resource::Runs, Some(&home), "nightly.json")
            .expect("read"),
        value
    );
    assert!(
        directory
            .path()
            .join("projects/checkout/test_runs/nightly.json")
            .is_file()
    );

    repository
        .write_at(
            Resource::Runs,
            Some(&home),
            "nightly.json",
            &json!({"testRunId": "R-002"}),
        )
        .expect("overwrite");
    assert_eq!(
        repository
            .read_at(Resource::Runs, Some(&home), "nightly.json")
            .expect("read")["testRunId"],
        "R-002"
    );

    repository
        .delete_at(Resource::Runs, Some(&home), "nightly.json")
        .expect("delete");
    assert!(
        repository
            .read_at(Resource::Runs, Some(&home), "nightly.json")
            .is_err()
    );
    assert!(
        directory
            .path()
            .join("projects/checkout/test_runs")
            .is_dir(),
        "deleting one document leaves the collection, and the project, alone"
    );
}

#[test]
fn list_is_empty_for_new_storage() {
    let (_directory, repository) = repository();
    for resource in Resource::ALL {
        assert!(
            repository.list(resource).expect("list").is_empty(),
            "{resource:?}"
        );
    }
}

#[test]
fn write_leaves_stray_files_out_of_the_listing() {
    let (directory, repository) = repository();
    create_project(&repository, "alpha.json");
    create_project(&repository, "beta.json");
    fs::write(directory.path().join("projects/notes.txt"), b"ignored").expect("stray file");

    assert_eq!(
        repository.list(Resource::Projects).expect("list"),
        vec!["alpha.json".to_owned(), "beta.json".to_owned()]
    );
}

#[test]
fn documents_require_a_json_extension() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let error = repository
        .write_at(
            Resource::Projects,
            None,
            "checkout",
            &json!({"name": "Checkout"}),
        )
        .expect_err("rejected");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    for resource in [
        Resource::Runs,
        Resource::Milestones,
        Resource::Configurations,
    ] {
        let error = repository
            .write_at(
                resource,
                Some(&project("checkout.json")),
                "nightly",
                &json!({"name": "nightly"}),
            )
            .expect_err("rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
    }
}

#[test]
fn hostile_identifiers_are_rejected() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");

    for id in [
        "",
        ".",
        "..",
        "../escape.json",
        "nested/child.json",
        "back\\slash.json",
        "/absolute.json",
    ] {
        assert!(
            repository.read_at(Resource::Projects, None, id).is_err(),
            "project identifier should be rejected: {id:?}"
        );
        assert!(
            repository
                .read_at(Resource::Cases, Some(&project("checkout.json")), id)
                .is_err(),
            "case identifier should be rejected: {id:?}"
        );
        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            assert!(
                repository
                    .read_at(resource, Some(&project("checkout.json")), id)
                    .is_err(),
                "{resource:?} identifier should be rejected: {id:?}"
            );
            assert!(
                repository
                    .read_at(resource, Some(&project(id)), "nightly.json")
                    .is_err(),
                "{resource:?} should be rejected in a hostile project: {id:?}"
            );
        }
    }
}

#[test]
fn a_parent_is_required_where_the_tree_requires_one() {
    let (_directory, repository) = repository();
    assert!(
        repository
            .read_at(Resource::Suites, None, "smoke.json")
            .is_err()
    );
    assert!(repository.read_at(Resource::Cases, None, "TC-001").is_err());
    assert!(
        repository
            .read_at(Resource::Runs, None, "nightly.json")
            .is_err()
    );
    assert!(
        repository
            .read_at(
                Resource::Projects,
                Some(&project("checkout.json")),
                "checkout.json"
            )
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_document_cannot_leak_a_file_outside_the_root() {
    use std::os::unix::fs::symlink;

    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let outside = directory
        .path()
        .parent()
        .expect("parent")
        .join("secret.json");
    fs::write(
        &outside,
        serde_json::to_string(&json!({"name": "secret"})).expect("json"),
    )
    .expect("outside file");
    let runs = directory.path().join("projects/checkout/test_runs");
    fs::create_dir_all(&runs).expect("runs dir");
    symlink(&outside, runs.join("evil.json")).expect("symlink");

    let error = repository
        .read_at(Resource::Runs, Some(&project("checkout.json")), "evil.json")
        .expect_err("symlink escape must be rejected");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn concurrent_writers_never_publish_partial_documents() {
    let (_directory, repository) = repository();
    let readable = repository.clone();
    let writers = (0..8)
        .map(|index| {
            let writer = repository.clone();
            std::thread::spawn(move || {
                writer
                    .write_at(
                        Resource::Projects,
                        None,
                        "shared.json",
                        &json!({"name": index}),
                    )
                    .expect("concurrent write");
            })
        })
        .collect::<Vec<_>>();

    for writer in writers {
        writer.join().expect("writer thread");
    }

    let stored = readable
        .read_at(Resource::Projects, None, "shared.json")
        .expect("read");
    assert!(stored["name"].is_number());
}

#[cfg(unix)]
#[test]
fn stored_documents_use_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let parent = project("checkout.json");
    repository
        .write_at(
            Resource::Cases,
            Some(&parent),
            "TC-001",
            &json!({"testCaseId": "TC-001", "steps": [{}]}),
        )
        .expect("case");
    let entry = json!({"filename": "notes.txt"});
    repository
        .save_attachment(&parent, "TC-001", "notes.txt", &entry, b"evidence")
        .expect("attachment");
    repository
        .save_step_attachment(&parent, "TC-001", 0, "notes.txt", &entry, b"evidence")
        .expect("step attachment");
    repository
        .save_revision(&parent, "TC-001", 1, &json!({"testCaseId": "TC-001"}))
        .expect("snapshot");

    let mode_of = |relative: &str| {
        fs::metadata(directory.path().join(relative))
            .expect(relative)
            .permissions()
            .mode()
            & 0o777
    };

    // Every file the store writes — a document, a revision and both kinds of
    // attachment — is owner-only, so another uid can neither read nor rewrite
    // it.
    assert_eq!(mode_of("projects/checkout/project.json"), 0o600);
    assert_eq!(mode_of("projects/checkout/TC-001/test-case.json"), 0o600);
    assert_eq!(mode_of("projects/checkout/TC-001/revisions/v1.json"), 0o600);
    assert_eq!(mode_of("projects/checkout/TC-001/notes.txt"), 0o600);
    assert_eq!(mode_of("projects/checkout/TC-001/steps/0/notes.txt"), 0o600);
}

#[cfg(unix)]
#[test]
fn stored_directories_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let (directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let parent = project("checkout.json");
    repository
        .write_at(
            Resource::Cases,
            Some(&parent),
            "TC-001",
            &json!({"testCaseId": "TC-001", "steps": [{}]}),
        )
        .expect("case");
    repository
        .save_step_attachment(
            &parent,
            "TC-001",
            0,
            "notes.txt",
            &json!({"filename": "notes.txt"}),
            b"evidence",
        )
        .expect("step attachment");
    repository
        .save_revision(&parent, "TC-001", 1, &json!({"testCaseId": "TC-001"}))
        .expect("snapshot");

    let mode_of = |relative: &str| {
        fs::metadata(directory.path().join(relative))
            .expect(relative)
            .permissions()
            .mode()
            & 0o777
    };

    // A directory the store creates is owner-only too, so another uid cannot
    // even list what is stored, let alone open a document.
    assert_eq!(mode_of("projects"), 0o700);
    assert_eq!(mode_of("projects/checkout"), 0o700);
    assert_eq!(mode_of("projects/checkout/TC-001"), 0o700);
    assert_eq!(mode_of("projects/checkout/TC-001/revisions"), 0o700);
    assert_eq!(mode_of("projects/checkout/TC-001/steps/0"), 0o700);
}

#[test]
fn a_fresh_root_is_ready_and_holds_no_lock() {
    let (_directory, repository) = repository();

    let probe = repository.probe_readiness();
    assert!(probe.exists);
    assert!(probe.writable);
    assert!(probe.lockable);
    assert!(!probe.lock_held);
    assert!(probe.last_write_unix.is_some());
    assert!(probe.ready());
}

#[test]
fn a_root_that_is_gone_is_not_ready() {
    let (directory, repository) = repository();
    fs::remove_dir_all(directory.path()).expect("remove root");

    let probe = repository.probe_readiness();
    assert_eq!(probe, StorageProbe::UNREACHABLE);
    assert!(!probe.ready());
}

#[test]
fn a_lock_another_process_holds_is_busy_rather_than_broken() {
    let (_directory, repository) = repository();
    let held = repository.acquire_lock().expect("lock");

    let probe = repository.probe_readiness();
    assert!(probe.lockable, "a lock a peer holds is not a fault");
    assert!(probe.lock_held);
    assert!(probe.ready());

    drop(held);
    assert!(!repository.probe_readiness().lock_held);
}

#[test]
fn a_probe_removes_the_scratch_file_it_wrote() {
    let (directory, repository) = repository();
    repository.probe_readiness();

    let mut names = fs::read_dir(directory.path())
        .expect("root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![".tucano.lock".to_owned(), "projects".to_owned()],
        "the writability probe left something behind"
    );
}

#[test]
fn a_probe_sees_writes_below_the_root_not_only_the_root() {
    let (directory, repository) = repository();
    let opened = mtime_seconds(directory.path()).expect("root mtime");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    create_project(&repository, "checkout.json");

    let seen = repository
        .probe_readiness()
        .last_write_unix
        .expect("a visible write");
    assert!(
        seen > opened,
        "the probe reported the root alone: {seen} is not after {opened}"
    );
}

#[test]
fn polling_a_probe_never_counts_as_a_write_itself() {
    let (_directory, repository) = repository();
    let first = repository.probe_readiness().last_write_unix;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = repository.probe_readiness().last_write_unix;

    assert_eq!(
        first, second,
        "the probe's own scratch file advanced the write clock"
    );
}

#[cfg(unix)]
#[test]
fn an_unwritable_root_is_not_ready() {
    use std::os::unix::fs::PermissionsExt;

    let (directory, repository) = repository();
    let root = directory.path();
    fs::set_permissions(root, fs::Permissions::from_mode(0o555)).expect("make read-only");
    let restore = || fs::set_permissions(root, fs::Permissions::from_mode(0o755));

    // A privileged user writes through the mode bits, so there is nothing to
    // observe and nothing to assert.
    let privileged = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(".tucano-privilege-check"))
        .is_ok();
    if privileged {
        let _ = fs::remove_file(root.join(".tucano-privilege-check"));
        restore().expect("restore permissions");
        return;
    }

    let probe = repository.probe_readiness();
    assert!(probe.exists);
    assert!(!probe.writable);
    assert!(!probe.ready());

    restore().expect("restore permissions");
}

#[test]
fn create_at_refuses_a_document_the_location_already_holds() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");

    repository
        .create_at(
            Resource::Runs,
            Some(&home),
            "nightly.json",
            &json!({"name": "first"}),
        )
        .expect("first create");

    let error = repository
        .create_at(
            Resource::Runs,
            Some(&home),
            "nightly.json",
            &json!({"name": "second"}),
        )
        .expect_err("a taken location is not creatable");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        repository
            .read_at(Resource::Runs, Some(&home), "nightly.json")
            .expect("read")["name"],
        json!("first"),
        "a refused create must leave the stored document untouched"
    );
}

#[test]
fn create_at_refuses_a_suite_the_folder_already_holds() {
    let (_directory, repository) = repository();
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");

    repository
        .create_at(
            Resource::Suites,
            Some(&home),
            "regression.json",
            &json!({"name": "first"}),
        )
        .expect("first create");

    let error = repository
        .create_at(
            Resource::Suites,
            Some(&home),
            "regression.json",
            &json!({"name": "second"}),
        )
        .expect_err("a taken location is not creatable");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        repository
            .read_at(Resource::Suites, Some(&home), "regression.json")
            .expect("read")["name"],
        json!("first"),
        "a refused create must leave the stored document untouched"
    );
}

#[test]
fn concurrent_creates_of_one_identifier_yield_exactly_one_winner() {
    let (_directory, repository) = repository();
    let repository = std::sync::Arc::new(repository);
    create_project(&repository, "checkout.json");
    let home = project("checkout.json");

    let threads = 8usize;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(threads));
    let mut handles = Vec::with_capacity(threads);
    for thread in 0..threads {
        let repository = std::sync::Arc::clone(&repository);
        let barrier = std::sync::Arc::clone(&barrier);
        let home = home.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            repository.create_at(
                Resource::Runs,
                Some(&home),
                "nightly.json",
                &json!({"name": format!("thread {thread}")}),
            )
        }));
    }

    let mut created = 0usize;
    let mut refused = 0usize;
    for handle in handles {
        match handle.join().expect("thread") {
            Ok(()) => created += 1,
            Err(error) => {
                assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error}");
                refused += 1;
            }
        }
    }

    assert_eq!(created, 1, "exactly one create may win");
    assert_eq!(refused, threads - 1, "every other create is refused");
}
