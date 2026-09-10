use fs2::FileExt;
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::layout::{
    Parent, Placement, attachment_path, case_dir, case_marker, document_path, folder_wire_id,
    node_folder, parent_dir, project_dir, project_marker, root_dir, set_private_permissions,
    suite_dir, suite_marker, unique_suffix,
};
use super::{Repository, Resource};

/// Filesystem-backed [`Repository`]: a folder tree for projects, suites, and
/// cases, one JSON document elsewhere.
#[derive(Clone)]
pub struct FileRepository {
    root: PathBuf,
}

impl FileRepository {
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        for resource in Resource::ROOT_DIRS {
            if let Some(name) = resource.dir_name() {
                fs::create_dir_all(root.join(name))?;
            }
        }
        Ok(Self { root })
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

    /// Path of the document that stores `resource` addressed by `id`.
    fn document(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<PathBuf> {
        match resource {
            Resource::Projects => {
                reject_parent(parent)?;
                project_marker(&self.root, id)
            }
            Resource::Suites => {
                let project = project_parent(parent)?;
                suite_marker(&self.root, project, id)
            }
            Resource::Cases => {
                let parent = required_parent(parent)?;
                case_marker(&self.root, parent, id)
            }
            _ => {
                reject_parent(parent)?;
                document_path(&self.root, resource, id)
            }
        }
    }

    /// Folder that a hierarchy node occupies.
    fn folder(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<PathBuf> {
        match resource {
            Resource::Projects => {
                reject_parent(parent)?;
                project_dir(&self.root, id)
            }
            Resource::Suites => {
                let project = project_parent(parent)?;
                suite_dir(&self.root, project, id)
            }
            Resource::Cases => {
                let parent = required_parent(parent)?;
                case_dir(&self.root, parent, id)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resource is not stored in the project tree",
            )),
        }
    }

    /// Folders below `directory` that hold `marker`, sorted.
    fn child_folders(&self, directory: &Path, marker: &str) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut folders = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
            .filter(|entry| entry.path().join(marker).is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        folders.sort();
        Ok(folders)
    }

    /// Folder name of every project stored in the tree, sorted.
    fn project_folders(&self) -> io::Result<Vec<String>> {
        self.child_folders(&root_dir(&self.root, Resource::Projects)?, "project.json")
    }

    /// Flat `.json` documents of a resource stored below the data root.
    fn list_flat(&self, resource: Resource) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(root_dir(&self.root, resource)?) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut names = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            })
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".json"))
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    fn read_json(&self, path: &Path) -> io::Result<Value> {
        let contents = fs::read_to_string(path)?;
        serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Write a document atomically: a same-directory temporary file, flushed and
    /// synced, then renamed over the destination.
    fn write_json(&self, destination: &Path, value: &Value) -> io::Result<()> {
        let directory = destination.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent")
        })?;
        fs::create_dir_all(directory)?;
        let temporary = directory.join(format!(".tucano-{}.tmp", unique_suffix()));
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

    /// A folder holds one kind of node, so a name taken by a different kind of
    /// child is a collision rather than a second marker in the same folder.
    fn ensure_kind_available(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<()> {
        let folder = self.folder(resource, parent, id)?;
        if !folder.exists() {
            return Ok(());
        }
        for other in [Resource::Projects, Resource::Suites, Resource::Cases] {
            if let Some(name) = other.marker_name()
                && Some(name) != resource.marker_name()
                && folder.join(name).is_file()
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "child name is already taken by another kind of resource",
                ));
            }
        }
        Ok(())
    }

    /// Append an attachment record to the stored case document.
    fn record_attachment(&self, marker: &Path, entry: &Value) -> io::Result<()> {
        let mut document = self.read_json(marker)?;
        let object = document.as_object_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "stored test case is not an object",
            )
        })?;
        let attachments = object
            .entry("attachments")
            .or_insert_with(|| Value::Array(Vec::new()));
        let array = attachments.as_array_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "attachments is not an array")
        })?;
        array.push(entry.clone());
        self.write_json(marker, &document)
    }

    /// Drop an attachment record from the stored case document.
    fn forget_attachment(&self, marker: &Path, filename: &str) -> io::Result<()> {
        let mut document = self.read_json(marker)?;
        if let Some(object) = document.as_object_mut()
            && let Some(attachments) = object.get_mut("attachments").and_then(Value::as_array_mut)
        {
            attachments
                .retain(|entry| entry.get("filename").and_then(Value::as_str) != Some(filename));
            if attachments.is_empty() {
                object.remove("attachments");
            }
        }
        self.write_json(marker, &document)
    }

    fn place_locked(
        &self,
        resource: Resource,
        source: &Parent,
        id: &str,
        target: &Parent,
        mode: Placement,
    ) -> io::Result<()> {
        let from = self.folder(resource, Some(source), id)?;
        let to = self.folder(resource, Some(target), id)?;
        if from == to {
            return match mode {
                Placement::Move => Ok(()),
                Placement::Copy => Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "source and target are the same parent",
                )),
            };
        }
        if !from.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "source does not exist",
            ));
        }
        if to.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target already has a child with this identifier",
            ));
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        match mode {
            Placement::Copy => copy_dir_all(&from, &to),
            Placement::Move => match fs::rename(&from, &to) {
                Ok(()) => Ok(()),
                Err(_) => {
                    copy_dir_all(&from, &to)?;
                    fs::remove_dir_all(&from)
                }
            },
        }
    }
}

