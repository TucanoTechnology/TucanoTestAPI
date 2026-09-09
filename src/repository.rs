use fs2::FileExt;
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const RESOURCE_DIRS: [&str; 5] = [
    "projects",
    "test_cases",
    "test_suites",
    "test_runs",
    "milestones",
];

#[derive(Clone)]
pub struct FileRepository {
    root: PathBuf,
}

impl FileRepository {
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        for resource in RESOURCE_DIRS {
            fs::create_dir_all(root.join(resource))?;
        }
        Ok(Self { root })
    }

    pub fn list(&self, resource: &str) -> io::Result<Vec<String>> {
        let mut entries = fs::read_dir(self.resource_dir(resource))?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let kind = entry.file_type().ok()?;
                if resource == "test_cases" {
                    kind.is_dir()
                        .then(|| entry.file_name().to_string_lossy().into_owned())
                } else {
                    kind.is_file()
                        .then(|| entry.file_name().to_string_lossy().into_owned())
                        .filter(|name| name.ends_with(".json"))
                }
            })
            .collect::<Vec<_>>();
        entries.sort();
        Ok(entries)
    }

    pub fn read(&self, resource: &str, id: &str) -> io::Result<Value> {
        let path = self.resource_path(resource, id)?;
        let contents = fs::read_to_string(path)?;
        serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn exists(&self, resource: &str, id: &str) -> io::Result<bool> {
        Ok(self.resource_path(resource, id)?.is_file())
    }

    pub fn write(&self, resource: &str, id: &str, value: &Value) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let directory = self.resource_dir(resource);
        fs::create_dir_all(&directory)?;
        let destination = self.resource_path(resource, id)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = directory.join(format!(".{}.tmp-{}", id, unique_suffix()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        set_private_permissions(&file)?;
        let result = (|| {
            serde_json::to_writer_pretty(&mut file, value)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, destination)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        lock.unlock()?;
        result
    }

    pub fn delete(&self, resource: &str, id: &str) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = if resource == "test_cases" {
            fs::remove_dir_all(self.test_case_dir(id)?)
        } else {
            fs::remove_file(self.resource_path(resource, id)?)
        };
        lock.unlock()?;
        result
    }

    pub fn save_attachment(&self, id: &str, filename: &str, contents: &[u8]) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let directory = self.test_case_dir(id)?;
        fs::create_dir_all(&directory)?;
        let path = self.attachment_path(id, filename)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        set_private_permissions(&file)?;
        file.write_all(contents)?;
        file.sync_all()?;
        lock.unlock()
    }

    pub fn read_attachment(&self, id: &str, filename: &str) -> io::Result<Vec<u8>> {
        fs::read(self.attachment_path(id, filename)?)
    }

    pub fn delete_attachment(&self, id: &str, filename: &str) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = fs::remove_file(self.attachment_path(id, filename)?);
        lock.unlock()?;
        result
    }

    fn resource_dir(&self, resource: &str) -> PathBuf {
        self.root.join(resource)
    }

    fn acquire_lock(&self) -> io::Result<File> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".tucano.lock"))?;
        lock.lock_exclusive()?;
        Ok(lock)
    }

    fn resource_path(&self, resource: &str, id: &str) -> io::Result<PathBuf> {
        if !RESOURCE_DIRS.contains(&resource) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown resource",
            ));
        }
        validate_component(id)?;
        let path = if resource == "test_cases" {
            self.test_case_dir(id)?.join("test-case.json")
        } else {
            if !id.ends_with(".json") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "resource id must end in .json",
                ));
            }
            self.resource_dir(resource).join(id)
        };
        ensure_within(&self.root, &path)?;
        Ok(path)
    }

    fn test_case_dir(&self, id: &str) -> io::Result<PathBuf> {
        validate_component(id)?;
        let path = self.resource_dir("test_cases").join(id);
        ensure_within(&self.root, &path)?;
        Ok(path)
    }

    fn attachment_path(&self, id: &str, filename: &str) -> io::Result<PathBuf> {
        validate_component(filename)?;
        let path = self.test_case_dir(id)?.join(filename);
        ensure_within(&self.root, &path)?;
        Ok(path)
    }
}

fn validate_component(component: &str) -> io::Result<()> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains('/')
        || component.contains('\\')
        || component.contains('\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid path component",
        ));
    }
    Ok(())
}

