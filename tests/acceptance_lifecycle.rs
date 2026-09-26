//! End-to-end acceptance over the committed fixture volume (#101): the
//! lifecycle create project→suite→case→run→result→milestone progress→delete
//! runs against the seeded tree, asserting at every step that the on-disk
//! layout mirrors what the API reports — because the folder tree IS the
//! store. The untouched fixture documents must survive the session byte for
//! byte, and no temporary litter may remain.

mod common;

use std::{fs, path::PathBuf};

use axum::http::StatusCode;
use common::{delete, get, json_request, send_json};
use serde_json::json;

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    fs::create_dir_all(to).expect("mkdir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy");
        }
    }
}

fn staged() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().expect("temp dir");
    copy_tree(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/data"),
        dir.path(),
    );
    let data = dir.path().to_path_buf();
    (dir, data.clone())
}

fn snapshot(data: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut walk = vec![data.to_path_buf()];
    while let Some(dir) = walk.pop() {
        for entry in fs::read_dir(&dir).expect("read dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if entry.file_type().expect("type").is_dir() {
                walk.push(path);
            } else if out
                .insert(path.clone(), fs::read(&path).expect("read"))
                .is_some()
            {
                panic!("duplicate path {path:?}");
            }
        }
    }
    out
}

#[tokio::test]
async fn the_lifecycle_mirrors_the_api_onto_the_disk() {
    let (_stage, data) = staged();
    let before = snapshot(&data);
    let app = common::app_at(&data);

    // Seeded state serves.
    let (status, project) = send_json(&app, get("/projects/fixture-app.json")).await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["name"], "fixture-app");
    let suite_names: Vec<_> = project["testSuites"]
        .as_array()
        .expect("suites")
        .iter()
        .map(|s| s["suiteId"].as_str().expect("id").to_owned())
        .collect();
    assert!(suite_names.contains(&"release-suites.json".to_owned()));

    // Create a suite: the folder appears with its marker document.
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/fixture-app.json/test_suites",
            &json!({"name": "acceptance-suites"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert!(
        data.join("projects/fixture-app/acceptance-suites/suite.json")
            .is_file(),
        "the suite folder mirrors the create"
    );

    // Create a case inside it; the case folder holds its document.
    let (status, created) = send_json(
        &app,
        json_request(
            "POST",
            "/test_suites/acceptance-suites.json/test_cases",
            &json!({"testCaseId": "TC-900", "title": "acceptance case", "expectedResult": "passes"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let case_doc = data.join("projects/fixture-app/acceptance-suites/TC-900/test-case.json");
    assert!(case_doc.is_file(), "the case folder mirrors the create");
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&case_doc).expect("case doc")).unwrap();
    assert_eq!(stored["version"], json!(1), "the API stamps the version");

    // Run + result against the fixture run, and milestone progress derived.
    let (status, _) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/release-1.json/results",
            &json!({"testCaseId": "TC-100", "status": "Retest"}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "re-recording merges into the stored result"
    );
    let (status, progress) = send_json(&app, get("/milestones/ga.json/progress")).await;
    assert_eq!(status, StatusCode::OK, "{progress}");
    assert_eq!(progress["totalCases"], json!(2));
    assert_eq!(
        progress["retest"],
        json!(1),
        "the fresh result is in the bucket totals"
    );

    // A qualifying update snapshots the previous version to revisions/.
    let (status, _) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-900",
            &json!({"title": "acceptance case, renamed"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        data.join("projects/fixture-app/acceptance-suites/TC-900/revisions/v1.json")
            .is_file(),
        "the revision snapshot lands beside its case"
    );

    // Delete cascades the folder away.
    let (status, body) = send_json(&app, delete("/test_cases/TC-900")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !data
            .join("projects/fixture-app/acceptance-suites/TC-900")
            .exists(),
        "deleting the case removes its folder"
    );

    // The seeded documents this session never addressed are byte-identical.
    let after = snapshot(&data);
    for path in before.keys() {
        let relative = path.strip_prefix(&data).unwrap();
        if relative.starts_with("projects/fixture-app/test_runs")
            || relative.starts_with("projects/fixture-app/acceptance-suites")
        {
            continue; // release-1.json was re-recorded; the new suite is session work
        }
        if let Some(previous) = before.get(path) {
            assert_eq!(
                after.get(path),
                Some(previous),
                "an untouched fixture document changed: {relative:?}"
            );
        }
    }
    let litter: Vec<_> = after
        .keys()
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with(".tucano-"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        litter.is_empty(),
        "the session left temporary litter: {litter:?}"
    );
}