impl Repository for FileRepository {
    fn list(&self, resource: Resource) -> io::Result<Vec<String>> {
        let mut ids = match resource {
            Resource::Projects => self
                .project_folders()?
                .iter()
                .map(|folder| folder_wire_id(folder))
                .collect::<Vec<_>>(),
            Resource::Suites => {
                let mut ids = Vec::new();
                for project in self.project_folders()? {
                    let directory = project_dir(&self.root, &folder_wire_id(&project))?;
                    ids.extend(
                        self.child_folders(&directory, "suite.json")?
                            .iter()
                            .map(|folder| folder_wire_id(folder)),
                    );
                }
                ids
            }
            Resource::Cases => {
                let mut ids = Vec::new();
                for project in self.project_folders()? {
                    let directory = project_dir(&self.root, &folder_wire_id(&project))?;
                    ids.extend(self.child_folders(&directory, "test-case.json")?);
                    for suite in self.child_folders(&directory, "suite.json")? {
                        ids.extend(self.child_folders(&directory.join(&suite), "test-case.json")?);
                    }
                }
                ids
            }
            _ => self.list_flat(resource)?,
        };
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    fn locate(&self, resource: Resource, id: &str) -> io::Result<Vec<Parent>> {
        let folder = node_folder(resource, id)?;
        let mut homes = Vec::new();
        match resource {
            Resource::Suites => {
                for project in self.project_folders()? {
                    let project_id = folder_wire_id(&project);
                    if suite_dir(&self.root, &project_id, id)?
                        .join("suite.json")
                        .is_file()
                    {
                        homes.push(Parent::Project(project_id));
                    }
                }
            }
            Resource::Cases => {
                for project in self.project_folders()? {
                    let project_id = folder_wire_id(&project);
                    let directory = project_dir(&self.root, &project_id)?;
                    if directory.join(folder).join("test-case.json").is_file() {
                        homes.push(Parent::Project(project_id.clone()));
                    }
                    for suite in self.child_folders(&directory, "suite.json")? {
                        if directory
                            .join(&suite)
                            .join(folder)
                            .join("test-case.json")
                            .is_file()
                        {
                            homes.push(Parent::Suite {
                                project: project_id.clone(),
                                suite: folder_wire_id(&suite),
                            });
                        }
                    }
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "resource is not stored in the project tree",
                ));
            }
        }
        Ok(homes)
    }

