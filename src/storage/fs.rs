use fs2::FileExt;
use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::layout::{
    Parent, Placement, RESERVED_PROJECT_CHILDREN, attachment_path, case_dir, case_marker,
    folder_wire_id, node_folder, parent_dir, project_collection_dir, project_dir,
    project_document_path, project_marker, revision_dir, revision_marker, root_dir,
    set_private_permissions, step_attachment_path, suite_dir, suite_marker, unique_suffix,
    validate_document_id,
};
use super::{Repository, Resource, StorageProbe};

/// Filesystem-backed [`Repository`]: a folder tree for projects, suites, and
/// cases, and one JSON document per run, milestone and configuration inside the
/// project folder that owns it.
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
        refuse_legacy_layout(&root)?;
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
            Resource::Runs | Resource::Milestones | Resource::Configurations => {
                let project = project_parent(parent)?;
                project_document_path(&self.root, project, resource, id)
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

    /// Names of the `.json` documents inside a project-scoped collection
    /// directory, sorted. A collection a project does not hold is empty rather
    /// than an error, and the atomic-write temporary files are left out.
    fn collection_documents(&self, directory: &Path) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(directory) {
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

    /// Collection directory of a project-scoped resource, per project folder.
    fn collection_dirs(&self, resource: Resource) -> io::Result<Vec<PathBuf>> {
        self.project_folders()?
            .iter()
            .map(|project| project_collection_dir(&self.root, &folder_wire_id(project), resource))
            .collect()
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
        self.ensure_name_not_reserved(resource, parent, id)?;
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

    /// A project folder reserves the names of its own collections.
    ///
    /// A suite or a case folder taking one would be created *inside* that
    /// collection, where its marker would be listed as a document of a resource
    /// it is not, so the name is refused whether or not anything is stored
    /// there yet.
    fn ensure_name_not_reserved(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<()> {
        if !matches!(parent, Some(Parent::Project(_)))
            || !matches!(resource, Resource::Suites | Resource::Cases)
        {
            return Ok(());
        }
        if RESERVED_PROJECT_CHILDREN.contains(&node_folder(resource, id)?) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "child name is already taken by another kind of resource",
            ));
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

    /// Append an attachment record to one structured step of the stored case
    /// document.
    fn record_step_attachment(
        &self,
        marker: &Path,
        step_index: usize,
        entry: &Value,
    ) -> io::Result<()> {
        let mut document = self.read_json(marker)?;
        {
            let step = step_object_mut(&mut document, step_index)?;
            let attachments = step
                .entry("attachments")
                .or_insert_with(|| Value::Array(Vec::new()));
            let array = attachments.as_array_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "attachments is not an array")
            })?;
            array.push(entry.clone());
        }
        self.write_json(marker, &document)
    }

    /// Drop an attachment record from one structured step of the stored case
    /// document.
    ///
    /// A step that is absent, out of range, or a plain string is left untouched
    /// and reported as success: callers only forget attachments for a step that
    /// resolved earlier, so an unreachable step needs no write.
    fn forget_step_attachment(
        &self,
        marker: &Path,
        step_index: usize,
        filename: &str,
    ) -> io::Result<()> {
        let mut document = self.read_json(marker)?;
        let Some(step) = document
            .get_mut("steps")
            .and_then(Value::as_array_mut)
            .and_then(|steps| steps.get_mut(step_index))
            .and_then(Value::as_object_mut)
        else {
            return Ok(());
        };
        let mut emptied = false;
        if let Some(attachments) = step.get_mut("attachments").and_then(Value::as_array_mut) {
            attachments
                .retain(|entry| entry.get("filename").and_then(Value::as_str) != Some(filename));
            emptied = attachments.is_empty();
        }
        if emptied {
            step.remove("attachments");
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
        self.ensure_name_not_reserved(resource, Some(target), id)?;
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
            Resource::Runs | Resource::Milestones | Resource::Configurations => {
                let mut ids = Vec::new();
                for directory in self.collection_dirs(resource)? {
                    ids.extend(self.collection_documents(&directory)?);
                }
                ids
            }
        };
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    fn locate(&self, resource: Resource, id: &str) -> io::Result<Vec<Parent>> {
        let mut homes = Vec::new();
        if resource.is_project_scoped() {
            // Refuse an unusable identifier before the walk: with no project to
            // build a path against, nothing else would check it.
            validate_document_id(resource, id)?;
            // Every project whose collection holds the document owns an
            // occurrence; `project_folders` is sorted, so the order is stable.
            for project in self.project_folders()? {
                let project_id = folder_wire_id(&project);
                if project_document_path(&self.root, &project_id, resource, id)?.is_file() {
                    homes.push(Parent::Project(project_id));
                }
            }
            return Ok(homes);
        }
        let folder = node_folder(resource, id)?;
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
        if child.is_project_scoped() {
            let Parent::Project(project) = parent else {
                return Err(not_a_project_home(child));
            };
            let directory = project_collection_dir(&self.root, project, child)?;
            return self.collection_documents(&directory);
        }
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
            // Both guards are about a folder a child would occupy, so only the
            // resources stored as folders inside a parent are checked.
            if matches!(resource, Resource::Suites | Resource::Cases)
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

    fn save_revision(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
        value: &Value,
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
            let revision = revision_marker(&self.root, parent, case, version)?;
            if revision.exists() {
                // A revision snapshot is immutable: an existing one is never rewritten.
                return Ok(());
            }
            self.write_json(&revision, value)
        })();
        lock.unlock()?;
        result
    }

    fn list_revisions(&self, parent: &Parent, case: &str) -> io::Result<Vec<u64>> {
        let directory = revision_dir(&self.root, parent, case)?;
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut versions = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            })
            .filter_map(|entry| entry.file_name().to_str().and_then(revision_number))
            .collect::<Vec<_>>();
        versions.sort_unstable();
        Ok(versions)
    }

    fn read_revision(&self, parent: &Parent, case: &str, version: u64) -> io::Result<Value> {
        self.read_json(&revision_marker(&self.root, parent, case, version)?)
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

    fn save_step_attachment(
        &self,
        parent: &Parent,
        case: &str,
        step_index: usize,
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
            let path = step_attachment_path(&self.root, parent, case, step_index, filename)?;
            if let Some(directory) = path.parent() {
                fs::create_dir_all(directory)?;
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            set_private_permissions(&file)?;
            file.write_all(contents)?;
            file.sync_all()?;
            if let Err(error) = self.record_step_attachment(&marker, step_index, entry) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            Ok(())
        })();
        lock.unlock()?;
        result
    }

    fn delete_step_attachment(
        &self,
        parent: &Parent,
        case: &str,
        step_index: usize,
        filename: &str,
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = (|| {
            fs::remove_file(step_attachment_path(
                &self.root, parent, case, step_index, filename,
            )?)?;
            self.forget_step_attachment(
                &case_marker(&self.root, parent, case)?,
                step_index,
                filename,
            )
        })();
        lock.unlock()?;
        result
    }

    fn probe_readiness(&self) -> StorageProbe {
        if !self.root.is_dir() {
            return StorageProbe::UNREACHABLE;
        }
        // The newest write is read before the writability check writes its
        // scratch file, so a store that is only ever polled never looks busy.
        let last_write_unix = newest_mtime(&self.root);
        let (lockable, lock_held) = probe_lock(&self.root);
        StorageProbe {
            exists: true,
            writable: probe_writable(&self.root),
            lockable,
            lock_held,
            last_write_unix,
        }
    }
}

