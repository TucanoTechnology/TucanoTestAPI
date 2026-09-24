mod common;

use tempfile::TempDir;
use tucano_test::repository::{FileRepository, Repository, Resource};
use tucano_test::storage::Parent;

/// Helper to create a test repository with a temporary directory
fn setup_test_repo() -> (FileRepository, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo = FileRepository::new(temp_dir.path().to_path_buf()).expect("Failed to create repo");
    (repo, temp_dir)
}

#[cfg(test)]
mod path_traversal_tests {
    use super::*;

    #[test]
    fn test_rejects_dot_dot_traversal_in_resource_id() {
        let (repo, _temp) = setup_test_repo();
        let result = repo.read_at(Resource::Projects, None, "../../etc/passwd.json");
        assert!(result.is_err(), "Path traversal should be rejected");
    }

    #[test]
    fn test_rejects_absolute_path_in_resource_id() {
        let (repo, _temp) = setup_test_repo();
        let result = repo.read_at(Resource::Projects, None, "/etc/passwd.json");
        assert!(result.is_err(), "Absolute paths should be rejected");
    }

    #[test]
    fn test_rejects_encoded_traversal() {
        let (repo, _temp) = setup_test_repo();
        let result = repo.read_at(Resource::Projects, None, "..%2F..%2Fetc%2Fpasswd.json");
        assert!(result.is_err(), "URL-encoded traversal should be rejected");
    }

    #[test]
    fn test_rejects_traversal_in_write() {
        let (repo, _temp) = setup_test_repo();
        let value = serde_json::json!({
            "projectId": "../../etc/passwd.json",
            "name": "Malicious",
            "testSuites": []
        });
        let result = repo.write_at(Resource::Projects, None, "../../etc/passwd.json", &value);
        assert!(
            result.is_err(),
            "Path traversal in write should be rejected"
        );
    }

    #[test]
    fn test_rejects_traversal_through_a_case_identifier() {
        let (repo, _temp) = setup_test_repo();
        let home = Parent::Project("checkout.json".to_owned());

        for identifier in ["../escape", "../../escape", "/etc/passwd"] {
            let read = repo.read_at(Resource::Cases, Some(&home), identifier);
            assert!(read.is_err(), "{identifier} should be rejected");

            let written = repo.write_at(
                Resource::Cases,
                Some(&home),
                identifier,
                &serde_json::json!({"title": "x"}),
            );
            assert!(written.is_err(), "{identifier} should be rejected");
        }
    }
}

#[cfg(test)]
mod symlink_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn test_rejects_symlink_escape() {
        let (repo, temp) = setup_test_repo();

        // A folder outside the data root holding a valid project document: if
        // the link were followed, `read_at` would succeed and leak its
        // contents, so `PermissionDenied` proves the check never ran.
        let outside = temp.path().parent().unwrap().join("outside-secret");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("project.json"), r#"{"name": "secret"}"#).unwrap();

        let symlink_path = temp.path().join("projects").join("evil");
        symlink(&outside, &symlink_path).unwrap();

        let read_result = repo.read_at(Resource::Projects, None, "evil.json");
        assert_eq!(
            read_result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
            "symlink escape should be rejected as permission denied"
        );
    }
}

#[cfg(test)]
#[cfg(unix)]
mod hardlink_tests {
    use super::*;

    #[test]
    fn test_rejects_hardlink_escape() {
        let (repo, temp) = setup_test_repo();

        // Seed a case so its folder exists as a real attachment directory.
        repo.write_at(
            Resource::Projects,
            None,
            "checkout.json",
            &serde_json::json!({"name": "checkout"}),
        )
        .unwrap();
        let parent = Parent::Project("checkout.json".to_owned());
        repo.write_at(
            Resource::Cases,
            Some(&parent),
            "TC-001",
            &serde_json::json!({"testCaseId": "TC-001"}),
        )
        .unwrap();

        // A file outside the data root, hardlinked onto an in-tree attachment
        // name: the path is ordinary, so only the opened file can betray the
        // second link, and its content must not be served.
        let outside = temp
            .path()
            .parent()
            .unwrap()
            .join(format!("outside-secret-{}", std::process::id()));
        std::fs::write(&outside, b"outside content").unwrap();
        std::fs::hard_link(
            &outside,
            temp.path()
                .join("projects")
                .join("checkout")
                .join("TC-001")
                .join("outside-hard.txt"),
        )
        .unwrap();

        let read_result = repo.read_attachment(&parent, "TC-001", "outside-hard.txt");
        assert_eq!(
            read_result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
            "hardlink escape should be rejected as permission denied"
        );
        assert_eq!(
            std::fs::read(&outside).unwrap(),
            b"outside content",
            "the outside file must stay intact"
        );

        std::fs::remove_file(&outside).unwrap();
    }
}

