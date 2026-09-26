//! Generic CRUD over documents, and the placement of one entity under another.

use super::*;

impl FileRepository {
    /// Every identifier the tree holds for `resource`, sorted, without repeats.
    ///
    /// A listing is a set of identifiers, not of placements, because an
    /// identifier is what a caller addresses a resource by. Two homes holding
    /// the same identifier — two projects owning a run of the same name, a case
    /// duplicated into a second suite — are named here once, and that single
    /// name is what resolution reads: it refuses an identifier two homes hold as
    /// ambiguous. Listing a parent (`list_children`) is the view that still
    /// distinguishes the placements.
    pub(super) fn list(&self, resource: Resource) -> io::Result<Vec<String>> {
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

    pub(super) fn locate(&self, resource: Resource, id: &str) -> io::Result<Vec<Parent>> {
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

    pub(super) fn list_children(
        &self,
        parent: &Parent,
        child: Resource,
    ) -> io::Result<Vec<String>> {
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

    pub(super) fn exists_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<bool> {
        Ok(self.document(resource, parent, id)?.is_file())
    }

    pub(super) fn read_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<Value> {
        self.read_json(&self.document(resource, parent, id)?)
    }

    pub(super) fn read_raw_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<Vec<u8>> {
        // Descriptor confinement, not path approval (#366).
        read_confined(&self.root, &self.document(resource, parent, id)?)
    }

    pub(super) fn transform_at<F>(
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
        let lock = self.acquire_lock()?;
        let result = (|| {
            let path = self.document(resource, parent, id)?;
            if !path.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Resource not found",
                ));
            }
            let raw = read_confined(&self.root, &path)?;

            if let Some(expected) = expected_etag {
                let current = crate::storage::compute_etag(&raw);
                if current != expected {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, current));
                }
            }

            let stored: Value = serde_json::from_slice(&raw)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let new_value = transform(stored)?;

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
            self.write_json(&path, &new_value)
        })();
        lock.unlock()?;
        result
    }

    pub(super) fn write_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = self.write_at_locked(resource, parent, id, value);
        lock.unlock()?;
        result
    }

    /// Persists a document only where nothing is stored yet.
    ///
    /// The existence check reads the same location `write_at_locked` writes, so
    /// both happen inside one lock acquisition: two creates of the same
    /// identifier are serialised and the second one finds the first one's
    /// document rather than a location it can take. The check is the folder's
    /// own marker file, the same thing [`Self::exists_at`] reports, so a create
    /// answers exactly what a preceding `exists_at` would have answered.
    pub(super) fn create_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = (|| {
            if self.document(resource, parent, id)?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Resource already exists",
                ));
            }
            self.write_at_locked(resource, parent, id, value)
        })();
        lock.unlock()?;
        result
    }

    /// Writes a document, applying the guards the location imposes. The caller
    /// holds the advisory lock.
    fn write_at_locked(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
        value: &Value,
    ) -> io::Result<()> {
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
    }

    pub(super) fn delete_at(
        &self,
        resource: Resource,
        parent: Option<&Parent>,
        id: &str,
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = if resource.is_hierarchical() {
            fs::remove_dir_all(self.folder(resource, parent, id)?)
        } else {
            fs::remove_file(self.document(resource, parent, id)?)
        };
        lock.unlock()?;
        result
    }

    pub(super) fn place(
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
}