/// Whether a scratch file can be created, written and removed in `root`, which
/// is the whole of what persisting a document needs.
///
/// The scratch file follows the atomic-write temporary convention, so a probe
/// interrupted part-way can never be mistaken for a stored document.
fn probe_writable(root: &Path) -> bool {
    let scratch = root.join(format!(".tucano-{}.tmp", unique_suffix()));
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&scratch)
    {
        Ok(mut file) => {
            let wrote = file.write_all(b"probe").is_ok();
            drop(file);
            let _ = fs::remove_file(&scratch);
            wrote
        }
        Err(_) => false,
    }
}

/// Whether the write lock is available, and whether someone else holds it.
///
/// A lock a peer holds right now is not a fault: the store is serialising
/// writers exactly as designed, so it reports as available *and* held rather
/// than as unavailable. Only a lock that cannot be taken for some other reason
/// means the store is not lockable.
fn probe_lock(root: &Path) -> (bool, bool) {
    let Ok(lock) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".tucano.lock"))
    else {
        return (false, false);
    };
    match lock.try_lock_exclusive() {
        Ok(()) => (lock.unlock().is_ok(), false),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => (true, true),
        Err(_) => (false, false),
    }
}

/// Newest modification time under the data root, one level deep.
///
/// Walking the whole tree would cost as much as the work a probe is meant to
/// precede; the root and its immediate entries are enough to tell a live volume
/// from a cold or detached one.
fn newest_mtime(root: &Path) -> Option<u64> {
    let mut newest = mtime_seconds(root);
    for entry in fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        if let Some(seconds) = mtime_seconds(&entry.path()) {
            newest = Some(newest.map_or(seconds, |current| current.max(seconds)));
        }
    }
    newest
}

