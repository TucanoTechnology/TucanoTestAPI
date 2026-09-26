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

    /// #366: the same link-following read #327 closed for attachments still
    /// existed for documents, revision snapshots, grants and the composition
    /// copy — each decided confinement from a *path* and then opened it. A
    /// hardlink has nothing to canonicalise, so the path check passed and the
    /// `fs::read` followed the second name. Every read and the copy now go
    /// through the handle-confined reader: the opened descriptor is verified.
    #[test]
    fn test_rejects_hardlink_documents_revisions_grants_and_copies() {
        let (repo, temp) = setup_test_repo();
        let root = temp.path().to_path_buf();
        let parent = Parent::Project("checkout.json".to_owned());
        let outside = root
            .parent()
            .unwrap()
            .join(format!("outside-{}", std::process::id()));

        repo.write_at(
            Resource::Projects,
            None,
            "checkout.json",
            &serde_json::json!({"name": "checkout"}),
        )
        .unwrap();
        repo.write_at(
            Resource::Cases,
            Some(&parent),
            "TC-001",
            &serde_json::json!({"testCaseId": "TC-001"}),
        )
        .unwrap();

        // 1. A document: the stored path is ordinary; the bytes behind the
        // name live outside the root. The project's own document is its
        // marker file, exactly the layout the ticket's repro plants.
        let doc = tucano_test::storage::layout::project_marker(&root, "checkout.json")
            .expect("project document path");
        std::fs::remove_file(&doc).unwrap();
        std::fs::write(&outside, br#"{"projectId":"checkout","name":"STOLEN"}"#).unwrap();
        std::fs::hard_link(&outside, &doc).unwrap();
        let error = repo
            .read_at(Resource::Projects, None, "checkout.json")
            .expect_err("a planted document name must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);

        // Control: the same path reads fine once the second name is gone.
        std::fs::remove_file(&doc).unwrap();
        std::fs::remove_file(&outside).unwrap();
        repo.write_at(
            Resource::Projects,
            None,
            "checkout.json",
            &serde_json::json!({"name": "checkout"}),
        )
        .unwrap();
        let read_back = repo
            .read_at(Resource::Projects, None, "checkout.json")
            .expect("an unlinked document reads");
        assert_eq!(read_back["name"], "checkout");

        // 2. A revision snapshot — served verbatim by the history routes.
        std::fs::write(&outside, br#"{"testCaseId":"STOLEN"}"#).unwrap();
        let revision = tucano_test::storage::layout::revision_marker(&root, &parent, "TC-001", 1)
            .expect("revision path");
        std::fs::create_dir_all(revision.parent().unwrap()).unwrap();
        std::fs::hard_link(&outside, &revision).unwrap();
        let error = repo
            .read_revision(&parent, "TC-001", 1)
            .expect_err("a planted revision name must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        std::fs::remove_file(&revision).unwrap();

        // 3. A grant document — not served by any route, but read on every
        // authorization decision.
        let store = tucano_test::auth::AuthStore::new(&root).unwrap();
        let grants_dir = root.join("auth").join("projects");
        std::fs::create_dir_all(&grants_dir).unwrap();
        std::fs::write(&outside, br#"{"users":{}}"#).unwrap();
        std::fs::hard_link(&outside, grants_dir.join("checkout.json")).unwrap();
        let error = store
            .grants("checkout.json")
            .expect_err("a planted grant name must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        std::fs::remove_file(grants_dir.join("checkout.json")).unwrap();

        // 4. The composition copy: an ordinary `include` with mode `copy`
        // walked the source folder with `fs::copy`, which follows planted
        // names — it would have materialised outside bytes into the tree.
        std::fs::write(&outside, b"outside content").unwrap();
        let case_doc = tucano_test::storage::layout::case_marker(&root, &parent, "TC-001")
            .expect("case document path");
        let case_folder = case_doc.parent().unwrap();
        std::fs::hard_link(&outside, case_folder.join("planted.bin")).unwrap();
        repo.write_at(
            Resource::Projects,
            None,
            "beta.json",
            &serde_json::json!({"name": "beta"}),
        )
        .unwrap();
        let error = repo
            .place(
                Resource::Cases,
                &parent,
                "TC-001",
                &Parent::Project("beta.json".to_owned()),
                tucano_test::storage::Placement::Copy,
            )
            .expect_err("the copy must refuse the planted name");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        std::fs::remove_file(&outside).unwrap();
    }
}
