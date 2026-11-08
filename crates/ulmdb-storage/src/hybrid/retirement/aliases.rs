use crate::hybrid::{
    checkpoint::{CheckpointArchive, SnapshotSelection},
    provenance::{Origin, ProvenanceDirectory},
    redo::{Authority, Extent, SourceReceipt},
};
use crate::{Error, Reference, Result};
use std::{borrow::Borrow, sync::Arc};
/// Exact read-only membership, not an authority issuer. The facade must bind
/// this archive to its privately selected descriptor before trusting aliases.
#[derive(Clone)]
pub struct CheckpointAliases {
    pub(crate) cut: SnapshotSelection,
    pub(crate) uuid: [u8; 16],
    pub(crate) epoch: u64,
    pub(crate) digest: [u8; 32],
    directory: ProvenanceDirectory,
}
impl CheckpointAliases {
    pub fn from_archive(archive: &CheckpointArchive) -> Result<Self> {
        let d = archive.descriptor();
        if d.chunks.len() as u64
            != d.selected
                .root
                .next_chunk
                .checked_sub(1)
                .ok_or(Error::Corrupt("alias chunk counter"))?
        {
            return Err(Error::Corrupt("alias incomplete chunk inventory"));
        }
        let mut directory = ProvenanceDirectory::default();
        for (&id, c) in archive.chunks() {
            if id >= d.selected.root.next_chunk {
                return Err(Error::Corrupt("alias chunk outside checkpoint"));
            }
            directory = directory.insert(Arc::clone(c.table()))?;
        }
        Ok(Self {
            cut: d.selected,
            uuid: d.uuid,
            epoch: d.epoch,
            digest: d.digest,
            directory,
        })
    }
    pub fn selection(&self) -> SnapshotSelection {
        self.cut
    }
    pub fn descriptor_digest(&self) -> [u8; 32] {
        self.digest
    }
    pub fn chunk(&self, id: u64) -> Option<&Arc<crate::hybrid::provenance::ChunkProvenance>> {
        self.directory.get(id)
    }
    pub fn chunks(&self) -> crate::hybrid::provenance::ProvenanceIter<'_> {
        self.directory.iter()
    }
    pub fn origin(&self, at: Reference) -> Option<Origin> {
        self.directory
            .get(at.segment)?
            .get(at)
            .map(|o| *Borrow::<Origin>::borrow(&o))
    }
    pub fn authorizes(&self, source: &Extent, receipt: &SourceReceipt) -> bool {
        if receipt.uuid != self.uuid
            || receipt.epoch != self.epoch
            || receipt.creation_lsn > self.cut.receipt.lsn
            || receipt.creation_sequence > self.cut.receipt.sequence
        {
            return false;
        }
        let Authority::Wal {
            lsn,
            sequence,
            digest,
        } = receipt.authority
        else {
            return false;
        };
        if lsn != receipt.creation_lsn
            || sequence != receipt.creation_sequence
            || digest != receipt.provenance_digest
        {
            return false;
        }
        self.origin(source.at)
            .is_some_and(|o| o.extent == *source && o.receipt == *receipt)
    }
}