fn mtime_seconds(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs())
}

/// Reach the object of one structured step inside a stored case document.
///
/// A missing `steps` array, an out-of-range index, or a plain string step are
/// all reported as invalid data: a step attachment is only recorded after the
/// step was resolved and confirmed structured.
fn step_object_mut(document: &mut Value, step_index: usize) -> io::Result<&mut Map<String, Value>> {
    document
        .get_mut("steps")
        .and_then(Value::as_array_mut)
        .and_then(|steps| steps.get_mut(step_index))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "stored test case has no structured step at this index",
            )
        })
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

/// A suite, a run, a milestone and a configuration always live in a project.
fn project_parent(parent: Option<&Parent>) -> io::Result<&str> {
    match parent {
        Some(Parent::Project(project)) => Ok(project),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resource requires a project parent",
        )),
    }
}

/// The refusal a parent that cannot own a project-scoped document answers.
fn not_a_project_home(resource: Resource) -> io::Error {
    let noun = match resource {
        Resource::Runs => "runs",
        Resource::Milestones => "milestones",
        Resource::Configurations => "configurations",
        Resource::Projects | Resource::Suites | Resource::Cases => "these resources",
    };
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{noun} live inside a project"),
    )
}

/// A case is always owned by a parent.
fn required_parent(parent: Option<&Parent>) -> io::Result<&Parent> {
    parent.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "resource requires a parent"))
}

/// The revision number a snapshot file name (`v1.json`) stands for.
fn revision_number(name: &str) -> Option<u64> {
    let digits = name.strip_prefix('v')?.strip_suffix(".json")?;
    digits.parse::<u64>().ok()
}