    fn list_children(&self, parent: &Parent, child: Resource) -> io::Result<Vec<String>> {
        let directory = parent_dir(&self.root, parent)?;
        match child {
            Resource::Suites => {
                if !matches!(parent, Parent::Project(_)) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "suites live inside a project",
                    ));
                }
                Ok(self
                    .child_folders(&directory, "suite.json")?
                    .iter()
                    .map(|folder| folder_wire_id(folder))
                    .collect())
            }
            Resource::Cases => self.child_folders(&directory, "test-case.json"),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resource is not stored in the project tree",
            )),
        }
    }

    fn exists_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<bool> {
        Ok(self.document(resource, parent, id)?.is_file())
    }

    fn read_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<Value> {
        self.read_json(&self.document(resource, parent, id)?)
    }

    fn write_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = (|| {
            if resource != Resource::Projects
                && let Some(parent) = parent
            {
                let folder = self.folder(resource, Some(parent), id)?;
                if folder.file_name().and_then(|name| name.to_str()) == Some(parent.marker_name()) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "child name shadows the parent marker",
                    ));
                }
                self.ensure_kind_available(resource, Some(parent), id)?;
            }
            self.write_json(&self.document(resource, parent, id)?, value)
        })();
        lock.unlock()?;
        result
    }

    fn delete_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = if resource.is_hierarchical() {
            fs::remove_dir_all(self.folder(resource, parent, id)?)
        } else {
            fs::remove_file(self.document(resource, parent, id)?)
        };
        lock.unlock()?;
        result
    }

    fn place(
        &self,
        resource: Resource,
        source: &Parent,
        id: &str,
        target: &Parent,
        mode: Placement,
    ) -> io::Result<()> {
        if !matches!(resource, Resource::Suites | Resource::Cases) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "only suites and cases can be placed",
            ));
        }
        if resource == Resource::Suites && !matches!(target, Parent::Project(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a suite can only be placed into a project",
            ));
        }
        let lock = self.acquire_lock()?;
        let result = self.place_locked(resource, source, id, target, mode);
        lock.unlock()?;
        result
    }

    fn save_attachment(
        &self,
        parent: &Parent,
        case: &str,
        filename: &str,
        entry: &Value,
        contents: &[u8],
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = (|| {
            let marker = case_marker(&self.root, parent, case)?;
            if !marker.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "test case does not exist",
                ));
            }
            let path = attachment_path(&self.root, parent, case, filename)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            set_private_permissions(&file)?;
            file.write_all(contents)?;
            file.sync_all()?;
            if let Err(error) = self.record_attachment(&marker, entry) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            Ok(())
        })();
        lock.unlock()?;
        result
    }

    fn read_attachment(&self, parent: &Parent, case: &str, filename: &str) -> io::Result<Vec<u8>> {
        fs::read(attachment_path(&self.root, parent, case, filename)?)
    }

    fn delete_attachment(&self, parent: &Parent, case: &str, filename: &str) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = (|| {
            fs::remove_file(attachment_path(&self.root, parent, case, filename)?)?;
            self.forget_attachment(&case_marker(&self.root, parent, case)?, filename)
        })();
        lock.unlock()?;
        result
    }
}

/// Recursively duplicate a folder, contents and all.
fn copy_dir_all(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Projects are addressed without a parent.
fn reject_parent(parent: Option<&Parent>) -> io::Result<()> {
    if parent.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resource is not stored inside a parent",
        ));
    }
    Ok(())
}

/// A suite's parent is always a project.
fn project_parent(parent: Option<&Parent>) -> io::Result<&str> {
    match parent {
        Some(Parent::Project(project)) => Ok(project),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resource requires a project parent",
        )),
    }
}

