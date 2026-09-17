//! Readiness probe for the storage volume.

use super::*;

impl FileRepository {
    pub(super) fn probe_readiness(&self) -> StorageProbe {
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
