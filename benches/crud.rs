//! Criterion benchmarks for the Tucano Test API domain layer.
//!
//! Each benchmark talks directly to [`TestService`] (backed by a
//! [`FileRepository`]) so it measures the work the API actually does — JSON
//! validation, folder layout, atomic writes — without going through a TCP
//! stack. Every workload uses its own isolated `TempDir`, so runs are
//! independent and can be compared across revisions.
//!
//! # Workloads
//!
//! * `crud_case_full_cycle` — create, read, update, delete a single test case
//!   per iteration. Exercises the full write path (atomic rename, revision
//!   snapshot on update) and the global lookup path.
//! * `list_projects_many` — list projects after seeding a tree with a bounded
//!   number of projects. Exercises the folder walk and substring filter.
//! * `list_cases_in_suite` — list the cases inside one suite. Exercises the
//!   child folder walk.
//! * `validate_large_case_payload` — validate a test-case body carrying a
//!   hundred structured steps and a thousand tags, without persisting it.
//!   Exercises the payload validator in isolation from I/O.
//! * `attachment_upload_delete` — store and delete a fixed-size attachment
//!   against a persistent case. Exercises the atomic file write and the
//!   marker-document re-serialisation that records the attachment metadata.
//!
//! # Running
//!
//! ```sh
//! cargo bench                       # every workload
//! cargo bench --bench crud          # this bench only
//! cargo bench -- "validate"         # one workload by substring
//! ```
//!
//! Baseline numbers are recorded in
//! `docs/architecture/performance.md`; rerun after a change that touches the
//! storage or domain layers and compare against the previous baseline before
//! declaring a regression.

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::{Value, json};
use tempfile::TempDir;
use tucano_test::domain::{ListQuery, TestService, validation};
use tucano_test::storage::{FileRepository, Parent, Resource};

/// Upper bound on how many seeded items a workload creates before the timer
/// starts. Picked to be large enough that list workloads exercise the folder
/// walk but small enough that a bench run finishes in seconds, not minutes.
const SEED_CAP: usize = 64;

/// Size of the attachment payload the upload benchmark exercises. 8 KiB is
/// a realistic small screenshot or log snippet; the API cap is 50 MiB, so
/// this stays well below it.
const ATTACHMENT_BYTES: usize = 8 * 1024;

/// Builds an isolated service backed by a fresh temporary directory. The
/// returned `TempDir` must stay alive for as long as the service is used.
fn fresh_service() -> (TempDir, TestService<FileRepository>) {
    let directory = TempDir::new().expect("temp dir");
    let repository = FileRepository::new(directory.path()).expect("repository");
    (directory, TestService::new(repository))
}

/// Creates a project and returns its identifier.
fn seed_project(service: &TestService<FileRepository>, name: &str) -> String {
    let created = service
        .create(Resource::Projects, &json!({ "name": name }))
        .expect("create project");
    created.id
}

/// Creates a suite inside `project` and returns its identifier.
fn seed_suite(service: &TestService<FileRepository>, project: &str, name: &str) -> String {
    let home = Parent::Project(project.to_owned());
    let created = service
        .create_in(Resource::Suites, &home, &json!({ "name": name }))
        .expect("create suite");
    created.id
}

/// Creates a case directly inside `parent` with the given identifier.
fn seed_case(service: &TestService<FileRepository>, parent: &Parent, case_id: &str) {
    service
        .create_in(
            Resource::Cases,
            parent,
            &json!({
                "testCaseId": case_id,
                "title": format!("Benchmark case {case_id}"),
                "expectedResult": "Benchmarked",
            }),
        )
        .expect("create case");
}

/// One full create → read → update → delete cycle for a test case.
///
/// Each iteration creates a brand new case (the identifier rotates per
/// iteration so no leftover state is visible to the next run), reads it,
/// updates a qualifying field (which starts a revision snapshot), and
/// deletes it. The cycle is the unit the bench reports on.
fn bench_crud_case_full_cycle(c: &mut Criterion) {
    let (_dir, service) = fresh_service();
    let project = seed_project(&service, "bench");
    let home = Parent::Project(project);

    let mut iteration: u64 = 0;
    c.bench_function("crud_case_full_cycle", |b| {
        b.iter(|| {
            iteration += 1;
            let case_id = format!("TC-{:06}", iteration);
            let body = json!({
                "testCaseId": case_id,
                "title": "Benchmark login",
                "expectedResult": "Stored",
            });

            service
                .create_in(Resource::Cases, &home, &body)
                .expect("bench create");

            let document = service.get(Resource::Cases, &case_id).expect("bench read");
            assert_eq!(
                document.get("testCaseId").and_then(Value::as_str),
                Some(case_id.as_str())
            );

            service
                .update(
                    Resource::Cases,
                    &case_id,
                    &json!({ "title": "Benchmark login (revised)" }),
                )
                .expect("bench update");

            service
                .delete(Resource::Cases, &case_id)
                .expect("bench delete");
        });
    });
}