/// A case is always owned by a parent.
fn required_parent(parent: Option<&Parent>) -> io::Result<&Parent> {
    parent.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "resource requires a parent"))
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

    fn project(id: &str) -> Parent {
        Parent::Project(id.to_owned())
    }

    fn suite(project_id: &str, suite_id: &str) -> Parent {
        Parent::Suite {
            project: project_id.to_owned(),
            suite: suite_id.to_owned(),
        }
    }

    fn create_project(repository: &FileRepository, id: &str) {
        repository
            .write_at(Resource::Projects, None, id, &json!({"name": id}))
            .expect("project");
    }

    #[test]
    fn new_creates_only_the_root_collections() {
        let (directory, _repository) = repository();
        for resource in Resource::ROOT_DIRS {
            let name = resource.dir_name().expect("root dir");
            assert!(directory.path().join(name).is_dir(), "{name}");
        }
        assert!(!directory.path().join("test_suites").exists());
        assert!(!directory.path().join("test_cases").exists());
    }

    #[test]
    fn a_project_is_a_folder_holding_a_marker() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");

        assert!(
            directory
                .path()
                .join("projects/checkout/project.json")
                .is_file()
        );
        assert_eq!(
            repository
                .read_at(Resource::Projects, None, "checkout.json")
                .expect("read")["name"],
            "checkout.json"
        );
        assert_eq!(
            repository.list(Resource::Projects).expect("list"),
            vec!["checkout.json".to_owned()]
        );
    }

    #[test]
    fn suites_and_cases_live_inside_their_parents() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&project("checkout.json")),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");
        repository
            .write_at(
                Resource::Cases,
                Some(&project("checkout.json")),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("direct case");
        repository
            .write_at(
                Resource::Cases,
                Some(&suite("checkout.json", "smoke.json")),
                "TC-002",
                &json!({"testCaseId": "TC-002"}),
            )
            .expect("suite case");

        assert!(
            directory
                .path()
                .join("projects/checkout/smoke/suite.json")
                .is_file()
        );
        assert!(
            directory
                .path()
                .join("projects/checkout/TC-001/test-case.json")
                .is_file()
        );
        assert!(
            directory
                .path()
                .join("projects/checkout/smoke/TC-002/test-case.json")
                .is_file()
        );

        assert_eq!(
            repository.list(Resource::Suites).expect("suites"),
            vec!["smoke.json".to_owned()]
        );
        assert_eq!(
            repository.list(Resource::Cases).expect("cases"),
            vec!["TC-001".to_owned(), "TC-002".to_owned()]
        );
    }

    #[test]
    fn list_de_duplicates_the_same_identifier_in_several_homes() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        create_project(&repository, "billing.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&project("checkout.json")),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");
        repository
            .write_at(
                Resource::Suites,
                Some(&project("billing.json")),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");

        assert_eq!(
            repository.list(Resource::Suites).expect("suites"),
            vec!["smoke.json".to_owned()],
            "a list never repeats an identifier"
        );
    }

    #[test]
    fn locate_reports_every_home_of_an_identifier() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&project("checkout.json")),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");
        repository
            .write_at(
                Resource::Cases,
                Some(&project("checkout.json")),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("direct case");
        repository
            .write_at(
                Resource::Cases,
                Some(&suite("checkout.json", "smoke.json")),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("suite case");

        assert_eq!(
            repository.locate(Resource::Cases, "TC-001").expect("homes"),
            vec![
                project("checkout.json"),
                suite("checkout.json", "smoke.json"),
            ]
        );
        assert_eq!(
            repository
                .locate(Resource::Suites, "smoke.json")
                .expect("homes"),
            vec![project("checkout.json")]
        );
        assert!(
            repository
                .locate(Resource::Cases, "unknown")
                .expect("homes")
                .is_empty()
        );
    }

    #[test]
    fn list_children_reports_only_the_parents_own_children() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        create_project(&repository, "billing.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&project("checkout.json")),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");
        repository
            .write_at(
                Resource::Cases,
                Some(&project("checkout.json")),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");

        assert_eq!(
            repository
                .list_children(&project("checkout.json"), Resource::Suites)
                .expect("suites"),
            vec!["smoke.json".to_owned()]
        );
        assert_eq!(
            repository
                .list_children(&project("checkout.json"), Resource::Cases)
                .expect("cases"),
            vec!["TC-001".to_owned()]
        );
        assert!(
            repository
                .list_children(&project("billing.json"), Resource::Cases)
                .expect("cases")
                .is_empty()
        );
        assert!(
            repository
                .list_children(&project("checkout.json"), Resource::Runs)
                .is_err()
        );
    }

    #[test]
    fn a_folder_cannot_hold_two_kinds_of_child() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&project("checkout.json")),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");

        let error = repository
            .write_at(
                Resource::Cases,
                Some(&project("checkout.json")),
                "smoke",
                &json!({"testCaseId": "smoke"}),
            )
            .expect_err("collision");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn a_case_cannot_shadow_its_parent_marker() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&project("checkout.json")),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");

        let error = repository
            .write_at(
                Resource::Cases,
                Some(&project("checkout.json")),
                "project.json",
                &json!({"testCaseId": "project.json"}),
            )
            .expect_err("shadows the project marker");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);

        let error = repository
            .write_at(
                Resource::Cases,
                Some(&suite("checkout.json", "smoke.json")),
                "suite.json",
                &json!({"testCaseId": "suite.json"}),
            )
            .expect_err("shadows the suite marker");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn place_copies_a_subtree_without_touching_the_source() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        create_project(&repository, "billing.json");
        let home = project("checkout.json");
        let source = suite("checkout.json", "smoke.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&home),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");
        repository
            .write_at(
                Resource::Cases,
                Some(&source),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");
        repository
            .save_attachment(
                &source,
                "TC-001",
                "notes.txt",
                &json!({"filename": "notes.txt"}),
                b"evidence",
            )
            .expect("attachment");

        repository
            .place(
                Resource::Suites,
                &home,
                "smoke.json",
                &project("billing.json"),
                Placement::Copy,
            )
            .expect("copy");

        let copy = suite("billing.json", "smoke.json");
        assert_eq!(
            repository
                .read_attachment(&copy, "TC-001", "notes.txt")
                .expect("attachment copied"),
            b"evidence"
        );
        assert_eq!(
            repository
                .locate(Resource::Suites, "smoke.json")
                .expect("homes"),
            vec![project("billing.json"), project("checkout.json")]
        );

        repository
            .delete_at(
                Resource::Suites,
                Some(&project("billing.json")),
                "smoke.json",
            )
            .expect("delete the copy");
        assert_eq!(
            repository
                .read_attachment(&source, "TC-001", "notes.txt")
                .expect("source survives"),
            b"evidence"
        );
    }

    #[test]
    fn place_moves_a_subtree_away_from_its_old_parent() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        create_project(&repository, "billing.json");
        let source = project("checkout.json");
        repository
            .write_at(
                Resource::Cases,
                Some(&source),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");

        repository
            .place(
                Resource::Cases,
                &source,
                "TC-001",
                &project("billing.json"),
                Placement::Move,
            )
            .expect("move");

        assert_eq!(
            repository.locate(Resource::Cases, "TC-001").expect("homes"),
            vec![project("billing.json")]
        );
        assert!(
            !repository
                .list_children(&project("checkout.json"), Resource::Cases)
                .expect("cases")
                .contains(&"TC-001".to_owned())
        );
    }

    #[test]
    fn place_onto_an_existing_child_is_rejected() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        create_project(&repository, "billing.json");
        for parent in [project("checkout.json"), project("billing.json")] {
            repository
                .write_at(
                    Resource::Cases,
                    Some(&parent),
                    "TC-001",
                    &json!({"testCaseId": "TC-001"}),
                )
                .expect("case");
        }

        let error = repository
            .place(
                Resource::Cases,
                &project("checkout.json"),
                "TC-001",
                &project("billing.json"),
                Placement::Copy,
            )
            .expect_err("target is taken");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn delete_cascades_through_the_subtree() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let project_id = project("checkout.json");
        let suite_id = suite("checkout.json", "smoke.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&project_id),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");
        repository
            .write_at(
                Resource::Cases,
                Some(&suite_id),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");
        repository
            .save_attachment(
                &suite_id,
                "TC-001",
                "notes.txt",
                &json!({"filename": "notes.txt"}),
                b"evidence",
            )
            .expect("attachment");

        repository
            .delete_at(Resource::Projects, None, "checkout.json")
            .expect("delete project");

        assert!(!directory.path().join("projects/checkout").exists());
        assert!(repository.list(Resource::Cases).expect("cases").is_empty());
    }

    #[test]
    fn attachments_round_trip_and_update_the_case_document() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let parent = project("checkout.json");
        repository
            .write_at(
                Resource::Cases,
                Some(&parent),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");

        let entry = json!({"filename": "1-notes.txt", "originalName": "notes.txt"});
        repository
            .save_attachment(&parent, "TC-001", "1-notes.txt", &entry, b"evidence")
            .expect("save");

        assert_eq!(
            repository
                .read_attachment(&parent, "TC-001", "1-notes.txt")
                .expect("read"),
            b"evidence"
        );
        let stored = repository
            .read_at(Resource::Cases, Some(&parent), "TC-001")
            .expect("case document");
        assert_eq!(stored["attachments"][0], entry);

        repository
            .delete_attachment(&parent, "TC-001", "1-notes.txt")
            .expect("delete");
        assert!(
            repository
                .read_attachment(&parent, "TC-001", "1-notes.txt")
                .is_err()
        );
        let stored = repository
            .read_at(Resource::Cases, Some(&parent), "TC-001")
            .expect("case document");
        assert!(stored.get("attachments").is_none());
    }

    #[test]
    fn attachments_reject_traversal_and_duplicate_names() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let parent = project("checkout.json");
        repository
            .write_at(
                Resource::Cases,
                Some(&parent),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");

        let entry = json!({"filename": "notes.txt"});
        assert!(
            repository
                .save_attachment(&parent, "TC-001", "../escape.txt", &entry, b"x")
                .is_err()
        );
        assert!(
            repository
                .read_attachment(&parent, "TC-001", "../../etc/passwd")
                .is_err()
        );

        repository
            .save_attachment(&parent, "TC-001", "notes.txt", &entry, b"first")
            .expect("first");
        assert!(
            repository
                .save_attachment(&parent, "TC-001", "notes.txt", &entry, b"second")
                .is_err(),
            "a second file with the same name must be rejected"
        );
    }

    #[test]
    fn attachments_need_an_existing_case() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let error = repository
            .save_attachment(
                &project("checkout.json"),
                "missing",
                "notes.txt",
                &json!({"filename": "notes.txt"}),
                b"evidence",
            )
            .expect_err("missing case");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn write_leaves_no_temporary_files_behind() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");

        let leftovers = fs::read_dir(directory.path().join("projects/checkout"))
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn write_overwrites_an_existing_document() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        repository
            .write_at(
                Resource::Projects,
                None,
                "checkout.json",
                &json!({"name": "renamed"}),
            )
            .expect("overwrite");

        assert_eq!(
            repository
                .read_at(Resource::Projects, None, "checkout.json")
                .expect("read")["name"],
            "renamed"
        );
    }

    #[test]
    fn read_reports_missing_documents() {
        let (_directory, repository) = repository();
        let error = repository
            .read_at(Resource::Projects, None, "missing.json")
            .expect_err("miss");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn read_reports_corrupted_json_as_invalid_data() {
        let (directory, repository) = repository();
        fs::create_dir_all(directory.path().join("projects/broken")).expect("folder");
        fs::write(
            directory.path().join("projects/broken/project.json"),
            b"{ not json",
        )
        .expect("corrupt file");

        let error = repository
            .read_at(Resource::Projects, None, "broken.json")
            .expect_err("corrupt");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn exists_reflects_stored_documents() {
        let (_directory, repository) = repository();
        assert!(
            !repository
                .exists_at(Resource::Projects, None, "missing.json")
                .expect("miss")
        );
        create_project(&repository, "checkout.json");
        assert!(
            repository
                .exists_at(Resource::Projects, None, "checkout.json")
                .expect("hit")
        );
    }

    #[test]
    fn flat_resources_keep_their_document_lifecycle() {
        let (directory, repository) = repository();
        let value = json!({"testRunId": "R-001"});
        repository
            .write_at(Resource::Runs, None, "nightly.json", &value)
            .expect("write");
        assert_eq!(
            repository
                .read_at(Resource::Runs, None, "nightly.json")
                .expect("read"),
            value
        );
        assert!(directory.path().join("test_runs/nightly.json").is_file());

        repository
            .delete_at(Resource::Runs, None, "nightly.json")
            .expect("delete");
        assert!(
            repository
                .read_at(Resource::Runs, None, "nightly.json")
                .is_err()
        );
    }

    #[test]
    fn list_is_empty_for_new_storage() {
        let (_directory, repository) = repository();
        for resource in Resource::ALL {
            assert!(
                repository.list(resource).expect("list").is_empty(),
                "{resource:?}"
            );
        }
    }

    #[test]
    fn write_leaves_stray_files_out_of_the_listing() {
        let (directory, repository) = repository();
        create_project(&repository, "alpha.json");
        create_project(&repository, "beta.json");
        fs::write(directory.path().join("projects/notes.txt"), b"ignored").expect("stray file");

        assert_eq!(
            repository.list(Resource::Projects).expect("list"),
            vec!["alpha.json".to_owned(), "beta.json".to_owned()]
        );
    }

    #[test]
    fn flat_resources_require_a_json_extension() {
        let (_directory, repository) = repository();
        let error = repository
            .write_at(
                Resource::Projects,
                None,
                "checkout",
                &json!({"name": "Checkout"}),
            )
            .expect_err("rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn hostile_identifiers_are_rejected() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");

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
                repository.read_at(Resource::Projects, None, id).is_err(),
                "project identifier should be rejected: {id:?}"
            );
            assert!(
                repository
                    .read_at(Resource::Cases, Some(&project("checkout.json")), id)
                    .is_err(),
                "case identifier should be rejected: {id:?}"
            );
        }
    }

    #[test]
    fn a_parent_is_required_where_the_tree_requires_one() {
        let (_directory, repository) = repository();
        assert!(
            repository
                .read_at(Resource::Suites, None, "smoke.json")
                .is_err()
        );
        assert!(repository.read_at(Resource::Cases, None, "TC-001").is_err());
        assert!(
            repository
                .read_at(
                    Resource::Projects,
                    Some(&project("checkout.json")),
                    "checkout.json"
                )
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_document_cannot_leak_a_file_outside_the_root() {
        use std::os::unix::fs::symlink;

        let (directory, repository) = repository();
        let outside = directory
            .path()
            .parent()
            .expect("parent")
            .join("secret.json");
        fs::write(
            &outside,
            serde_json::to_string(&json!({"name": "secret"})).expect("json"),
        )
        .expect("outside file");
        symlink(&outside, directory.path().join("test_runs/evil.json")).expect("symlink");

        let error = repository
            .read_at(Resource::Runs, None, "evil.json")
            .expect_err("symlink escape must be rejected");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
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
                        .write_at(
                            Resource::Projects,
                            None,
                            "shared.json",
                            &json!({"name": index}),
                        )
                        .expect("concurrent write");
                })
            })
            .collect::<Vec<_>>();

        for writer in writers {
            writer.join().expect("writer thread");
        }

        let stored = readable
            .read_at(Resource::Projects, None, "shared.json")
            .expect("read");
        assert!(stored["name"].is_number());
    }

    #[cfg(unix)]
    #[test]
    fn stored_documents_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");

        let mode = fs::metadata(directory.path().join("projects/checkout/project.json"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o666);
    }
}