#[cfg(test)]
mod malformed_json_tests {
    use super::*;

    #[test]
    fn test_rejects_malformed_json() {
        let (_repo, _temp) = setup_test_repo();
        let value = serde_json::from_str::<serde_json::Value>("{ invalid json }");
        assert!(value.is_err(), "Malformed JSON should fail to parse");
    }

    #[test]
    fn test_repository_accepts_any_valid_json() {
        // Repository layer accepts any valid JSON - validation happens at API layer
        let (repo, _temp) = setup_test_repo();
        let value = serde_json::json!({
            "projectId": "P-001",
            "name": "Test",
            "testSuites": [],
            "unknownField": "allowed at repo layer"
        });
        let result = repo.write_at(Resource::Projects, None, "P-001.json", &value);
        assert!(result.is_ok(), "Repository accepts any valid JSON");
    }

    #[test]
    fn test_api_layer_validates_schema() {
        // Schema validation (required fields, unknown fields) happens at API layer
        // This is tested in service.rs integration tests
        let (repo, _temp) = setup_test_repo();
        let value = serde_json::json!({
            "projectId": "P-001"
            // Missing required fields - repo accepts, API would reject
        });
        let result = repo.write_at(Resource::Projects, None, "P-001.json", &value);
        assert!(result.is_ok(), "Repository layer does not validate schema");
    }
}

#[cfg(test)]
mod data_integrity_tests {
    use super::*;

    #[test]
    fn test_write_creates_valid_json_file() {
        let (repo, _temp) = setup_test_repo();

        let value = serde_json::json!({
            "projectId": "P-001",
            "name": "Test Project",
            "testSuites": []
        });
        repo.write_at(Resource::Projects, None, "P-001.json", &value)
            .unwrap();

        let result = repo
            .read_at(Resource::Projects, None, "P-001.json")
            .unwrap();
        assert_eq!(result["name"], "Test Project");
    }

    #[test]
    fn test_concurrent_writes_do_not_corrupt() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let (repo, _temp) = setup_test_repo();
        let _temp = _temp; // Keep temp dir alive
        let repo = Arc::new(repo);
        let barrier = Arc::new(Barrier::new(2));

        let repo1 = Arc::clone(&repo);
        let repo2 = Arc::clone(&repo);
        let barrier1 = Arc::clone(&barrier);
        let barrier2 = Arc::clone(&barrier);

        let handle1 = thread::spawn(move || {
            barrier1.wait();
            let value = serde_json::json!({
                "projectId": "P-001",
                "name": "Thread 1",
                "testSuites": []
            });
            repo1.write_at(Resource::Projects, None, "P-001.json", &value)
        });

        let handle2 = thread::spawn(move || {
            barrier2.wait();
            let value = serde_json::json!({
                "projectId": "P-001",
                "name": "Thread 2",
                "testSuites": []
            });
            repo2.write_at(Resource::Projects, None, "P-001.json", &value)
        });

        let result1 = handle1.join().unwrap();
        let result2 = handle2.join().unwrap();

        // Both writers run under the storage lock, so each either publishes a
        // complete document or fails; the survivor is always valid JSON.
        assert!(
            result1.is_ok() || result2.is_ok(),
            "at least one write should succeed"
        );

