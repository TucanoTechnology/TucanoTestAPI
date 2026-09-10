//! On-disk layout conventions: which resources exist, how their identifiers map
//! to paths, and the guards that keep every path inside the data root.
//!
//! Both storage and domain depend on this module, so identifier and filename
//! rules live here exactly once.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A resource collection known to the storage layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Projects,
    Cases,
    Suites,
    Runs,
    Milestones,
    Configurations,
}

impl Resource {
    /// Every resource the layout knows about.
    pub const ALL: [Resource; 6] = [
        Resource::Projects,
        Resource::Cases,
        Resource::Suites,
        Resource::Runs,
        Resource::Milestones,
        Resource::Configurations,
    ];

    /// Directory name of the collection below the data root.
    pub const fn dir_name(self) -> &'static str {
        match self {
            Resource::Projects => "projects",
            Resource::Cases => "test_cases",
            Resource::Suites => "test_suites",
            Resource::Runs => "test_runs",
            Resource::Milestones => "milestones",
            Resource::Configurations => "configurations",
        }
    }

    /// Test cases are a folder per case; every other resource is one JSON file.
    pub const fn is_directory_backed(self) -> bool {
        matches!(self, Resource::Cases)
    }

    /// Flat resources address their document with a `.json` identifier.
    pub const fn requires_json_suffix(self) -> bool {
        !self.is_directory_backed()
    }

    /// Document name inside a directory-backed resource.
    pub const fn document_name(self) -> &'static str {
        "test-case.json"
    }
}

/// Directory holding a resource collection.
pub fn resource_dir(root: &Path, resource: Resource) -> PathBuf {
    root.join(resource.dir_name())
}

/// Path of a resource document addressed by `id`.
pub fn document_path(root: &Path, resource: Resource, id: &str) -> io::Result<PathBuf> {
    validate_component(id)?;
    let path = if resource.is_directory_backed() {
        test_case_dir(root, id)?.join(resource.document_name())
    } else {
        if resource.requires_json_suffix() && !id.ends_with(".json") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resource id must end in .json",
            ));
        }
        resource_dir(root, resource).join(id)
    };
    ensure_within(root, &path)?;
    Ok(path)
}

/// Folder holding a single test case.
pub fn test_case_dir(root: &Path, id: &str) -> io::Result<PathBuf> {
    validate_component(id)?;
    let path = resource_dir(root, Resource::Cases).join(id);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Path of an attachment inside a test case folder.
pub fn attachment_path(root: &Path, id: &str, filename: &str) -> io::Result<PathBuf> {
    validate_component(filename)?;
    let path = test_case_dir(root, id)?.join(filename);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Reject anything that is not a single, plain path component.
pub fn validate_component(component: &str) -> io::Result<()> {
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

/// Reject a path that would resolve outside the data root.
pub fn ensure_within(root: &Path, candidate: &Path) -> io::Result<()> {
    if candidate.parent().and_then(Path::parent).is_none() || !candidate.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path escapes data root",
        ));
    }
    Ok(())
}

/// Collision-free suffix used for temporary files and derived identifiers.
pub fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Restrict a freshly created file so the host user can read and write it.
pub fn set_private_permissions(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o666))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_resource_has_a_unique_directory_name() {
        let mut names = Resource::ALL.map(Resource::dir_name).to_vec();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
        assert!(names.iter().all(|name| !name.is_empty()));
    }

    #[test]
    fn only_test_cases_are_directory_backed() {
        for resource in Resource::ALL {
            assert_eq!(
                resource.is_directory_backed(),
                resource == Resource::Cases,
                "{resource:?}"
            );
            assert_eq!(
                resource.requires_json_suffix(),
                !resource.is_directory_backed(),
                "{resource:?}"
            );
        }
    }

    #[test]
    fn document_paths_are_built_from_the_layout() {
        let root = Path::new("/data");
        assert_eq!(
            document_path(root, Resource::Projects, "checkout.json").expect("path"),
            Path::new("/data/projects/checkout.json")
        );
        assert_eq!(
            document_path(root, Resource::Cases, "TC-001").expect("path"),
            Path::new("/data/test_cases/TC-001/test-case.json")
        );
        assert_eq!(
            attachment_path(root, "TC-001", "notes.txt").expect("path"),
            Path::new("/data/test_cases/TC-001/notes.txt")
        );
    }

    #[test]
    fn flat_documents_require_a_json_suffix() {
        let error =
            document_path(Path::new("/data"), Resource::Milestones, "v1").expect_err("rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn hostile_components_are_rejected() {
        for component in [
            "",
            ".",
            "..",
            "../escape.json",
            "nested/child.json",
            "back\\slash.json",
            "/absolute.json",
        ] {
            assert!(validate_component(component).is_err(), "{component:?}");
        }
        assert!(validate_component("TC-001.json").is_ok());
    }

    #[test]
    fn paths_outside_the_root_are_rejected() {
        assert!(ensure_within(Path::new("/data"), Path::new("/data/projects/a.json")).is_ok());
        assert!(ensure_within(Path::new("/data"), Path::new("/elsewhere/secret")).is_err());
    }
}
