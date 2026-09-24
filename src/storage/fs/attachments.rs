//! Attachment storage, for a case and for its structured steps.

use super::*;

impl FileRepository {
    pub(super) fn save_attachment(
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

    pub(super) fn read_attachment(
        &self,
        parent: &Parent,
        case: &str,
        filename: &str,
    ) -> io::Result<Vec<u8>> {
        fs::read(attachment_path(&self.root, parent, case, filename)?)
    }

    pub(super) fn delete_attachment(
        &self,
        parent: &Parent,
        case: &str,
        filename: &str,
    ) -> io::Result<()> {
        let lock = self.acquire_lock()?;
        let result = (|| {
            fs::remove_file(attachment_path(&self.root, parent, case, filename)?)?;
            self.forget_attachment(&case_marker(&self.root, parent, case)?, filename)
        })();
        lock.unlock()?;
        result
    }

    pub(super) fn save_step_attachment(
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

    pub(super) fn read_step_attachment(
        &self,
        parent: &Parent,
        case: &str,
        step_index: usize,
        filename: &str,
    ) -> io::Result<Vec<u8>> {
        fs::read(step_attachment_path(
            &self.root, parent, case, step_index, filename,
        )?)
    }

    pub(super) fn delete_step_attachment(
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
}