        let final_value = repo
            .read_at(Resource::Projects, None, "P-001.json")
            .unwrap();
        assert!(
            final_value["name"].as_str().is_some(),
            "Final value should be valid JSON"
        );
    }

    /// F-177-3 regression (Issue #323): a write the API acknowledged with `200`
    /// must never be silently discarded. The test above only proves the
    /// surviving document stays parseable; this one proves durability — N
    /// concurrent writers each record a distinct acknowledged write, and
    /// afterwards every acknowledged value reads back, from the live document
    /// or from one of the revision snapshots the writes produced.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_every_acknowledged_write_is_readable_back() {
        use axum::http::StatusCode;
        use serde_json::json;
        use std::collections::HashSet;
        use std::sync::Arc;

        let (_dir, app) = common::test_app();
        let app = Arc::new(app);

        let project = common::create_named(&app, "/projects", "integrity").await;
        let (status, created) = common::send_json(
            &app,
            common::json_request(
                "POST",
                &format!("/projects/{project}/test_cases"),
                &json!({
                    "testCaseId": "CW-1",
                    "title": "base",
                    "expectedResult": "ok"
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "create: {created}");

        // Concurrent unconditional writes (no If-Match): the legacy
        // last-writer-wins arm, which the audit showed discarding half of the
        // acknowledged writes. Each carries a distinct qualifying field, so
        // each acknowledged write must start exactly one new version.
        let writers = 16usize;
        let barrier = Arc::new(tokio::sync::Barrier::new(writers));
        let mut handles = Vec::with_capacity(writers);
        for i in 0..writers {
            let app = Arc::clone(&app);
            let barrier = Arc::clone(&barrier);
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                common::send_json(
                    &app,
                    common::json_request(
                        "PUT",
                        "/test_cases/CW-1",
                        &json!({"title": format!("writer {i}")}),
                    ),
                )
                .await
            }));
        }
        for handle in handles {
            let (status, body) = handle.await.expect("task");
            assert_eq!(
                status,
                StatusCode::OK,
                "every concurrent write is acknowledged: {body}"
            );
        }

        // The version advanced once per acknowledged write, and the history
        // records every one of them — the audit's failure shape was 32
        // acknowledged writes answering version 17 with 16 history entries.
        let (status, live) = common::send_json(&app, common::get("/test_cases/CW-1")).await;
        assert_eq!(status, StatusCode::OK, "read back: {live}");
        assert_eq!(
            live["version"].as_u64(),
            Some(1 + writers as u64),
            "each acknowledged write starts exactly one version: {live}"
        );

        let (status, history) =
            common::send_json(&app, common::get("/test_cases/CW-1/history")).await;
        assert_eq!(status, StatusCode::OK, "history: {history}");
        assert_eq!(
            history.as_array().map(Vec::len),
            Some(writers),
            "the history records every acknowledged write: {history}"
        );

        // Every distinct acknowledged value is readable back: the last writer's
        // title from the live document, every earlier one from the immutable
        // snapshot its write left behind.
        let mut titles: HashSet<String> = HashSet::new();
        titles.insert(live["title"].as_str().expect("live title").to_owned());
        for version in 1..=writers as u64 {
            let (status, revision) = common::send_json(
                &app,
                common::get(&format!("/test_cases/CW-1/history/{version}")),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "revision {version}: {revision}");
            titles.insert(
                revision["title"]
                    .as_str()
                    .expect("revision title")
                    .to_owned(),
            );
        }
        for i in 0..writers {
            assert!(
                titles.contains(&format!("writer {i}")),
                "the acknowledged write from writer {i} was silently discarded; readable titles: {titles:?}"
            );
        }
    }
}

#[cfg(test)]
mod lock_timeout_tests {
    use super::*;
    use axum::http::StatusCode;
    use fs2::FileExt;
    use serde_json::json;
    use std::fs::OpenOptions;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    #[tokio::test]
    async fn a_write_under_lock_contention_answers_503_with_retry_after() {
        let directory = TempDir::new().expect("temp dir");
        // A short timeout so the test does not wait the default five seconds.
        let timeout = Duration::from_millis(200);
        let app = common::app_at_with_lock_timeout(directory.path(), timeout);

        // Hold the advisory lock from a background thread so the API's
        // acquire_lock sees a contention it cannot resolve within the deadline.
        let lock_path = directory.path().join(".tucano.lock");
        let barrier = Arc::new(Barrier::new(2));
        let barrier_bg = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
                .expect("open lock file");
            lock.lock_exclusive().expect("lock");
            // Signal the main thread that the lock is held.
            barrier_bg.wait();
            // Keep the lock held long enough for the API's deadline to expire.
            thread::sleep(Duration::from_millis(1000));
            drop(lock);
        });

        // Wait for the background thread to hold the lock before sending the
        // request, so the contention window is deterministic.
        barrier.wait();

        let (status, headers, body) = common::send_full(
            &app,
            common::json_request("POST", "/projects", &json!({"name": "blocked"})),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "a lock timeout must answer 503"
        );
        let value: serde_json::Value =
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        common::assert_error_envelope(&value, "lock_timeout");
        assert_eq!(
            headers
                .get(axum::http::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            Some("1"),
            "a 503 from lock contention must carry Retry-After: 1"
        );

        handle.join().expect("background thread");
    }
}
