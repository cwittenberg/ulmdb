//! H5 checkpoint artifacts, NOT activation or independent recovery authority.
//! Source receipts are preserved verbatim. WAL remains authoritative/retained;
//! neither these files nor their dependency floors permit WAL truncation.
mod archive;
mod builder;
pub mod selection;
pub use archive::{ArchivedChunk, CheckpointArchive};
mod descriptor;
mod fault;
use super::{
    ImageTable,
    journal::Journal,
    provenance::ProvenanceDirectory,
    redo::{RootState, WalReceipt},
};
use crate::{Error, Result};
pub use builder::{CheckpointArtifact, CheckpointLimits, SpaceReport, build, verify_artifact};
pub use descriptor::{CheckpointDescriptor, ChunkDescription, read_descriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotSelection {
    pub receipt: WalReceipt,
    pub commit: u128,
    pub root: RootState,
}
/// Owning O(1) cut. The caller must be the H3 writer-fenced Snapshot adapter:
/// this low-level factory compares the exact Snapshot selection with the live
/// Journal, but cannot issue Environment provenance authority for arbitrary
/// caller-created ImageTables. Off-lock building validates all supplied bounds.
/// No writer lock, Journal/File handle or mutable image permission is retained.
#[derive(Clone)]
pub struct CheckpointCut {
    pub(super) uuid: [u8; 16],
    pub(super) epoch: u64,
    pub(super) selected: SnapshotSelection,
    pub(super) image: ImageTable,
    pub(super) directory: ProvenanceDirectory,
}
impl CheckpointCut {
    pub fn capture(
        journal: &Journal,
        snapshot: SnapshotSelection,
        image: &ImageTable,
        directory: &ProvenanceDirectory,
    ) -> Result<Self> {
        if journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        if snapshot.receipt != journal.selected()
            || snapshot.commit != journal.commit_id()
            || snapshot.root != journal.root()
        {
            return Err(Error::InvalidInput(
                "checkpoint cut is not selected Snapshot",
            ));
        }
        let bootstrap = journal.bootstrap();
        Ok(Self {
            uuid: bootstrap.uuid,
            epoch: bootstrap.epoch,
            selected: snapshot,
            image: image.clone(),
            directory: directory.clone(),
        })
    }
    pub fn selection(&self) -> SnapshotSelection {
        self.selected
    }
    pub fn chunk_count(&self) -> usize {
        self.directory.len()
    }
    /// Conservative full-history retention. Table minima are not enough to
    /// justify dropping dependencies of historical BaseCopy records/H0 state.
    pub fn required_wal_floor(&self) -> u64 {
        u64::from(self.selected.receipt.lsn != 0)
    }
}
