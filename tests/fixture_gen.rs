//! One-shot fixture generator: drives the public API exactly as an operator
//! would, producing the committed tree under tests/fixtures/data. Re-run with
//! `cargo test --test fixture_gen -- --ignored --nocapture` after a deliberate
//! format change, then copy the printed directory over the committed fixture.
mod common;

use common::{get, json_request, send_json, test_app};
use serde_json::json;

#[tokio::test]
#[ignore]
async fn generate() {
    let (directory, app) = test_app();

    macro_rules! expect {
        ($status:expr, $code:expr, $body:expr) => {{
            assert_eq!($status, $code, "unexpected status: {}", $body);
            $body
        }};
    }

    let (s, b) = send_json(&app, json_request("POST", "/projects", &json!({"projectId": "fixture-app.json", "name": "fixture-app", "description": "representative project", "tags": ["fixture"]}))).await;
    expect!(s, 201, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/fixture-app.json/test_suites",
            &json!({"name": "release-suites", "testCases": []}),
        ),
    )
    .await;
    expect!(s, 201, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/test_suites/release-suites.json/test_cases",
            &json!({"testCaseId": "TC-100", "title": "login works", "expectedResult": "session issued", "steps": ["open the login page", {"action": "submit credentials", "expectedResult": "redirect to dashboard"}], "priority": "High", "tags": ["smoke"]}),
        ),
    )
    .await;
    expect!(s, 201, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/fixture-app.json/test_cases",
            &json!({"testCaseId": "TC-101", "title": "held directly by the project", "expectedResult": "listed"}),
        ),
    )
    .await;
    expect!(s, 201, b);
    // a qualifying update so a revision snapshot (revisions/1.json) exists
    let (s, b) = send_json(
        &app,
        json_request(
            "PUT",
            "/test_cases/TC-100",
            &json!({"title": "login works end to end"}),
        ),
    )
    .await;
    expect!(s, 200, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/fixture-app.json/test_runs",
            &json!({"name": "release-1"}),
        ),
    )
    .await;
    expect!(s, 201, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/release-1.json/test_suites",
            &json!({"suiteId": "release-suites.json"}),
        ),
    )
    .await;
    expect!(s, 200, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/release-1.json/test_cases",
            &json!({"testCaseId": "TC-101"}),
        ),
    )
    .await;
    expect!(s, 200, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/fixture-app.json/milestones",
            &json!({"name": "ga", "testRunIds": ["release-1.json"]}),
        ),
    )
    .await;
    expect!(s, 201, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/projects/fixture-app.json/configurations",
            &json!({"name": "chrome", "browser": "chrome"}),
        ),
    )
    .await;
    expect!(s, 201, b);
    let (s, b) = send_json(
        &app,
        json_request("POST", "/test_runs/release-1.json/results", &json!({"testCaseId": "TC-100", "status": "Passed", "notes": "fixture result", "durationMs": 12})),
    )
    .await;
    expect!(s, 200, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/release-1.json/results",
            &json!({"testCaseId": "TC-101", "status": "Failed"}),
        ),
    )
    .await;
    expect!(s, 200, b);
    let (s, b) = send_json(
        &app,
        json_request(
            "POST",
            "/test_runs/release-1.json/results/TC-101/defects",
            &json!({"defectId": "42", "defectUrl": "https://github.com/tucanotechnology/example/issues/42", "trackerType": "github", "title": "direct case fails"}),
        ),
    )
    .await;
    expect!(s, 201, b);
    let (s, progress) = send_json(&app, get("/milestones/ga.json/progress")).await;
    expect!(s, 200, progress);

    let out = std::path::Path::new("/tmp/opencode/fixture-data");
    let _ = std::fs::remove_dir_all(out);
    copy_tree(directory.path(), out);
    println!("FIXTURE_TREE {out:?}");
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "auth" || name.ends_with(".lock") {
            continue; // the auth store is test scaffolding, not fixture data
        }
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