/// Refuse a data root that still holds the pre-v3 flat collections.
///
/// Layout v3 stores runs, milestones and configurations inside the project
/// folder that owns them, so a root-level collection is never read or listed.
/// Refusing to open such a root is what keeps that from being a silent `[]`.
///
/// The check is read-only: it counts the `*.json` documents directly inside
/// each former root collection and reports the ones that hold any. A name
/// beginning with `.` is skipped, which is the atomic-write temporary pattern
/// `write_json` uses (`.tucano-<suffix>.tmp`); a subdirectory, a symlink and
/// any other file are not documents and are left alone. An absent directory is
/// a fresh volume and an empty one is every volume that predates the change,
/// so neither is an error.
fn refuse_legacy_layout(root: &Path) -> io::Result<()> {
    let mut offenders: Vec<(&str, usize)> = Vec::new();
    for collection in RESERVED_PROJECT_CHILDREN {
        let directory = root.join(collection);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let documents = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            })
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".json") && !name.starts_with('.'))
            .count();
        if documents > 0 {
            offenders.push((collection, documents));
        }
    }
    if offenders.is_empty() {
        return Ok(());
    }
    let held = offenders
        .iter()
        .map(|(collection, count)| format!("{collection}/ holds {count} document(s)"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "legacy flat storage layout detected: {held}; layout v3 stores runs, milestones and \
             configurations inside their project folder — move each document into \
             projects/<project>/<collection>/ and restart (docs/deployment/deployment-guide.md)"
        ),
    ))
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
        for name in RESERVED_PROJECT_CHILDREN {
            assert!(
                !directory.path().join(name).exists(),
                "{name} is a project collection, not a root collection"
            );
        }
    }

    #[test]
    fn a_fresh_root_holds_only_projects() {
        let (directory, _repository) = repository();

        let mut created = fs::read_dir(directory.path())
            .expect("root")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        created.sort();
        assert_eq!(created, vec!["projects".to_owned()]);
    }

    #[test]
    fn empty_legacy_collections_do_not_refuse_the_root() {
        let directory = TempDir::new().expect("temp dir");
        for name in RESERVED_PROJECT_CHILDREN {
            fs::create_dir_all(directory.path().join(name)).expect("legacy collection");
        }

        FileRepository::new(directory.path()).expect("an empty legacy collection is not an error");
    }

    #[test]
    fn a_legacy_document_refuses_the_root_and_names_only_its_collection() {
        let directory = TempDir::new().expect("temp dir");
        let legacy = directory.path().join("test_runs");
        fs::create_dir_all(&legacy).expect("legacy collection");
        fs::write(
            legacy.join("nightly.json"),
            b"{\"testRunId\": \"nightly.json\"}\n",
        )
        .expect("legacy document");

        let error = FileRepository::new(directory.path())
            .err()
            .expect("refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let message = error.to_string();
        assert_eq!(
            message,
            "legacy flat storage layout detected: test_runs/ holds 1 document(s); layout v3 \
             stores runs, milestones and configurations inside their project folder — move each \
             document into projects/<project>/<collection>/ and restart \
             (docs/deployment/deployment-guide.md)"
        );
        assert!(!message.contains("milestones/"), "{message}");
        assert!(!message.contains("configurations/"), "{message}");
    }

    #[test]
    fn a_legacy_document_in_two_collections_names_both_in_layout_order() {
        let directory = TempDir::new().expect("temp dir");
        fs::create_dir_all(directory.path().join("test_runs")).expect("legacy collection");
        fs::create_dir_all(directory.path().join("milestones")).expect("legacy collection");
        fs::write(
            directory.path().join("test_runs/nightly.json"),
            b"{\"testRunId\": \"nightly.json\"}\n",
        )
        .expect("legacy document");
        fs::write(
            directory.path().join("test_runs/weekly.json"),
            b"{\"testRunId\": \"weekly.json\"}\n",
        )
        .expect("legacy document");
        fs::write(
            directory.path().join("milestones/v1.0.json"),
            b"{\"milestoneId\": \"v1.0.json\"}\n",
        )
        .expect("legacy document");

        let error = FileRepository::new(directory.path())
            .err()
            .expect("refused");
        let message = error.to_string();
        assert!(
            message.contains("test_runs/ holds 2 document(s), milestones/ holds 1 document(s)"),
            "{message}"
        );
        assert!(!message.contains("configurations/"), "{message}");
    }

    #[test]
    fn a_refused_root_leaves_the_legacy_document_untouched() {
        let directory = TempDir::new().expect("temp dir");
        let document = directory.path().join("test_runs/nightly.json");
        let contents = b"{\n  \"testRunId\": \"nightly.json\"\n}\n";
        fs::create_dir_all(document.parent().expect("parent")).expect("legacy collection");
        fs::write(&document, contents).expect("legacy document");

        FileRepository::new(directory.path())
            .err()
            .expect("refused");

        assert_eq!(
            fs::read(&document).expect("still on disk"),
            contents,
            "the refusal is read-only"
        );
    }

    #[test]
    fn a_legacy_temporary_file_does_not_refuse_the_root() {
        let directory = TempDir::new().expect("temp dir");
        fs::create_dir_all(directory.path().join("test_runs")).expect("legacy collection");
        fs::write(
            directory
                .path()
                .join("test_runs/.tucano-1700000000000000000.tmp"),
            b"{\"testRunId\": \"half-written.json\"}\n",
        )
        .expect("temporary file");

        FileRepository::new(directory.path())
            .expect("an atomic-write temporary file is not a document");
    }

    #[test]
    fn a_subdirectory_in_a_legacy_collection_does_not_refuse_the_root() {
        let directory = TempDir::new().expect("temp dir");
        let nested = directory.path().join("test_runs/nested");
        fs::create_dir_all(&nested).expect("nested directory");
        fs::write(
            nested.join("nightly.json"),
            b"{\"testRunId\": \"nightly.json\"}\n",
        )
        .expect("nested document");

        FileRepository::new(directory.path()).expect("a directory is not a document");
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
        let (directory, repository) = repository();
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
                .expect("runs")
                .is_empty(),
            "a project that holds no run lists none, and its collection folder is not created"
        );
        assert!(
            !directory
                .path()
                .join("projects/checkout/test_runs")
                .exists()
        );
    }

    #[test]
    fn project_scoped_documents_live_in_their_project_collection() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let home = project("checkout.json");

        for (resource, id) in [
            (Resource::Runs, "nightly.json"),
            (Resource::Milestones, "v1.0.json"),
            (Resource::Configurations, "chrome.json"),
        ] {
            repository
                .write_at(resource, Some(&home), id, &json!({ "name": id }))
                .expect("write");
            assert_eq!(
                repository.read_at(resource, Some(&home), id).expect("read")["name"],
                json!(id),
                "{resource:?}"
            );
            assert!(
                repository
                    .exists_at(resource, Some(&home), id)
                    .expect("exists"),
                "{resource:?}"
            );
            assert_eq!(
                repository.list_children(&home, resource).expect("children"),
                vec![id.to_owned()],
                "{resource:?}"
            );
            assert_eq!(
                repository.list(resource).expect("list"),
                vec![id.to_owned()],
                "{resource:?}"
            );
        }

        assert!(
            directory
                .path()
                .join("projects/checkout/test_runs/nightly.json")
                .is_file()
        );
        assert!(
            directory
                .path()
                .join("projects/checkout/milestones/v1.0.json")
                .is_file()
        );
        assert!(
            directory
                .path()
                .join("projects/checkout/configurations/chrome.json")
                .is_file()
        );
    }

    #[test]
    fn a_project_scoped_document_requires_a_project_parent() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let home = project("checkout.json");
        repository
            .write_at(
                Resource::Suites,
                Some(&home),
                "smoke.json",
                &json!({"name": "smoke"}),
            )
            .expect("suite");
        let source = suite("checkout.json", "smoke.json");

        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            assert!(
                repository.read_at(resource, None, "nightly.json").is_err(),
                "{resource:?} is never addressed without a parent"
            );
            for parent in [Some(&source), None] {
                let error = repository
                    .write_at(resource, parent, "nightly.json", &json!({}))
                    .expect_err("a suite is not a home");
                assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
            }
            let error = repository
                .list_children(&source, resource)
                .expect_err("a suite owns no collection");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
            let error = repository
                .locate(resource, "nightly")
                .expect_err("identifier without the suffix");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
        }
    }

    #[test]
    fn locate_reports_every_project_holding_the_same_identifier() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        create_project(&repository, "billing.json");
        for parent in [project("checkout.json"), project("billing.json")] {
            repository
                .write_at(
                    Resource::Runs,
                    Some(&parent),
                    "nightly.json",
                    &json!({"name": "nightly"}),
                )
                .expect("run");
        }

        assert_eq!(
            repository
                .locate(Resource::Runs, "nightly.json")
                .expect("homes"),
            vec![project("billing.json"), project("checkout.json")],
            "homes come back in the stable order the project listing has"
        );
        assert_eq!(
            repository.list(Resource::Runs).expect("runs"),
            vec!["nightly.json".to_owned()],
            "a global listing de-duplicates, exactly as it does for suites"
        );
        assert_eq!(
            repository
                .list_children(&project("checkout.json"), Resource::Runs)
                .expect("runs"),
            vec!["nightly.json".to_owned()],
            "a parent-scoped listing names that project's own occurrence"
        );
        assert!(
            repository
                .locate(Resource::Runs, "missing.json")
                .expect("homes")
                .is_empty()
        );
    }

    #[test]
    fn locate_refuses_an_unusable_identifier_with_no_project_to_hold_it() {
        let (_directory, repository) = repository();
        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            for id in [
                "nightly",
                "",
                ".",
                "..",
                "../escape.json",
                "nested/child.json",
            ] {
                let error = repository.locate(resource, id).expect_err("refused");
                assert_eq!(
                    error.kind(),
                    io::ErrorKind::InvalidInput,
                    "{resource:?} {id:?} is refused rather than reported missing"
                );
            }
            assert!(
                repository
                    .locate(resource, "nightly.json")
                    .expect("a usable identifier")
                    .is_empty()
            );
        }
    }

    #[test]
    fn a_project_reserves_the_names_of_its_collections() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let home = project("checkout.json");

        for name in RESERVED_PROJECT_CHILDREN {
            let error = repository
                .write_at(
                    Resource::Suites,
                    Some(&home),
                    &format!("{name}.json"),
                    &json!({"name": name}),
                )
                .expect_err("a suite may not take a collection name");
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{name}");

            let error = repository
                .write_at(
                    Resource::Cases,
                    Some(&home),
                    name,
                    &json!({"testCaseId": name}),
                )
                .expect_err("a case may not take a collection name");
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{name}");
        }
    }

    #[test]
    fn the_reservation_only_guards_the_project_folder() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let home = project("checkout.json");
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
                Some(&suite("checkout.json", "smoke.json")),
                "test_runs",
                &json!({"testCaseId": "test_runs"}),
            )
            .expect("inside a suite the name is free");
        assert_eq!(
            repository
                .list_children(&suite("checkout.json", "smoke.json"), Resource::Cases)
                .expect("cases"),
            vec!["test_runs".to_owned()]
        );
    }

    #[test]
    fn placing_a_reserved_name_into_a_project_is_refused() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");
        create_project(&repository, "billing.json");
        // A folder that predates the reservation, sitting where checkout's run
        // collection lives.
        let legacy = directory.path().join("projects/checkout/test_runs");
        fs::create_dir_all(&legacy).expect("folder");
        fs::write(legacy.join("suite.json"), b"{\"name\": \"legacy\"}").expect("marker");

        let error = repository
            .place(
                Resource::Suites,
                &project("checkout.json"),
                "test_runs.json",
                &project("billing.json"),
                Placement::Move,
            )
            .expect_err("reserved target name");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
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
            .write_at(
                Resource::Runs,
                Some(&project_id),
                "nightly.json",
                &json!({"name": "nightly"}),
            )
            .expect("run");

        repository
            .delete_at(Resource::Projects, None, "checkout.json")
            .expect("delete project");

        assert!(!directory.path().join("projects/checkout").exists());
        assert!(repository.list(Resource::Cases).expect("cases").is_empty());
        assert!(
            repository.list(Resource::Runs).expect("runs").is_empty(),
            "a project's runs go with it"
        );
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
    fn revision_snapshots_are_listed_ascending_and_read_back_by_version() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");
        repository
            .write_at(
                Resource::Cases,
                Some(&project("checkout.json")),
                "TC-001",
                &json!({"testCaseId": "TC-001"}),
            )
            .expect("case");

        // A case that has never been revised has no `revisions/` folder, and
        // that is an empty history rather than an error.
        assert!(
            repository
                .list_revisions(&project("checkout.json"), "TC-001")
                .expect("history")
                .is_empty()
        );

        // Written out of order, so the listing has to sort rather than trust
        // the directory's order.
        for version in [3, 1, 2] {
            repository
                .save_revision(
                    &project("checkout.json"),
                    "TC-001",
                    version,
                    &json!({"testCaseId": "TC-001", "version": version}),
                )
                .expect("snapshot");
        }

        // Only `v{number}.json` files are snapshots: a stray file this API did
        // not write and a directory that happens to share the name are not.
        let revisions = directory.path().join("projects/checkout/TC-001/revisions");
        std::fs::write(revisions.join("notes.txt"), b"not a snapshot").expect("stray file");
        std::fs::create_dir(revisions.join("v4.json")).expect("a directory is not a snapshot");

        assert_eq!(
            repository
                .list_revisions(&project("checkout.json"), "TC-001")
                .expect("history"),
            vec![1, 2, 3]
        );
        assert_eq!(
            repository
                .read_revision(&project("checkout.json"), "TC-001", 2)
                .expect("snapshot")["version"],
            json!(2)
        );

        // A snapshot is immutable, so re-saving a version leaves the first one.
        repository
            .save_revision(
                &project("checkout.json"),
                "TC-001",
                1,
                &json!({"testCaseId": "TC-001", "version": 99}),
            )
            .expect("re-save");
        assert_eq!(
            repository
                .read_revision(&project("checkout.json"), "TC-001", 1)
                .expect("snapshot")["version"],
            json!(1)
        );

        // A version the case never recorded is a missing document.
        assert!(
            repository
                .read_revision(&project("checkout.json"), "TC-001", 4)
                .is_err()
        );

        // A snapshot needs its case, so an unknown one cannot record a revision.
        assert!(
            repository
                .save_revision(
                    &project("checkout.json"),
                    "TC-404",
                    1,
                    &json!({"testCaseId": "TC-404"}),
                )
                .is_err()
        );
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
    fn project_scoped_documents_keep_their_document_lifecycle() {
        let (directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let home = project("checkout.json");
        let value = json!({"testRunId": "R-001"});
        repository
            .write_at(Resource::Runs, Some(&home), "nightly.json", &value)
            .expect("write");
        assert_eq!(
            repository
                .read_at(Resource::Runs, Some(&home), "nightly.json")
                .expect("read"),
            value
        );
        assert!(
            directory
                .path()
                .join("projects/checkout/test_runs/nightly.json")
                .is_file()
        );

        repository
            .write_at(
                Resource::Runs,
                Some(&home),
                "nightly.json",
                &json!({"testRunId": "R-002"}),
            )
            .expect("overwrite");
        assert_eq!(
            repository
                .read_at(Resource::Runs, Some(&home), "nightly.json")
                .expect("read")["testRunId"],
            "R-002"
        );

        repository
            .delete_at(Resource::Runs, Some(&home), "nightly.json")
            .expect("delete");
        assert!(
            repository
                .read_at(Resource::Runs, Some(&home), "nightly.json")
                .is_err()
        );
        assert!(
            directory
                .path()
                .join("projects/checkout/test_runs")
                .is_dir(),
            "deleting one document leaves the collection, and the project, alone"
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
    fn documents_require_a_json_extension() {
        let (_directory, repository) = repository();
        create_project(&repository, "checkout.json");
        let error = repository
            .write_at(
                Resource::Projects,
                None,
                "checkout",
                &json!({"name": "Checkout"}),
            )
            .expect_err("rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

        for resource in [
            Resource::Runs,
            Resource::Milestones,
            Resource::Configurations,
        ] {
            let error = repository
                .write_at(
                    resource,
                    Some(&project("checkout.json")),
                    "nightly",
                    &json!({"name": "nightly"}),
                )
                .expect_err("rejected");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{resource:?}");
        }
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
            for resource in [
                Resource::Runs,
                Resource::Milestones,
                Resource::Configurations,
            ] {
                assert!(
                    repository
                        .read_at(resource, Some(&project("checkout.json")), id)
                        .is_err(),
                    "{resource:?} identifier should be rejected: {id:?}"
                );
                assert!(
                    repository
                        .read_at(resource, Some(&project(id)), "nightly.json")
                        .is_err(),
                    "{resource:?} should be rejected in a hostile project: {id:?}"
                );
            }
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
                .read_at(Resource::Runs, None, "nightly.json")
                .is_err()
        );
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
        create_project(&repository, "checkout.json");
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
        let runs = directory.path().join("projects/checkout/test_runs");
        fs::create_dir_all(&runs).expect("runs dir");
        symlink(&outside, runs.join("evil.json")).expect("symlink");

        let error = repository
            .read_at(Resource::Runs, Some(&project("checkout.json")), "evil.json")
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

    #[test]
    fn a_fresh_root_is_ready_and_holds_no_lock() {
        let (_directory, repository) = repository();

        let probe = repository.probe_readiness();
        assert!(probe.exists);
        assert!(probe.writable);
        assert!(probe.lockable);
        assert!(!probe.lock_held);
        assert!(probe.last_write_unix.is_some());
        assert!(probe.ready());
    }

    #[test]
    fn a_root_that_is_gone_is_not_ready() {
        let (directory, repository) = repository();
        fs::remove_dir_all(directory.path()).expect("remove root");

        let probe = repository.probe_readiness();
        assert_eq!(probe, StorageProbe::UNREACHABLE);
        assert!(!probe.ready());
    }

    #[test]
    fn a_lock_another_process_holds_is_busy_rather_than_broken() {
        let (_directory, repository) = repository();
        let held = repository.acquire_lock().expect("lock");

        let probe = repository.probe_readiness();
        assert!(probe.lockable, "a lock a peer holds is not a fault");
        assert!(probe.lock_held);
        assert!(probe.ready());

        drop(held);
        assert!(!repository.probe_readiness().lock_held);
    }

    #[test]
    fn a_probe_removes_the_scratch_file_it_wrote() {
        let (directory, repository) = repository();
        repository.probe_readiness();

        let mut names = fs::read_dir(directory.path())
            .expect("root")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            vec![".tucano.lock".to_owned(), "projects".to_owned()],
            "the writability probe left something behind"
        );
    }

    #[test]
    fn a_probe_sees_writes_below_the_root_not_only_the_root() {
        let (directory, repository) = repository();
        let opened = mtime_seconds(directory.path()).expect("root mtime");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        create_project(&repository, "checkout.json");

        let seen = repository
            .probe_readiness()
            .last_write_unix
            .expect("a visible write");
        assert!(
            seen > opened,
            "the probe reported the root alone: {seen} is not after {opened}"
        );
    }

    #[test]
    fn polling_a_probe_never_counts_as_a_write_itself() {
        let (_directory, repository) = repository();
        let first = repository.probe_readiness().last_write_unix;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = repository.probe_readiness().last_write_unix;

        assert_eq!(
            first, second,
            "the probe's own scratch file advanced the write clock"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_root_is_not_ready() {
        use std::os::unix::fs::PermissionsExt;

        let (directory, repository) = repository();
        let root = directory.path();
        fs::set_permissions(root, fs::Permissions::from_mode(0o555)).expect("make read-only");
        let restore = || fs::set_permissions(root, fs::Permissions::from_mode(0o755));

        // A privileged user writes through the mode bits, so there is nothing to
        // observe and nothing to assert.
        let privileged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(".tucano-privilege-check"))
            .is_ok();
        if privileged {
            let _ = fs::remove_file(root.join(".tucano-privilege-check"));
            restore().expect("restore permissions");
            return;
        }

        let probe = repository.probe_readiness();
        assert!(probe.exists);
        assert!(!probe.writable);
        assert!(!probe.ready());

        restore().expect("restore permissions");
    }
}
