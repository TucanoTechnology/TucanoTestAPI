//! Migration fixtures and the rollback drill (#101).
//!
//! `tests/fixtures/data` is a committed, API-generated snapshot of a
//! representative volume: a project holding a direct case, a suite holding a
//! case with a structured step, an attachment and a revision snapshot, a run
//! with results and a defect link, a milestone and a configuration. The
//! round-trip test guards the format: today's readers must parse every
//! document, and a no-op rewrite must not change any of them. The rollback
//! drill guards the refusal: a document the current reader cannot understand —
//! here, a future `formatVersion` it was never taught — must be refused as the
//! safe error envelope, leave the stored bytes untouched, and leave the rest
//! of the volume serving.

mod common;

use std::{fs, path::PathBuf};

use axum::http::StatusCode;
use common::{get, send_json};
use serde_json::{Value, json};
use tucano_test::repository::{FileRepository, Repository, Resource};

fn fixture_tree() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/data")
}

/// Copy of the committed tree into a live tempdir, mirroring how a restored
/// volume arrives.
fn stage() -> (tempfile::TempDir, PathBuf) {
    let target = tempfile::TempDir::new().expect("temp dir");
    copy_tree(&fixture_tree(), target.path());
    let data = target.path().to_path_buf();
    (target, data)
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    fs::create_dir_all(to).expect("mkdir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy");
        }
    }
}

#[test]
fn the_committed_volume_round_trips_through_todays_reader_and_writer() {
    let (_stage, data) = stage();
    let repository = FileRepository::new(&data).expect("open the fixture volume");

    let project = repository
        .read_at(Resource::Projects, None, "fixture-app.json")
        .expect("fixture project reads");
    assert_eq!(project["name"], "fixture-app");

    // A no-op rewrite — read, then store unchanged — must leave the document
    // semantically identical, and must leave no temporary litter behind.
    repository
        .write_at(Resource::Projects, None, "fixture-app.json", &project)
        .expect("no-op rewrite");
    let after = repository
        .read_at(Resource::Projects, None, "fixture-app.json")
        .expect("re-read");
    assert_eq!(after, project, "a no-op rewrite changed the document");

    // Every other document class parses too.
    let suite = repository
        .read_at(
            Resource::Suites,
            Some(&tucano_test::storage::Parent::Project(
                "fixture-app.json".into(),
            )),
            "release-suites.json",
        )
        .expect("fixture suite reads");
    assert_eq!(suite["name"], "release-suites");
    let run = repository
        .read_at(
            Resource::Runs,
            Some(&tucano_test::storage::Parent::Project(
                "fixture-app.json".into(),
            )),
            "release-1.json",
        )
        .expect("fixture run reads");
    assert_eq!(
        run["results"].as_array().expect("results array").len(),
        2,
        "the fixture run must carry both results"
    );
    let milestone = repository
        .read_at(
            Resource::Milestones,
            Some(&tucano_test::storage::Parent::Project(
                "fixture-app.json".into(),
            )),
            "ga.json",
        )
        .expect("fixture milestone reads");
    assert_eq!(milestone["name"], "ga");
    let configuration = repository
        .read_at(
            Resource::Configurations,
            Some(&tucano_test::storage::Parent::Project(
                "fixture-app.json".into(),
            )),
            "chrome.json",
        )
        .expect("fixture configuration reads");
    assert_eq!(configuration["browser"], "chrome");

    // Revision snapshot and attachment: served shapes parse from disk.
    let suite_parent = tucano_test::storage::Parent::Suite {
        project: "fixture-app.json".into(),
        suite: "release-suites.json".into(),
    };
    let revision = repository
        .read_revision(&suite_parent, "TC-100", 1)
        .expect("fixture revision reads");
    assert_eq!(revision["testCaseId"], "TC-100");
    let attachment = repository
        .read_attachment(&suite_parent, "TC-100", "1-expected.png")
        .expect("fixture attachment reads");
    assert_eq!(attachment.len(), 22);

    // The volume holds no `.tucano-*.tmp` leftovers after the round trip.
    let litter: Vec<_> = walk(&data)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .map(|n| n.to_string_lossy().starts_with(".tucano-"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        litter.is_empty(),
        "round trip left temporary files: {litter:?}"
    );
}

#[tokio::test]
async fn a_document_the_reader_cannot_understand_is_refused_cleanly() {
    // The versioning plan's fallback, measured today: an unknown top-level
    // key — the shape of a future `formatVersion` bump the reader was never
    // taught — is refused as the safe storage error, the stored bytes are
    // left exactly as they were, and the rest of the volume keeps serving.
    let (_stage, data) = stage();
    let document = data.join("projects/fixture-app/project.json");

    let mut future: Value =
        serde_json::from_slice(&fs::read(&document).expect("read fixture document"))
            .expect("parse");
    future["formatVersion"] = json!(99);
    let injected = serde_json::to_vec_pretty(&future).unwrap();
    fs::write(&document, &injected).expect("write future shape");

    let app = common::app_at(&data);
    let (status, body) = send_json(&app, get("/projects/fixture-app.json")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"]["code"], "storage_error");
    assert!(
        body["error"]["message"].as_str().map(|m| !m.is_empty()) == Some(true),
        "the refusal still explains nothing about internals: {body}"
    );
    assert_eq!(
        fs::read(&document).expect("re-read"),
        injected,
        "a refused read must leave the stored bytes exactly as they were"
    );

    // The sibling documents still serve.
    let (status, suite) = send_json(&app, get("/test_suites/release-suites.json")).await;
    assert_eq!(status, StatusCode::OK, "{suite}");
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        if entry.file_type().expect("type").is_dir() {
            out.extend(walk(&entry.path()));
        } else {
            out.push(entry.path());
        }
    }
    out
}
