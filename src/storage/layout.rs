//! On-disk layout conventions: which resources exist, how their identifiers map
//! to paths, and the guards that keep every path inside the data root.
//!
//! Both storage and domain depend on this module, so identifier and filename
//! rules live here exactly once.
//!
//! Projects, suites, and cases are stored as folders that mirror their real
//! homes. Runs, milestones and configurations are single documents stored in a
//! collection inside the project folder that owns them, so nothing hangs below
//! the data root except `projects/`.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Folder names a project reserves for its own collections.
///
/// A suite or a case folder taking one of these names would be created inside
/// that collection, where its marker would be listed as a document of a
/// resource it is not, so the name is refused instead.
pub const RESERVED_PROJECT_CHILDREN: [&str; 3] = ["test_runs", "milestones", "configurations"];

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

    /// Collections stored directly below the data root. Every other resource
    /// lives inside a project folder, so it has no directory of its own.
    pub const ROOT_DIRS: [Resource; 1] = [Resource::Projects];

    /// Directory name below the data root, for resources held there.
    pub const fn dir_name(self) -> Option<&'static str> {
        match self {
            Resource::Projects => Some("projects"),
            _ => None,
        }
    }

    /// Collection directory name inside the project folder that owns the
    /// document, for the resources stored there.
    pub const fn project_dir_name(self) -> Option<&'static str> {
        match self {
            Resource::Runs => Some("test_runs"),
            Resource::Milestones => Some("milestones"),
            Resource::Configurations => Some("configurations"),
            Resource::Projects | Resource::Suites | Resource::Cases => None,
        }
    }

    /// File that marks a folder as a node of this resource.
    pub const fn marker_name(self) -> Option<&'static str> {
        match self {
            Resource::Projects => Some("project.json"),
            Resource::Suites => Some("suite.json"),
            Resource::Cases => Some("test-case.json"),
            Resource::Runs | Resource::Milestones | Resource::Configurations => None,
        }
    }

    /// Whether the resource is stored as folders inside the project tree.
    pub const fn is_hierarchical(self) -> bool {
        matches!(
            self,
            Resource::Projects | Resource::Suites | Resource::Cases
        )
    }

    /// Whether the resource is stored inside the project that owns it.
    ///
    /// The project folder is the only home: nothing is stored on the document,
    /// and its identifier is unique within that project rather than globally.
    pub const fn is_project_scoped(self) -> bool {
        self.project_dir_name().is_some()
    }

    /// Whether the resource is one flat document rather than a folder.
    pub const fn is_flat(self) -> bool {
        self.is_project_scoped()
    }

    /// Whether an identifier of this resource ends in `.json`.
    ///
    /// Test cases are the single exception: they are addressed by a bare
    /// identifier. Every other resource is reached through a path builder that
    /// insists on the suffix — project-scoped documents through
    /// [`project_document_path`], project and suite folders through
    /// [`node_folder`].
    pub const fn id_requires_json_suffix(self) -> bool {
        !matches!(self, Resource::Cases)
    }
}

/// The parent that owns a stored entity: a project, or a suite inside one.
///
/// A project owns its suites, the cases it holds directly, and every run,
/// milestone and configuration stored in it; a suite owns only cases.
///
/// Identifiers are wire ids, so `checkout.json` names the folder `checkout`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parent {
    Project(String),
    Suite { project: String, suite: String },
}

impl Parent {
    /// Identifier of the project this parent lives in.
    pub fn project(&self) -> &str {
        match self {
            Parent::Project(project) => project,
            Parent::Suite { project, .. } => project,
        }
    }

    /// File name that this parent's own marker occupies, so a child folder of
    /// the same name would shadow it.
    pub const fn marker_name(&self) -> &'static str {
        match self {
            Parent::Project(_) => "project.json",
            Parent::Suite { .. } => "suite.json",
        }
    }
}

/// How an entity is placed into a parent that does not own it yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Duplicate the source subtree; the source keeps its home.
    Copy,
    /// Relocate the source subtree; the target becomes its only home.
    Move,
}

/// Folder name for a wire id: projects and suites drop a trailing `.json`,
/// case identifiers are used verbatim.
pub fn folder_name(id: &str) -> &str {
    id.strip_suffix(".json").unwrap_or(id)
}

/// Wire id for a project or suite folder name.
pub fn folder_wire_id(folder: &str) -> String {
    format!("{folder}.json")
}