/// Lists a seeded project collection. Projects are created once outside the
/// timed loop so the measurement reflects the listing, not the seeding.
fn bench_list_projects(c: &mut Criterion) {
    let (_dir, service) = fresh_service();
    for i in 0..SEED_CAP {
        seed_project(&service, &format!("project-{i:04}"));
    }

    let mut group = c.benchmark_group("list_projects");
    group.bench_function("all", |b| {
        b.iter(|| {
            let items = service
                .list(Resource::Projects, &ListQuery::default())
                .expect("list projects");
            assert_eq!(items.len(), SEED_CAP);
        });
    });
    group.bench_function("with_filter", |b| {
        b.iter(|| {
            let items = service
                .list(
                    Resource::Projects,
                    &ListQuery {
                        filter: Some("project-00".to_owned()),
                        ..ListQuery::default()
                    },
                )
                .expect("list projects with filter");
            assert!(!items.is_empty());
        });
    });
    group.finish();
}

/// Lists the cases inside one seeded suite.
fn bench_list_cases_in_suite(c: &mut Criterion) {
    let (_dir, service) = fresh_service();
    let project = seed_project(&service, "bench");
    let suite_id = seed_suite(&service, &project, "bench-suite");
    let parent = Parent::Suite {
        project: project.clone(),
        suite: suite_id.clone(),
    };
    for i in 0..SEED_CAP {
        seed_case(&service, &parent, &format!("TC-{i:04}"));
    }

    c.bench_function("list_cases_in_suite", |b| {
        b.iter(|| {
            let items = service
                .list_children(&parent, Resource::Cases)
                .expect("list suite children");
            assert_eq!(items.len(), SEED_CAP);
        });
    });
}

/// Validates a large test-case payload without persisting it. The payload
/// carries a hundred structured steps and a thousand tags — well above any
/// realistic case — so the measurement reflects the validator's cost on
/// the upper tail of what the API accepts.
fn bench_validate_large_case(c: &mut Criterion) {
    let steps: Vec<Value> = (0..100)
        .map(|i| {
            json!({
                "action": format!("Step {i}: open the form and submit"),
                "expectedResult": format!("Step {i}: the form accepts the value"),
            })
        })
        .collect();
    let tags: Vec<Value> = (0..1_000)
        .map(|i| Value::String(format!("tag-{i:04}")))
        .collect();
    let payload = json!({
        "testCaseId": "TC-LARGE",
        "title": "A deliberately large test case for the validator",
        "description": "Exercises the payload validator on the upper tail of what the API accepts.",
        "preconditions": "None",
        "steps": steps,
        "expectedResult": "The validator accepts the payload in bounded time",
        "priority": "High",
        "severity": "Normal",
        "testType": "Functional",
        "exploratory": false,
        "tags": tags,
    });

    c.bench_function("validate_large_case_payload", |b| {
        b.iter(|| {
            validation::validate_payload(Resource::Cases, &payload).expect("payload must validate");
        });
    });
}

/// Stores and deletes a fixed-size attachment against a persistent case.
///
/// The case is created once so the measurement isolates the attachment I/O
/// and the marker-document re-serialisation that records the metadata. The
/// filename the store returns is unique per call, so the matching delete
/// always addresses what was just written.
fn bench_attachment_upload_delete(c: &mut Criterion) {
    let (_dir, service) = fresh_service();
    let project = seed_project(&service, "bench");
    let home = Parent::Project(project);
    seed_case(&service, &home, "TC-ATTACH");
    let parent = service
        .require_test_case("TC-ATTACH")
        .expect("resolve case");

    let contents: Vec<u8> = (0..ATTACHMENT_BYTES).map(|i| (i % 251) as u8).collect();

    c.bench_function("attachment_upload_delete", |b| {
        b.iter(|| {
            let stored = service
                .store_attachment(&parent, "TC-ATTACH", "bench.bin", &contents)
                .expect("store attachment");
            service
                .delete_attachment(&parent, "TC-ATTACH", &stored.filename)
                .expect("delete attachment");
        });
    });
}

/// Configurable cap on the number of timed samples Criterion takes. Raising
/// this smooths noise on noisy hardware; lowering it shortens runs on clean
/// CI workers. Kept as a module-level constant so a release run can bump it
/// without touching each bench function.
const _CONCURRENCY_CAP: usize = 100;

criterion_group!(
    benches,
    bench_crud_case_full_cycle,
    bench_list_projects,
    bench_list_cases_in_suite,
    bench_validate_large_case,
    bench_attachment_upload_delete,
);
criterion_main!(benches);
