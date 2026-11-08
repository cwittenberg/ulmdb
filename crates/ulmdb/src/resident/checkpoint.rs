//! Authenticated checkpoint plus bounded-tail rotation. Snapshot pages remain
//! anonymous immutable owners; retirement removes history, never reader bytes.
use super::{Environment, Snapshot, audit, environment::Writer};
use crate::{Error, Result};
use std::sync::{Arc, atomic::Ordering};
use ulmdb_storage::resident::wal_codec::DATA_START;
#[derive(Clone, Copy, Debug)]
pub struct CheckpointReport {
    pub sequence: u64,
    pub physical_lsn: u64,
    pub wal_capacity: u64,
    pub tail_bytes_before: u64,
    pub tail_bytes_after: u64,
    pub retired_files: u64,
}
impl Environment {
    /// Persist the current immutable slot image and replace the full history by
    /// an empty bounded tail. Does not advance the business sequence or commit.
    /// A held writer returns Busy rather than recursively waiting on itself.
    pub fn checkpoint(&self) -> Result<CheckpointReport> {
        self.check_process()?;
        let mut writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        let capacity = writer.journal.capacity();
        self.checkpoint_locked(&mut writer, capacity)
    }
    /// Grow the real preallocated WAL with the same authenticated atomic
    /// rotation. Reopen with options().wal_capacity, not the previous geometry.
    pub fn grow_wal(&self, capacity: u64) -> Result<CheckpointReport> {
        self.check_process()?;
        let mut writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if capacity <= writer.journal.capacity()
            || capacity > 268_435_456
            || !capacity.is_multiple_of(4096)
        {
            return Err(Error::InvalidInput("resident WAL growth capacity"));
        }
        self.checkpoint_locked(&mut writer, capacity)
    }
    pub(super) fn rotate_if_needed(&self, writer: &mut Writer) -> Result<()> {
        let used = writer.journal.physical_bytes()?.saturating_sub(DATA_START);
        let tail = writer.journal.capacity().saturating_sub(DATA_START);
        if used > 0 && used >= tail / 2 {
            self.checkpoint_locked(writer, writer.journal.capacity())?;
        }
        Ok(())
    }
    /// Negotiate coherent independent read capture. A checkpoint binds the
    /// stable lease inode into the authenticated WAL header. Older binaries
    /// reject that header; default environments retain their original bytes.
    /// Existing business sequence/commit and borrowed readers stay unchanged.
    pub fn enable_readonly_capture(&self) -> Result<()> {
        self.check_process()?;
        let mut writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if writer.poisoned || writer.journal.is_poisoned() || self.pool.stats()?.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if writer.journal.capture_enabled() {
            return Ok(());
        }
        let capacity = writer.journal.capacity();
        self.checkpoint_locked_inner(&mut writer, capacity, true)?;
        Ok(())
    }
    pub(super) fn checkpoint_locked(
        &self,
        writer: &mut Writer,
        capacity: u64,
    ) -> Result<CheckpointReport> {
        self.checkpoint_locked_inner(writer, capacity, false)
    }
    fn checkpoint_locked_inner(
        &self,
        writer: &mut Writer,
        capacity: u64,
        enable: bool,
    ) -> Result<CheckpointReport> {
        if writer.poisoned || writer.journal.is_poisoned() || self.pool.stats()?.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        let state = writer.journal.state();
        if state.selected() != base.receipt
            || state.commit() != base.commit
            || state.root() != base.root
            || state.next_database() != base.next_database
            || writer
                .incremental
                .as_ref()
                .is_some_and(|c| !c.matches(&base))
        {
            return Err(Error::Corrupt("resident checkpoint selected snapshot"));
        }
        // A complete typed audit precedes every checkpoint decision. Recovery
        // independently audits the seeded graph before heal/tail clearing.
        audit::audit(
            &base.image,
            base.root,
            base.next_database,
            self.options.graph_read_budget,
        )?;
        let before = base.receipt.end()?;
        let path = self.directory().join("RESIDENT.wal");
        let (journal, retired_files) =
            match writer
                .journal
                .checkpoint_and_prune(&path, &base.image, capacity, enable)
            {
                Ok(j) => j,
                Err(e) => {
                    if writer.journal.is_poisoned() {
                        writer.poisoned = true;
                        self.pool.quarantine();
                    }
                    return Err(e);
                }
            };
        let receipt = journal.state().selected();
        if self.pool.enable_checkpoint_tiny_reuse().is_err() {
            writer.poisoned = true;
            self.pool.quarantine();
            return Err(Error::OutcomeUnknown);
        }

        let next = Arc::new(Snapshot {
            image: base.image.clone(),
            catalog: base.catalog.clone(),
            root: base.root,
            next_database: base.next_database,
            receipt,
            commit: base.commit,
        });
        if let Some(c) = writer.incremental.as_mut() {
            c.checkpoint_rebind(receipt);
        }
        #[cfg(feature = "resident-prefix-audit-cache")]
        {
            writer.prefix_audit_cache = None;
        }
        writer.journal = journal;
        self.wal_capacity.store(capacity, Ordering::Release);
        self.current.store(next);
        // Selection and obsolete-file pruning share one exclusive capture
        // lease; an independent reader cannot enter between those operations.
        Ok(CheckpointReport {
            sequence: receipt.sequence,
            physical_lsn: receipt.lsn,
            wal_capacity: capacity,
            tail_bytes_before: before,
            tail_bytes_after: receipt.end()?,
            retired_files,
        })
    }
}