/// Folder name a hierarchy node's identifier maps to.
///
/// Projects and suites carry the `.json` suffix their wire ids have; cases keep
/// their identifier verbatim.
pub fn node_folder(resource: Resource, id: &str) -> io::Result<&str> {
    let folder = match resource {
        Resource::Cases => id,
        Resource::Projects | Resource::Suites => id.strip_suffix(".json").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "resource id must end in .json")
        })?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resource is not stored in the project tree",
            ));
        }
    };
    validate_component(folder)?;
    Ok(folder)
}

/// Directory holding a resource collection below the data root.
pub fn root_dir(root: &Path, resource: Resource) -> io::Result<PathBuf> {
    let name = resource.dir_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "resource is not stored below the data root",
        )
    })?;
    let path = root.join(name);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Folder holding a single project.
pub fn project_dir(root: &Path, project_id: &str) -> io::Result<PathBuf> {
    let path =
        root_dir(root, Resource::Projects)?.join(node_folder(Resource::Projects, project_id)?);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Marker document of a project.
pub fn project_marker(root: &Path, project_id: &str) -> io::Result<PathBuf> {
    Ok(project_dir(root, project_id)?.join("project.json"))
}

/// Directory holding one project-scoped collection inside its project folder.
///
/// The directory is not created here: a write makes it on demand, and a read of
/// a project that holds none of these documents answers with an empty listing.
pub fn project_collection_dir(
    root: &Path,
    project_id: &str,
    resource: Resource,
) -> io::Result<PathBuf> {
    let name = resource.project_dir_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "resource is not stored inside a project folder",
        )
    })?;
    let path = project_dir(root, project_id)?.join(name);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Whether `id` is usable as the name of a stored document of `resource`.
///
/// Every path builder insists on this, so a caller that walks the tree instead
/// of building one path — `Repository::locate` — can refuse a hostile or
/// unsuffixed identifier even when no project exists that could hold it.
pub fn validate_document_id(resource: Resource, id: &str) -> io::Result<()> {
    validate_component(id)?;
    if resource.id_requires_json_suffix() && !id.ends_with(".json") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resource id must end in .json",
        ));
    }
    Ok(())
}

/// Path of a project-scoped document addressed by `id`.
pub fn project_document_path(
    root: &Path,
    project_id: &str,
    resource: Resource,
    id: &str,
) -> io::Result<PathBuf> {
    validate_document_id(resource, id)?;
    let path = project_collection_dir(root, project_id, resource)?.join(id);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Folder holding a single suite inside its project.
pub fn suite_dir(root: &Path, project_id: &str, suite_id: &str) -> io::Result<PathBuf> {
    let path = project_dir(root, project_id)?.join(node_folder(Resource::Suites, suite_id)?);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Marker document of a suite.
pub fn suite_marker(root: &Path, project_id: &str, suite_id: &str) -> io::Result<PathBuf> {
    Ok(suite_dir(root, project_id, suite_id)?.join("suite.json"))
}

/// Folder whose children a parent owns.
pub fn parent_dir(root: &Path, parent: &Parent) -> io::Result<PathBuf> {
    match parent {
        Parent::Project(project) => project_dir(root, project),
        Parent::Suite { project, suite } => suite_dir(root, project, suite),
    }
}

/// Marker document of a parent.
pub fn parent_marker(root: &Path, parent: &Parent) -> io::Result<PathBuf> {
    match parent {
        Parent::Project(project) => project_marker(root, project),
        Parent::Suite { project, suite } => suite_marker(root, project, suite),
    }
}

/// Folder holding a single test case inside its parent.
pub fn case_dir(root: &Path, parent: &Parent, case_id: &str) -> io::Result<PathBuf> {
    let path = parent_dir(root, parent)?.join(node_folder(Resource::Cases, case_id)?);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Document of a test case, stored whole inside its folder.
pub fn case_marker(root: &Path, parent: &Parent, case_id: &str) -> io::Result<PathBuf> {
    Ok(case_dir(root, parent, case_id)?.join("test-case.json"))
}

/// Folder holding the immutable revision snapshots of a test case.
///
/// It sits inside the case folder, so the copy, move and delete semantics the
/// real-home tree already defines carry the snapshots with their case.
pub fn revision_dir(root: &Path, parent: &Parent, case_id: &str) -> io::Result<PathBuf> {
    let path = case_dir(root, parent, case_id)?.join("revisions");
    ensure_within(root, &path)?;
    Ok(path)
}

/// Path of one immutable revision snapshot of a test case.
pub fn revision_marker(
    root: &Path,
    parent: &Parent,
    case_id: &str,
    version: u64,
) -> io::Result<PathBuf> {
    let path = revision_dir(root, parent, case_id)?.join(format!("v{version}.json"));
    ensure_within(root, &path)?;
    Ok(path)
}

/// Path of an attachment inside a test case folder.
pub fn attachment_path(
    root: &Path,
    parent: &Parent,
    case_id: &str,
    filename: &str,
) -> io::Result<PathBuf> {
    validate_component(filename)?;
    let path = case_dir(root, parent, case_id)?.join(filename);
    ensure_within(root, &path)?;
    Ok(path)
}

/// Folder holding the attachments of one structured step inside a case folder.
///
/// Step attachments live in a `steps/<index>` subdirectory so the step
/// namespace can never collide with the case-level attachment namespace.
pub fn step_dir(
    root: &Path,
    parent: &Parent,
    case_id: &str,
    step_index: usize,
) -> io::Result<PathBuf> {
    let path = case_dir(root, parent, case_id)?
        .join("steps")
        .join(step_index.to_string());
    ensure_within(root, &path)?;
    Ok(path)
}

/// Path of an attachment inside one structured step of a test case.
pub fn step_attachment_path(
    root: &Path,
    parent: &Parent,
    case_id: &str,
    step_index: usize,
    filename: &str,
) -> io::Result<PathBuf> {
    validate_component(filename)?;
    let path = step_dir(root, parent, case_id, step_index)?.join(filename);
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
///
/// The lexical `starts_with` check is only a fast path: it stops `..` and
/// absolute escapes without touching the filesystem. A symlink planted inside
/// the data tree is invisible to that check, so the already-existing prefix of
/// `candidate` is also canonicalised and re-checked against the canonical root.
/// Symlinks are therefore followed here — and caught — rather than silently
/// followed later by an open, read, or rename.
pub fn ensure_within(root: &Path, candidate: &Path) -> io::Result<()> {
    if candidate.parent().and_then(Path::parent).is_none() || !candidate.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path escapes data root",
        ));
    }

    let canonical_root = root
        .canonicalize()
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "data root does not exist"))?;

    if !resolve_existing_prefix(candidate)?.starts_with(&canonical_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path escapes data root",
        ));
    }
    Ok(())
}

