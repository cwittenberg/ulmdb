//! Private selected-Snapshot issuance and off-lock artifact staging. This is not
//! checkpoint activation/recovery and never changes WAL retention/authority.
mod activate;
pub(super) mod audit;
pub(super) mod base;
pub(in crate::hybrid) mod history;
use super::Environment;
use crate::{Error, Result};
pub use activate::{CheckpointInstallReport, PreparedCheckpoint};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
pub(super) mod selector;
use ulmdb_storage::hybrid::checkpoint::{
    self, CheckpointArtifact, CheckpointCut, CheckpointLimits, SnapshotSelection,
};
#[derive(Clone, Copy, Debug)]
pub struct CaptureReport {
    pub rotated: bool,
    /// Full new image capacity admitted, not just bytes allocated by a record.
    pub added_image_capacity: u64,
    /// Selected bytes beyond the frozen prefix cannot be reused after sealing.
    pub sealed_slack_bytes: u64,
    /// Explicit cold rotation/file/mmap/preallocation cost, under writer fence.
    pub rotation_time: Duration,
    /// Fixed WAL-anchor and root capture work; directory work is in rotation_time.
    pub metadata_capture_time: Duration,
}
/// Owns immutable image/metadata roots and no Environment or writer fence.
#[derive(Clone)]
pub struct CheckpointCapture {
    cut: CheckpointCut,
    report: CaptureReport,
    issuer: Arc<()>,
    anchor: Arc<history::Anchor>,
}
impl CheckpointCapture {
    pub fn selection(&self) -> SnapshotSelection {
        self.cut.selection()
    }
    pub fn chunk_count(&self) -> usize {
        self.cut.chunk_count()
    }
    pub fn report(&self) -> CaptureReport {
        self.report
    }
    pub fn stage(
        &self,
        target: impl AsRef<Path>,
        generation: u128,
        limits: CheckpointLimits,
    ) -> Result<CheckpointArtifact> {
        checkpoint::build(&self.cut, target.as_ref(), generation, limits)
    }
}
impl Environment {
    /// Capture the exact activated Snapshot. The cold rotation seals the old map
    /// by moving its nodes, inserts at most sixteen directory path nodes, and
    /// admits a fresh unpublished chunk. No full map scan/clone occurs under the
    /// writer lock. Empty/unused pending chunks are reused on repeated captures.
    pub fn capture_checkpoint(&self) -> Result<CheckpointCapture> {
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        writer.ledger.healthy()?;
        if writer.checkpoint_poisoned || writer.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        let selected = self.current.load_full();
        let selection = SnapshotSelection {
            receipt: selected.receipt,
            commit: selected.commit,
            root: selected.root,
        };
        if selection.receipt != writer.journal.selected()
            || selection.root != writer.journal.root()
            || selection.commit != writer.journal.commit_id()
        {
            return Err(Error::Corrupt("hybrid selected Snapshot/Journal mismatch"));
        }
        let anchor_start = Instant::now();
        let seed = writer.seed.clone();
        let path = self.directory.join("HYBRID.wal");
        let anchor = Arc::new(if let Some(seed) = seed {
            history::Anchor::Seeded(writer.journal.capture_seeded(
                &path,
                seed.compound,
                Arc::clone(&seed.aliases),
            )?)
        } else {
            history::Anchor::Full(writer.journal.capture_committed(&path)?)
        });
        let anchor_time = anchor_start.elapsed();
        let old_capacity = writer.image_bytes;
        let old_span = selected.image.chunk(writer.chunk.chunk_id());
        let slack = old_span.map_or(0, |s| (s.capacity() - s.end()) as u64);
        let start = Instant::now();
        let rotated = self.rotate_locked(&mut writer)?;
        let rotation_time = start.elapsed();
        let start = Instant::now();
        let cut = CheckpointCut::capture(
            &writer.journal,
            selection,
            &selected.image,
            writer.ledger.directory(),
        )?;
        let report = CaptureReport {
            rotated,
            added_image_capacity: writer.image_bytes - old_capacity,
            sealed_slack_bytes: if rotated { slack } else { 0 },
            rotation_time,
            metadata_capture_time: anchor_time.saturating_add(start.elapsed()),
        };
        Ok(CheckpointCapture {
            cut,
            report,
            issuer: Arc::clone(&self.owner),
            anchor,
        })
    }
}

fn verified_prefix(env: &Environment, capture: &CheckpointCapture) -> Result<history::History> {
    match capture.anchor.verify() {
        Ok(prefix) => Ok(prefix),
        Err(error) => {
            // This is a detected authoritative WAL failure, not an unselected
            // artifact validation error. Readers stay pinned; new writes fence.
            if let Ok(mut writer) = env.writer.lock() {
                writer.checkpoint_poisoned = true;
            }
            Err(error)
        }
    }
}
