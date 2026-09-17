use fs2::FileExt;
use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use super::layout::{
    Parent, Placement, RESERVED_PROJECT_CHILDREN, attachment_path, case_dir, case_marker,
    folder_wire_id, node_folder, parent_dir, project_collection_dir, project_dir,
    project_document_path, project_marker, revision_dir, revision_marker, root_dir,
    set_private_permissions, step_attachment_path, suite_dir, suite_marker, unique_suffix,
    validate_document_id,
};
use super::{Repository, Resource, StorageProbe};

// The `Repository` implementation is grouped by concern; each sub-module holds
// one cohesive slice of it and nothing else.
mod attachments;
mod crud;
mod probe;
mod revisions;

/// Default lock-acquisition timeout: five seconds.
const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_millis(5000);

/// Poll interval between `try_lock_exclusive` attempts.
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Filesystem-backed [`Repository`]: a folder tree for projects, suites, and
/// cases, and one JSON document per run, milestone and configuration inside the
/// project folder that owns it.
#[derive(Clone)]
pub struct FileRepository {
    root: PathBuf,
    lock_timeout: Duration,
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
        Ok(Self {
            root,
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
        })
    }

    /// Override the advisory-lock acquisition timeout.
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// Take the single writer lock for the data directory, retrying with a
    /// deadline so a slow writer cannot starve all others indefinitely.
    ///
    /// Returns `WouldBlock` when the deadline expires, which the domain layer
    /// translates into a `LockTimeout` — a 503 with `Retry-After`.
    fn acquire_lock(&self) -> io::Result<File> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".tucano.lock"))?;
        let deadline = Instant::now() + self.lock_timeout;
        loop {
            match lock.try_lock_exclusive() {
                Ok(()) => return Ok(lock),
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(LOCK_POLL_INTERVAL);
                    continue;
                }
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "lock acquisition timed out",
                    ));
                }
            }
        }
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

// A trait implementation cannot be split across modules, so each `Repository`
// method lives as an inherent method on `FileRepository` beside its concern and
// is forwarded here one-to-one.
impl Repository for FileRepository {
    fn list(&self, resource: Resource) -> io::Result<Vec<String>> {
        FileRepository::list(self, resource)
    }
    fn locate(&self, resource: Resource, id: &str) -> io::Result<Vec<Parent>> {
        FileRepository::locate(self, resource, id)
    }
    fn list_children(&self, parent: &Parent, child: Resource) -> io::Result<Vec<String>> {
        FileRepository::list_children(self, parent, child)
    }
    fn exists_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<bool> {
        FileRepository::exists_at(self, resource, parent, id)
    }
    fn read_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<Value> {
        FileRepository::read_at(self, resource, parent, id)
    }
    fn read_raw_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<Vec<u8>> {
        FileRepository::read_raw_at(self, resource, parent, id)
    }
    fn transform_at<F>(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        expected_etag: Option<&str>,
        transform: F,
    ) -> io::Result<()>
    where
        F: FnOnce(Value) -> io::Result<Value>,
    {
        FileRepository::transform_at(self, resource, parent, id, expected_etag, transform)
    }
    fn write_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> io::Result<()> {
        FileRepository::write_at(self, resource, parent, id, value)
    }
    fn delete_at(&self, resource: Resource, parent: Option<&Parent>, id: &str) -> io::Result<()> {
        FileRepository::delete_at(self, resource, parent, id)
    }
    fn place(
        &self,
        resource: Resource,
        source: &Parent,
        id: &str,
        target: &Parent,
        mode: Placement,
    ) -> io::Result<()> {
        FileRepository::place(self, resource, source, id, target, mode)
    }
    fn save_attachment(
        &self,
        parent: &Parent,
        case: &str,
        filename: &str,
        entry: &Value,
        contents: &[u8],
    ) -> io::Result<()> {
        FileRepository::save_attachment(self, parent, case, filename, entry, contents)
    }
    fn save_revision(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
        value: &Value,
    ) -> io::Result<()> {
        FileRepository::save_revision(self, parent, case, version, value)
    }
    fn save_revision_locked(
        &self,
        parent: &Parent,
        case: &str,
        version: u64,
        value: &Value,
    ) -> io::Result<()> {
        FileRepository::save_revision_locked(self, parent, case, version, value)
    }
    fn list_revisions(&self, parent: &Parent, case: &str) -> io::Result<Vec<u64>> {
        FileRepository::list_revisions(self, parent, case)
    }
    fn read_revision(&self, parent: &Parent, case: &str, version: u64) -> io::Result<Value> {
        FileRepository::read_revision(self, parent, case, version)
    }
    fn read_attachment(&self, parent: &Parent, case: &str, filename: &str) -> io::Result<Vec<u8>> {
        FileRepository::read_attachment(self, parent, case, filename)
    }
    fn delete_attachment(&self, parent: &Parent, case: &str, filename: &str) -> io::Result<()> {
        FileRepository::delete_attachment(self, parent, case, filename)
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
        FileRepository::save_step_attachment(
            self, parent, case, step_index, filename, entry, contents,
        )
    }
    fn delete_step_attachment(
        &self,
        parent: &Parent,
        case: &str,
        step_index: usize,
        filename: &str,
    ) -> io::Result<()> {
        FileRepository::delete_step_attachment(self, parent, case, step_index, filename)
    }
    fn probe_readiness(&self) -> StorageProbe {
        FileRepository::probe_readiness(self)
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
mod tests;
