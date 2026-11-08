//! Explicit permission changes own all admission gates for one checked namespace.
use crate::{Environment, Error, Result};
use std::sync::atomic::Ordering;
pub use ulmdb_storage::access::{AccessMigrationReport, FileAccess, FileAccessContext};
impl Environment {
    /// Migrate only recognized storage/control files in one exclusive pass. All C
    /// environments must close; NOLOCK callers supply their own exclusion. Existing
    /// mapped reads keep their data. No business descriptor/sequence is rewritten.
    pub fn migrate_file_access(&self, policy: FileAccess) -> Result<AccessMigrationReport> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        self.current.load().check_physical_pin(&self.directory)?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let mut gates = Vec::new();
        for name in ["LMDB-ENV-GATE", "LMDB-ENV"] {
            let path = self.directory.join(name);
            let file = ulmdb_storage::access::options_for_entry(&path)?
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)?;
            ulmdb_storage::access::validate_entry(&path, &file)?;
            file.try_lock().map_err(|_| Error::Busy)?;
            gates.push(file);
        }
        let coordination = self
            .coordination
            .as_ref()
            .cloned()
            .ok_or(Error::InvalidInput(
                "permission migration needs coordinated file owner",
            ))?;
        let _maintenance = coordination.try_maintenance()?;
        self.current.load().check_physical_pin(&self.directory)?;
        ulmdb_storage::access::migrate(&self.directory, policy, 16384)
    }
}
