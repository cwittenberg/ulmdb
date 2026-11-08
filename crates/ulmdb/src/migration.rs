//! Physical format upgrades preserve logical identities and held snapshots.
use crate::{Environment, Error, Result, environment::Snapshot};
use std::{
    fs::File,
    sync::{Arc, atomic::Ordering},
};

impl Environment {
    /// Adopt cohort publication without changing logical state or old readers.
    pub fn upgrade_publication(&self) -> Result<u128> {
        self.upgrade_format(3)
    }

    /// Adopt the incremental segment directory (candidate format 0.4).
    /// This initial rebuild is charged maintenance; later commits copy paths.
    pub fn upgrade_inventory(&self) -> Result<u128> {
        self.upgrade_format(4)
    }

    fn upgrade_format(&self, minor: u16) -> Result<u128> {
        self.require_file_backend()?;
        self.require_writable()?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        if base.manifest.format_minor >= minor {
            return Ok(base.manifest.generation);
        }
        let directory = self.directory.join("cohorts");
        match std::fs::symlink_metadata(&directory) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err(Error::Corrupt("cohort directory is not a directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&directory)?
            }
            Err(error) => return Err(error.into()),
        }
        // Persist the parent before CURRENT can select a new storage strategy.
        ulmdb_platform::sync_path(&directory)?;
        ulmdb_platform::sync_path(&self.directory)?;
        let mut manifest = base.manifest.clone();
        manifest.format_minor = minor;
        manifest.previous = manifest.generation;
        manifest.generation = ulmdb_storage::new_identity()?;
        if minor == 4 {
            manifest.prepare_indexed(None)?;
        }
        let generation = manifest.generation;
        let mut snapshot = Snapshot {
            range_backed: false,
            external: None,
            physical_pin: None,
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: None,
            owner: base.owner,
            manifest,
            segments: base.segments.clone(),
            catalog: base.catalog.clone(),
            admission: None,
        };
        self.admit_snapshot(&mut snapshot)?;
        let publication = self.prepare_file_snapshot(&mut snapshot, None)?;
        let snapshot = Arc::new(snapshot);
        if publication.publish().is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        if self.activate(snapshot).is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        Ok(generation)
    }
}
