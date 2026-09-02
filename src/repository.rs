use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const RESOURCE_DIRS: [&str; 4] = ["projects", "test_cases", "test_suites", "test_runs"];

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
        result
    }

    pub fn delete(&self, resource: &str, id: &str) -> io::Result<()> {
        if resource == "test_cases" {
            fs::remove_dir_all(self.test_case_dir(id)?)
        } else {
            fs::remove_file(self.resource_path(resource, id)?)
        }
    }

    pub fn save_attachment(&self, id: &str, filename: &str, contents: &[u8]) -> io::Result<()> {
        let directory = self.test_case_dir(id)?;
        fs::create_dir_all(&directory)?;
        let path = self.attachment_path(id, filename)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        set_private_permissions(&file)?;
        file.write_all(contents)?;
        file.sync_all()
    }

    pub fn read_attachment(&self, id: &str, filename: &str) -> io::Result<Vec<u8>> {
        fs::read(self.attachment_path(id, filename)?)
    }

    pub fn delete_attachment(&self, id: &str, filename: &str) -> io::Result<()> {
        fs::remove_file(self.attachment_path(id, filename)?)
    }

    fn resource_dir(&self, resource: &str) -> PathBuf {
        self.root.join(resource)
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
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_json_atomically_and_rejects_traversal() {
        let root = std::env::temp_dir().join(format!("tucano-repo-{}", unique_suffix()));
        let repository = FileRepository::new(&root).expect("repository");
        let value = serde_json::json!({"name": "Checkout"});
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
        fs::remove_dir_all(root).expect("cleanup");
    }
}
