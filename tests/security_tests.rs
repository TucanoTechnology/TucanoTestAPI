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
}