fn ensure_within(root: &Path, candidate: &Path) -> io::Result<()> {
    if candidate.parent().and_then(Path::parent).is_none() || !candidate.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path escapes data root",
        ));
    }
    Ok(())
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn set_private_permissions(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o666))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn repository() -> (TempDir, FileRepository) {
        let directory = TempDir::new().expect("temp dir");
        let repository = FileRepository::new(directory.path()).expect("repository");
        (directory, repository)
    }

    #[test]
    fn new_creates_every_resource_directory() {
        let (directory, _repository) = repository();
        for resource in RESOURCE_DIRS {
            assert!(directory.path().join(resource).is_dir(), "{resource}");
        }
    }

    #[test]
    fn persists_json_atomically_and_rejects_traversal() {
        let (_directory, repository) = repository();
        let value = json!({"name": "Checkout"});
        repository
            .write("projects", "checkout.json", &value)
            .expect("write");
        assert_eq!(
            repository.read("projects", "checkout.json").expect("read"),
            value
        );
        assert!(
            repository
                .write("projects", "../escape.json", &value)
                .is_err()
        );
    }

    #[test]
    fn write_leaves_no_temporary_files_behind() {
        let (directory, repository) = repository();
        repository
            .write("projects", "checkout.json", &json!({"name": "Checkout"}))
            .expect("write");

        let leftovers = fs::read_dir(directory.path().join("projects"))
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn write_overwrites_existing_document() {
        let (_directory, repository) = repository();
        repository
            .write("projects", "checkout.json", &json!({"name": "First"}))
            .expect("first write");
        repository
            .write("projects", "checkout.json", &json!({"name": "Second"}))
            .expect("second write");

        assert_eq!(
            repository.read("projects", "checkout.json").expect("read")["name"],
            "Second"
        );
    }

    #[test]
    fn list_returns_sorted_json_files_only() {
        let (directory, repository) = repository();
        repository
            .write("projects", "beta.json", &json!({"name": "Beta"}))
            .expect("write beta");
        repository
            .write("projects", "alpha.json", &json!({"name": "Alpha"}))
            .expect("write alpha");
        fs::write(directory.path().join("projects/notes.txt"), b"ignored").expect("stray file");

        assert_eq!(
            repository.list("projects").expect("list"),
            vec!["alpha.json".to_owned(), "beta.json".to_owned()]
        );
    }

    #[test]
    fn list_is_empty_for_new_storage() {
        let (_directory, repository) = repository();
        assert!(repository.list("test_runs").expect("list").is_empty());
    }

    #[test]
    fn test_cases_are_stored_as_directories() {
        let (directory, repository) = repository();
        repository
            .write("test_cases", "TC-001", &json!({"testCaseId": "TC-001"}))
            .expect("write");

        assert!(
            directory
                .path()
                .join("test_cases/TC-001/test-case.json")
                .is_file()
        );
        assert_eq!(
            repository.list("test_cases").expect("list"),
            vec!["TC-001".to_owned()]
        );
    }

    #[test]
    fn exists_reflects_stored_documents() {
        let (_directory, repository) = repository();
        assert!(!repository.exists("projects", "missing.json").expect("miss"));
        repository
            .write("projects", "found.json", &json!({"name": "Found"}))
            .expect("write");
        assert!(repository.exists("projects", "found.json").expect("hit"));
    }

    #[test]
    fn read_reports_missing_documents() {
        let (_directory, repository) = repository();
        let error = repository
            .read("projects", "missing.json")
            .expect_err("miss");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn read_reports_corrupted_json_as_invalid_data() {
        let (directory, repository) = repository();
        fs::write(directory.path().join("projects/broken.json"), b"{ not json")
            .expect("corrupt file");

        let error = repository
            .read("projects", "broken.json")
            .expect_err("corrupt");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn delete_removes_documents_and_test_case_directories() {
        let (directory, repository) = repository();
        repository
            .write("projects", "checkout.json", &json!({"name": "Checkout"}))
            .expect("write project");
        repository
            .write("test_cases", "TC-001", &json!({"testCaseId": "TC-001"}))
            .expect("write case");
        repository
            .save_attachment("TC-001", "notes.txt", b"evidence")
            .expect("attachment");

        repository
            .delete("projects", "checkout.json")
            .expect("delete project");
        repository
            .delete("test_cases", "TC-001")
            .expect("delete case");

        assert!(!directory.path().join("projects/checkout.json").exists());
        assert!(!directory.path().join("test_cases/TC-001").exists());
    }

    #[test]
    fn delete_reports_missing_documents() {
        let (_directory, repository) = repository();
        let error = repository
            .delete("projects", "missing.json")
            .expect_err("miss");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn unknown_resources_are_rejected() {
        let (_directory, repository) = repository();
        let error = repository
            .read("secrets", "any.json")
            .expect_err("rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn flat_resources_require_a_json_extension() {
        let (_directory, repository) = repository();
        let error = repository
            .write("projects", "checkout", &json!({"name": "Checkout"}))
            .expect_err("rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn hostile_identifiers_are_rejected() {
        let (_directory, repository) = repository();
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
                repository.read("projects", id).is_err(),
                "identifier should be rejected: {id:?}"
            );
        }
    }

    #[test]
    fn attachments_round_trip_and_reject_traversal() {
        let (_directory, repository) = repository();
        repository
            .write("test_cases", "TC-001", &json!({"testCaseId": "TC-001"}))
            .expect("write case");

        repository
            .save_attachment("TC-001", "notes.txt", b"evidence")
            .expect("save");
        assert_eq!(
            repository
                .read_attachment("TC-001", "notes.txt")
                .expect("read"),
            b"evidence"
        );

        assert!(
            repository
                .save_attachment("TC-001", "../escape.txt", b"x")
                .is_err()
        );
        assert!(
            repository
                .read_attachment("TC-001", "../../etc/passwd")
                .is_err()
        );

        repository
            .delete_attachment("TC-001", "notes.txt")
            .expect("delete");
        assert!(repository.read_attachment("TC-001", "notes.txt").is_err());
    }

    #[test]
    fn duplicate_attachment_names_are_rejected() {
        let (_directory, repository) = repository();
        repository
            .write("test_cases", "TC-001", &json!({"testCaseId": "TC-001"}))
            .expect("write case");
        repository
            .save_attachment("TC-001", "notes.txt", b"first")
            .expect("first");

        assert!(
            repository
                .save_attachment("TC-001", "notes.txt", b"second")
                .is_err()
        );
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
                        .write("projects", "shared.json", &json!({"name": index}))
                        .expect("concurrent write");
                })
            })
            .collect::<Vec<_>>();

        for writer in writers {
            writer.join().expect("writer thread");
        }

        let stored = readable.read("projects", "shared.json").expect("read");
        assert!(stored["name"].is_number());
    }

    #[cfg(unix)]
    #[test]
    fn stored_documents_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (directory, repository) = repository();
        repository
            .write("projects", "checkout.json", &json!({"name": "Checkout"}))
            .expect("write");

        let mode = fs::metadata(directory.path().join("projects/checkout.json"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o666);
    }
}
