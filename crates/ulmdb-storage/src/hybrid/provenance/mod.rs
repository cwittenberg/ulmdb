//! Checkpoint-only immutable allocation metadata. This is not a source authority
//! issuer and does not replace H3's selected-source checks or its live ledger.
mod directory;
mod format;
use super::redo::{Authority, Extent, IMAGE_DATA_START, MAX_CHUNK_BYTES, SourceReceipt};
use crate::{Error, Reference, Result};
pub use directory::{ProvenanceDirectory, ProvenanceIter};
pub use format::{EncodedProvenance, decode, encode};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
fn owner_from(counter: &AtomicU64) -> Result<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| Error::LimitExceeded)
}
pub const MAX_EXTENTS: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkIdentity {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub chunk: u64,
    pub logical_generation: u128,
    pub capacity: u64,
}
impl ChunkIdentity {
    pub fn validate(self) -> Result<()> {
        if self.uuid == [0; 16]
            || self.epoch == 0
            || self.chunk == 0
            || self.logical_generation == 0
            || self.capacity <= IMAGE_DATA_START
            || self.capacity > MAX_CHUNK_BYTES
        {
            return Err(Error::InvalidInput("checkpoint chunk identity"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Origin {
    pub extent: Extent,
    pub receipt: SourceReceipt,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceBounds {
    pub first_creation_lsn: u64,
    pub last_creation_lsn: u64,
    pub first_creation_sequence: u64,
    pub last_creation_sequence: u64,
    pub last_authority_sequence: u64,
    /// Zero means no WAL-authorized origins in this table, NOT permission to trim.
    pub oldest_wal_lsn: u64,
}
/// Mutable only before sealing. Moving it into an Arc moves the BTreeMap root;
/// retaining a cut can never trigger an Arc::make_mut clone on the next append.
/// Continued writes must allocate a fresh logical chunk/ActiveProvenance.
pub struct ActiveProvenance {
    identity: ChunkIdentity,
    entries: BTreeMap<Reference, Origin>,
    bounds: SourceBounds,
    end: u64,
    owner: u64,
    rollback_epoch: u64,
}
pub struct AppendMark {
    owner: u64,
    rollback_epoch: u64,
    count: usize,
    bounds: SourceBounds,
    end: u64,
}
pub struct ChunkProvenance {
    identity: ChunkIdentity,
    entries: BTreeMap<Reference, Origin>,
    bounds: SourceBounds,
    end: u64,
}
impl ActiveProvenance {
    pub fn new(identity: ChunkIdentity) -> Result<Self> {
        identity.validate()?;
        Ok(Self {
            identity,
            entries: BTreeMap::new(),
            bounds: SourceBounds::default(),
            end: IMAGE_DATA_START,
            owner: owner_from(&NEXT_OWNER)?,
            rollback_epoch: 0,
        })
    }
    pub fn identity(&self) -> ChunkIdentity {
        self.identity
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn get(&self, at: Reference) -> Option<&Origin> {
        self.entries.get(&at)
    }
    pub fn floor(&self, at: Reference) -> Option<&Origin> {
        self.entries.range(..=at).next_back().map(|(_, o)| o)
    }
    /// Private append rollback token: no map scan/clone and no caller-constructible fields.
    pub fn append_mark(&self) -> AppendMark {
        AppendMark {
            owner: self.owner,
            rollback_epoch: self.rollback_epoch,
            count: self.entries.len(),
            bounds: self.bounds,
            end: self.end,
        }
    }
    pub fn rollback_append(&mut self, mark: AppendMark) -> Result<()> {
        if mark.owner != self.owner
            || mark.rollback_epoch != self.rollback_epoch
            || mark.count > self.entries.len()
        {
            return Err(Error::InvalidInput("foreign append rollback token"));
        }
        let epoch = self
            .rollback_epoch
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        while self.entries.len() > mark.count {
            self.entries.pop_last();
        }
        self.bounds = mark.bounds;
        self.end = mark.end;
        self.rollback_epoch = epoch;
        Ok(())
    }
    pub fn check_seal(
        &self,
        committed_end: u64,
        selected_lsn: u64,
        selected_sequence: u64,
    ) -> Result<()> {
        if committed_end < self.end
            || committed_end > self.identity.capacity
            || self.bounds.last_creation_lsn > selected_lsn
            || self.bounds.last_creation_sequence > selected_sequence
            || self.bounds.last_authority_sequence > selected_sequence
        {
            return Err(Error::Corrupt(
                "checkpoint contains future/unbounded provenance",
            ));
        }
        Ok(())
    }
    pub fn insert(&mut self, origin: Origin) -> Result<()> {
        validate_origin(self.identity, origin)?;
        if self.entries.len() >= MAX_EXTENTS {
            return Err(Error::LimitExceeded);
        }
        let r = origin.receipt;
        if origin.extent.at.offset < self.end {
            return Err(Error::Corrupt("checkpoint origin order/overlap"));
        }
        if let Some((_, prior)) = self.entries.last_key_value() {
            let p = prior.receipt;
            if r.creation_lsn < p.creation_lsn
                || r.creation_sequence < p.creation_sequence
                || (r.creation_lsn == p.creation_lsn && creation_key(r) != creation_key(p))
                || (r.creation_lsn > p.creation_lsn && r.creation_sequence <= p.creation_sequence)
            {
                return Err(Error::Corrupt("checkpoint creation order/identity"));
            }
        }
        let mut b = self.bounds;
        if self.entries.is_empty() {
            b.first_creation_lsn = r.creation_lsn;
            b.first_creation_sequence = r.creation_sequence;
        }
        b.last_creation_lsn = r.creation_lsn;
        b.last_creation_sequence = r.creation_sequence;
        let authority_sequence = match r.authority {
            Authority::Wal { lsn, sequence, .. } => {
                if b.oldest_wal_lsn == 0 {
                    b.oldest_wal_lsn = lsn;
                }
                b.oldest_wal_lsn = b.oldest_wal_lsn.min(lsn);
                sequence
            }
            Authority::Checkpoint { sequence, .. } => sequence,
        };
        b.last_authority_sequence = b.last_authority_sequence.max(authority_sequence);
        let end = origin.extent.end()?;
        self.entries.insert(origin.extent.at, origin);
        self.bounds = b;
        self.end = end;
        Ok(())
    }
    /// O(1): cached bounds are checked, then the complete map root is moved.
    /// This establishes representational cut bounds, not durable source authority.
    pub fn seal(
        self,
        committed_end: u64,
        selected_lsn: u64,
        selected_sequence: u64,
    ) -> Result<Arc<ChunkProvenance>> {
        self.check_seal(committed_end, selected_lsn, selected_sequence)?;
        Ok(Arc::new(ChunkProvenance {
            identity: self.identity,
            entries: self.entries,
            bounds: self.bounds,
            end: committed_end,
        }))
    }
}
impl ChunkProvenance {
    pub fn identity(&self) -> ChunkIdentity {
        self.identity
    }
    pub fn end(&self) -> u64 {
        self.end
    }
    pub fn bounds(&self) -> SourceBounds {
        self.bounds
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn get(&self, at: Reference) -> Option<&Origin> {
        self.entries.get(&at)
    }
    pub fn floor(&self, at: Reference) -> Option<&Origin> {
        self.entries.range(..=at).next_back().map(|(_, o)| o)
    }
    pub fn origins(&self) -> impl ExactSizeIterator<Item = &Origin> {
        self.entries.values()
    }
}
pub(super) type Creation = (u64, u64, u128, [u8; 32]);
pub(super) fn creation_key(r: SourceReceipt) -> Creation {
    (
        r.creation_lsn,
        r.creation_sequence,
        r.creation_commit,
        r.provenance_digest,
    )
}
pub(super) fn validate_origin(identity: ChunkIdentity, o: Origin) -> Result<()> {
    o.extent.validate()?;
    let r = o.receipt;
    if o.extent.at.segment != identity.chunk
        || o.extent.logical_generation != identity.logical_generation
        || o.extent.end()? > identity.capacity
        || r.uuid != identity.uuid
        || r.epoch != identity.epoch
        || r.creation_commit == 0
    {
        return Err(Error::Corrupt("checkpoint origin realm/extent/creation"));
    }
    match r.authority {
        Authority::Wal {
            lsn,
            sequence,
            digest,
        } if lsn > 0
            && sequence > 0
            && lsn == r.creation_lsn
            && sequence == r.creation_sequence
            && digest == r.provenance_digest =>
        {
            Ok(())
        }
        Authority::Checkpoint {
            generation,
            sequence,
            ..
        } if generation != 0 && sequence >= r.creation_sequence => Ok(()),
        _ => Err(Error::Corrupt("checkpoint source authority")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_owner_exhaustion_never_wraps_or_reissues_a_token() {
        let counter = AtomicU64::new(u64::MAX - 1);
        assert_eq!(owner_from(&counter).unwrap(), u64::MAX - 1);
        assert!(owner_from(&counter).is_err());
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    }
    #[test]
    fn sealing_moves_btree_nodes_without_cloning_entries() {
        let mut active = ActiveProvenance::new(ChunkIdentity {
            uuid: [1; 16],
            epoch: 1,
            chunk: 1,
            logical_generation: 1,
            capacity: 65536,
        })
        .unwrap();
        for n in 0..128 {
            let receipt = SourceReceipt {
                uuid: [1; 16],
                epoch: 1,
                creation_lsn: 1,
                creation_sequence: 1,
                creation_commit: 1,
                provenance_digest: [2; 32],
                authority: Authority::Wal {
                    lsn: 1,
                    sequence: 1,
                    digest: [2; 32],
                },
            };
            active
                .insert(Origin {
                    extent: Extent {
                        at: Reference {
                            segment: 1,
                            offset: 4096 + n * 96,
                        },
                        logical_generation: 1,
                        length: 96,
                    },
                    receipt,
                })
                .unwrap();
        }
        let pointers = active
            .entries
            .values()
            .map(std::ptr::from_ref)
            .collect::<Vec<_>>();
        let sealed = active.seal(4096 + 128 * 96, 1, 1).unwrap();
        assert_eq!(
            pointers,
            sealed.origins().map(std::ptr::from_ref).collect::<Vec<_>>()
        );
    }
}
