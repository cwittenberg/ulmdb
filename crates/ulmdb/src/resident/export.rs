//! Snapshot-owned checkpoint export. No mutable source WAL is copied. The
//! writer mutex covers capture only; all validation and output IO use the pin.
use super::{Environment, Options, ReadTxn, SharedSnapshot, SnapshotExportReport};
use crate::{Error, Result};
use std::path::Path;

/// A snapshot captured from a live Resident owner, independently retained after
/// that owner closes. This is not a cross-process opener of a mutable WAL.
pub struct CapturedSnapshot {
    image: SharedSnapshot,
    export: ulmdb_storage::resident::SnapshotExport,
    options: Options,
    creator: u32,
}
impl Environment {
    pub fn capture_snapshot(&self) -> Result<CapturedSnapshot> {
        self.check_process()?;
        let writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if writer.poisoned || writer.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        let image = self.current.load_full();
        let state = writer.journal.state();
        if state.selected() != image.receipt
            || state.commit() != image.commit
            || state.root() != image.root
            || state.next_database() != image.next_database
        {
            return Err(Error::Corrupt("resident export selected identity"));
        }
        let export = writer.journal.capture_snapshot(&image.image)?;
        let mut options = self.options;
        options.wal_capacity = writer.journal.capacity();
        drop(writer);
        super::audit::audit(
            &image.image,
            image.root,
            image.next_database,
            options.graph_read_budget,
        )?;
        Ok(CapturedSnapshot {
            image,
            export,
            options,
            creator: std::process::id(),
        })
    }
    /// Export the snapshot captured at entry while later source writes continue.
    pub fn export_snapshot(&self, target: impl AsRef<Path>) -> Result<SnapshotExportReport> {
        self.capture_snapshot()?.export_to(target)
    }
}
impl CapturedSnapshot {
    pub fn begin_read(&self) -> ReadTxn {
        ReadTxn {
            snapshot: self.image.clone(),
        }
    }
    pub fn options(&self) -> Options {
        self.options
    }
    /// Export a complete independently reopenable Resident environment, keeping
    /// UUID, business sequence, commit, database IDs and slot generation floors.
    /// Physical checkpoint LSN advances once; source WAL/history is unchanged.
    pub fn export_to(&self, target: impl AsRef<Path>) -> Result<SnapshotExportReport> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident snapshot export"));
        }
        let target = target.as_ref();
        let mut stage = ulmdb_platform::owned_export::OwnedExport::create(
            target,
            ulmdb_storage::new_identity()?,
        )?;
        let report = self.export.write_to_owned(&mut stage)?;
        stage.publish().map_err(|error| {
            if ulmdb_platform::rename_outcome_unknown(&error) {
                Error::OutcomeUnknown
            } else {
                Error::Io(error)
            }
        })?;
        Ok(report)
    }
}