/// Canonicalises the deepest ancestor of `candidate` that already exists and
/// re-appends the not-yet-created tail. The final component(s) may legitimately
/// be absent (a new document or attachment), so they cannot be canonicalised.
fn resolve_existing_prefix(candidate: &Path) -> io::Result<PathBuf> {
    let mut existing = candidate;
    let mut missing: Vec<OsString> = Vec::new();

    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "path has no file name")
                })?;
                missing.push(name.to_os_string());
                existing = existing.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "path has no parent")
                })?;
            }
            Err(error) => return Err(error),
        }
    }

    let mut resolved = existing.canonicalize()?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
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
    use tempfile::TempDir;

    fn suite_parent() -> Parent {
        Parent::Suite {
            project: "checkout.json".to_owned(),
            suite: "smoke.json".to_owned(),
        }
    }

    #[test]
    fn only_projects_are_stored_below_the_data_root() {
        assert_eq!(Resource::ROOT_DIRS, [Resource::Projects]);
        let mut names = Resource::ROOT_DIRS
            .map(|resource| resource.dir_name().expect("root dir"))
            .to_vec();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
        assert!(names.iter().all(|name| !name.is_empty()));
        for resource in Resource::ALL {
            assert_eq!(
                resource.dir_name().is_some(),
                resource == Resource::Projects,
                "{resource:?}"
            );
        }
    }

    #[test]
    fn only_projects_suites_and_cases_live_in_the_tree() {
        for resource in Resource::ALL {
            let hierarchical = matches!(
                resource,
                Resource::Projects | Resource::Suites | Resource::Cases
            );
            assert_eq!(resource.is_hierarchical(), hierarchical, "{resource:?}");
            assert_eq!(resource.is_flat(), !hierarchical, "{resource:?}");
            assert_eq!(resource.is_project_scoped(), !hierarchical, "{resource:?}");
            assert_eq!(
                resource.marker_name().is_some(),
                hierarchical,
                "{resource:?}"
            );
        }
    }

    #[test]
    fn project_scoped_resources_name_their_collection_directory() {
        for (resource, name) in [
            (Resource::Runs, "test_runs"),
            (Resource::Milestones, "milestones"),
            (Resource::Configurations, "configurations"),
        ] {
            assert_eq!(resource.project_dir_name(), Some(name), "{resource:?}");
            assert_eq!(resource.marker_name(), None, "{resource:?}");
            assert!(resource.id_requires_json_suffix(), "{resource:?}");
        }
        for resource in [Resource::Projects, Resource::Suites, Resource::Cases] {
            assert_eq!(resource.project_dir_name(), None, "{resource:?}");
        }
    }

    #[test]
    fn hierarchy_paths_are_built_from_the_layout() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();

        assert_eq!(
            project_dir(root, "checkout.json").expect("project dir"),
            root.join("projects/checkout")
        );
        assert_eq!(
            project_marker(root, "checkout.json").expect("project marker"),
            root.join("projects/checkout/project.json")
        );
        assert_eq!(
            suite_dir(root, "checkout.json", "smoke.json").expect("suite dir"),
            root.join("projects/checkout/smoke")
        );
        assert_eq!(
            suite_marker(root, "checkout.json", "smoke.json").expect("suite marker"),
            root.join("projects/checkout/smoke/suite.json")
        );
    }

    #[test]
    fn a_case_folder_sits_inside_its_parent() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();
        let project = Parent::Project("checkout.json".to_owned());

        assert_eq!(
            case_marker(root, &project, "TC-001").expect("direct case"),
            root.join("projects/checkout/TC-001/test-case.json")
        );
        assert_eq!(
            case_marker(root, &suite_parent(), "TC-001").expect("suite case"),
            root.join("projects/checkout/smoke/TC-001/test-case.json")
        );
        assert_eq!(
            attachment_path(root, &suite_parent(), "TC-001", "notes.txt").expect("attachment"),
            root.join("projects/checkout/smoke/TC-001/notes.txt")
        );
    }

    #[test]
    fn step_attachments_live_in_an_indexed_subfolder() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();

        assert_eq!(
            step_dir(root, &suite_parent(), "TC-001", 0).expect("step dir"),
            root.join("projects/checkout/smoke/TC-001/steps/0")
        );
        assert_eq!(
            step_attachment_path(root, &suite_parent(), "TC-001", 2, "shot.png")
                .expect("step attachment"),
            root.join("projects/checkout/smoke/TC-001/steps/2/shot.png")
        );
    }

    #[test]
    fn revision_snapshots_live_in_a_revisions_subfolder() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();

        assert_eq!(
            revision_dir(root, &suite_parent(), "TC-001").expect("revision dir"),
            root.join("projects/checkout/smoke/TC-001/revisions")
        );
        assert_eq!(
            revision_marker(root, &suite_parent(), "TC-001", 2).expect("revision marker"),
            root.join("projects/checkout/smoke/TC-001/revisions/v2.json")
        );
        assert_eq!(
            revision_marker(
                root,
                &Parent::Project("checkout.json".to_owned()),
                "TC-001",
                1
            )
            .expect("direct revision marker"),
            root.join("projects/checkout/TC-001/revisions/v1.json")
        );
    }

    #[test]
    fn step_attachment_names_may_not_traverse() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();

        for filename in ["../escape", "nested/child.txt", "", "."] {
            let error = step_attachment_path(root, &suite_parent(), "TC-001", 0, filename)
                .expect_err("rejected");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{filename:?}");
        }
    }

    #[test]
    fn case_folders_keep_their_identifier_verbatim() {
        assert_eq!(
            node_folder(Resource::Cases, "TC-001").expect("bare"),
            "TC-001"
        );
        assert_eq!(
            node_folder(Resource::Cases, "TC-001.json").expect("suffixed"),
            "TC-001.json"
        );
    }

    #[test]
    fn hierarchy_nodes_require_a_json_suffix() {
        for resource in [Resource::Projects, Resource::Suites] {
            let error = node_folder(resource, "checkout").expect_err("rejected");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
            assert_eq!(
                node_folder(resource, "checkout.json").expect("accepted"),
                "checkout"
            );
        }
    }

    #[test]
    fn project_scoped_documents_hang_off_their_project_folder() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();

        assert_eq!(
            project_collection_dir(root, "checkout.json", Resource::Runs).expect("runs"),
            root.join("projects/checkout/test_runs")
        );
        assert_eq!(
            project_document_path(root, "checkout.json", Resource::Runs, "nightly.json")
                .expect("run"),
            root.join("projects/checkout/test_runs/nightly.json")
        );
        assert_eq!(
            project_document_path(root, "checkout.json", Resource::Milestones, "v1.0.json")
                .expect("milestone"),
            root.join("projects/checkout/milestones/v1.0.json")
        );
        assert_eq!(
            project_document_path(
                root,
                "checkout.json",
                Resource::Configurations,
                "chrome.json"
            )
            .expect("configuration"),
            root.join("projects/checkout/configurations/chrome.json")
        );
    }

    #[test]
    fn project_scoped_documents_are_addressed_with_a_json_suffix() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();

        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            let error =
                project_document_path(root, "checkout.json", resource, "v1").expect_err("bare");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
        }
    }

    #[test]
    fn a_document_identifier_is_validated_before_any_path_is_built() {
        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            assert!(
                validate_document_id(resource, "nightly.json").is_ok(),
                "{resource:?}"
            );
            for id in [
                "nightly",
                "",
                ".",
                "..",
                "../escape.json",
                "nested/child.json",
            ] {
                let error = validate_document_id(resource, id).expect_err("refused");
                assert_eq!(
                    error.kind(),
                    io::ErrorKind::InvalidInput,
                    "{resource:?} {id:?}"
                );
            }
        }
        // A test case keeps its bare identifier, and still may not traverse.
        assert!(validate_document_id(Resource::Cases, "TC-001").is_ok());
        assert!(validate_document_id(Resource::Cases, "../escape").is_err());
    }

    #[test]
    fn only_project_scoped_resources_have_a_collection_directory() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();

        for resource in [Resource::Projects, Resource::Suites, Resource::Cases] {
            let error =
                project_collection_dir(root, "checkout.json", resource).expect_err("refused");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
        }
    }

    #[test]
    fn a_project_reserves_the_names_of_its_collections() {
        assert_eq!(
            RESERVED_PROJECT_CHILDREN,
            ["test_runs", "milestones", "configurations"]
        );
        // Every reserved name is a collection directory of one resource, so the
        // two lists can never drift apart.
        let mut named = Resource::ALL
            .iter()
            .filter_map(|resource| resource.project_dir_name())
            .collect::<Vec<_>>();
        named.sort_unstable();
        let mut reserved = RESERVED_PROJECT_CHILDREN.to_vec();
        reserved.sort_unstable();
        assert_eq!(named, reserved);
    }

    #[test]
    fn only_test_cases_are_addressed_without_a_json_suffix() {
        assert!(!Resource::Cases.id_requires_json_suffix());
        for resource in [
            Resource::Projects,
            Resource::Suites,
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            assert!(resource.id_requires_json_suffix(), "{resource:?}");
        }
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

        for resource in [Resource::Projects, Resource::Suites, Resource::Cases] {
            for hostile in ["..", "../escape.json", "nested/child.json"] {
                assert!(
                    node_folder(resource, hostile).is_err(),
                    "{resource:?} {hostile:?}"
                );
            }
        }

        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();
        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            for hostile in ["..", "../escape.json", "nested/child.json", "nightly"] {
                assert!(
                    project_document_path(root, "checkout.json", resource, hostile).is_err(),
                    "{resource:?} {hostile:?}"
                );
            }
            for hostile in ["..", "../escape.json", "nested/child.json", "checkout"] {
                assert!(
                    project_collection_dir(root, hostile, resource).is_err(),
                    "{resource:?} {hostile:?}"
                );
            }
        }
    }

    #[test]
    fn paths_outside_the_root_are_rejected() {
        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();
        assert!(ensure_within(root, &root.join("projects/checkout")).is_ok());
        assert!(ensure_within(root, Path::new("/elsewhere/secret")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_escapes_the_root_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();
        let runs = root.join("projects/checkout/test_runs");
        fs::create_dir_all(&runs).expect("runs dir");
        symlink("/etc/passwd", runs.join("evil.json")).expect("symlink");
        let error = project_document_path(root, "checkout.json", Resource::Runs, "evil.json")
            .expect_err("escape");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_collection_directory_that_escapes_the_root_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();
        let project = root.join("projects/checkout");
        fs::create_dir_all(&project).expect("project dir");
        symlink("/etc", project.join("test_runs")).expect("symlink");

        let error = project_document_path(root, "checkout.json", Resource::Runs, "passwd.json")
            .expect_err("escape");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_project_folder_that_escapes_the_root_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().expect("temp dir");
        let root = directory.path();
        let projects = root.join("projects");
        fs::create_dir_all(&projects).expect("projects dir");
        symlink("/etc", projects.join("evil")).expect("symlink");

        let error = project_dir(root, "evil.json").expect_err("escape");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}
