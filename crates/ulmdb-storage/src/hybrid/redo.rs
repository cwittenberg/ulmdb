//! Canonical logical redo identities. Physical mmap incarnations are separate.
use crate::{Error, Reference, Result};
pub const MAX_CHUNK_BYTES: u64 = 268_435_456;
/// A legal 64MiB value plus its 16-byte stored payload header.
pub const MAX_OBJECT_BYTES: u32 = 67_108_880;
/// Bound expansion of compact records before cold replay allocation.
pub const MAX_MATERIALIZED_BYTES: u64 = 268_435_456;
pub const MAX_OBJECTS: usize = 65_536;
pub const MAX_PATCHES: usize = 65_536;
pub const MAX_CHUNKS: usize = 4096;
pub const IMAGE_DATA_START: u64 = 4096;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Extent {
    pub at: Reference,
    pub logical_generation: u128,
    pub length: u32,
}
impl Extent {
    pub fn end(self) -> Result<u64> {
        self.at
            .offset
            .checked_add(u64::from(self.length))
            .ok_or(Error::LimitExceeded)
    }
    pub fn validate(self) -> Result<()> {
        if self.at.segment == 0
            || self.logical_generation == 0
            || self.length == 0
            || self.length > MAX_OBJECT_BYTES
            || self.at.offset < IMAGE_DATA_START
            || self.end()? > MAX_CHUNK_BYTES
        {
            return Err(Error::Corrupt("hybrid extent bounds/generation"));
        }
        Ok(())
    }
    pub fn overlaps(self, other: Self) -> Result<bool> {
        Ok(self.at.segment == other.at.segment
            && self.at.offset < other.end()?
            && other.at.offset < self.end()?)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authority {
    Checkpoint {
        generation: u128,
        sequence: u64,
        digest: [u8; 32],
    },
    Wal {
        lsn: u64,
        sequence: u64,
        digest: [u8; 32],
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceReceipt {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub creation_lsn: u64,
    pub creation_sequence: u64,
    pub creation_commit: u128,
    pub provenance_digest: [u8; 32],
    pub authority: Authority,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalReceipt {
    pub lsn: u64,
    pub sequence: u64,
    pub offset: u64,
    pub length: u64,
    pub digest: [u8; 32],
}
impl WalReceipt {
    pub fn end(self) -> Result<u64> {
        self.offset
            .checked_add(self.length)
            .ok_or(Error::LimitExceeded)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkDelta {
    pub chunk: u64,
    pub logical_generation: u128,
    pub before: u64,
    pub after: u64,
    pub capacity: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootState {
    pub catalog: Reference,
    pub next_chunk: u64,
    pub next_database: u64,
}
impl RootState {
    pub fn empty() -> Self {
        Self {
            catalog: Reference::NULL,
            next_chunk: 1,
            next_database: 1,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Patch {
    pub offset: u32,
    pub bytes: Vec<u8>,
}
// Keep recipes contiguous: boxing would add an allocation/indirection for each
// copied page. The bounded Vec is intentionally sized for the larger recipe.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectRedo {
    Literal {
        dest: Extent,
        bytes: Vec<u8>,
    },
    BaseCopy {
        dest: Extent,
        source: Extent,
        receipt: SourceReceipt,
        patches: Vec<Patch>,
    },
}
impl ObjectRedo {
    pub fn dest(&self) -> Extent {
        match self {
            Self::Literal { dest, .. } | Self::BaseCopy { dest, .. } => *dest,
        }
    }
    /// Control builder using an explicit O(object size) diff. The live H1 recorder
    /// may supply tracked final patches instead. Tiny/full-changed objects retain
    /// full images whenever their literal wire representation is smaller.
    pub fn from_committed_bytes(
        dest: Extent,
        source: Extent,
        receipt: SourceReceipt,
        before: &[u8],
        after: &[u8],
    ) -> Result<Self> {
        if before.len() != source.length as usize
            || after.len() != dest.length as usize
            || source.length != dest.length
        {
            return Err(Error::InvalidInput("hybrid committed copy widths"));
        }
        let patches = final_patches(before, after)?;
        let cost = patches
            .iter()
            .try_fold(208usize, |n, p| {
                n.checked_add(8)?.checked_add(p.bytes.len())
            })
            .ok_or(Error::LimitExceeded)?;
        if cost >= after.len() {
            Ok(Self::Literal {
                dest,
                bytes: copy_bytes(after)?,
            })
        } else {
            Ok(Self::BaseCopy {
                dest,
                source,
                receipt,
                patches,
            })
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub lsn: u64,
    pub sequence: u64,
    pub commit: u128,
    pub parent_commit: u128,
    pub parent: WalReceipt,
    pub root: RootState,
    pub chunks: Vec<ChunkDelta>,
    pub objects: Vec<ObjectRedo>,
}
/// Trusted H3 boundary, NOT an authority issuer. Implementations must verify
/// exact logical extent/content membership in a CURRENT retained authority and
/// bind it to immutable bytes in the selected physical image incarnation.
/// Sequence comparison alone is insufficient. External callers of this low-level
/// prototype cannot turn arbitrary bytes into an Environment source certificate.
pub trait SourceResolver {
    fn resolve_committed<'a>(
        &'a self,
        source: &Extent,
        receipt: &SourceReceipt,
    ) -> Result<&'a [u8]>;
}
/// A prepared destination view; implementations expose only frozen private data.
pub trait DestinationResolver {
    fn resolve_prepared(&self, dest: &Extent) -> Result<&[u8]>;
}
impl Record {
    pub fn validate(&self) -> Result<()> {
        super::view::validate(self)
    }
    pub fn validate_sources(&self, resolver: &impl SourceResolver) -> Result<()> {
        self.validate()?;
        for object in &self.objects {
            if let ObjectRedo::BaseCopy {
                source, receipt, ..
            } = object
                && resolver.resolve_committed(source, receipt)?.len() != source.length as usize
            {
                return Err(Error::Corrupt("hybrid source resolution width"));
            }
        }
        Ok(())
    }
}
pub(crate) fn validate_source(record: &Record, s: SourceReceipt) -> Result<()> {
    if s.uuid != record.uuid
        || s.epoch != record.epoch
        || s.creation_lsn > record.parent.lsn
        || s.creation_sequence > record.parent.sequence
        || s.creation_commit == 0
    {
        return Err(Error::Corrupt("hybrid source identity/future epoch"));
    }
    match s.authority {
        Authority::Wal {
            lsn,
            sequence,
            digest,
        } if lsn > 0
            && sequence > 0
            && lsn == s.creation_lsn
            && sequence == s.creation_sequence
            && digest == s.provenance_digest => {}
        Authority::Checkpoint {
            generation,
            sequence,
            digest: _,
        } if generation != 0
            && sequence >= s.creation_sequence
            && sequence <= record.parent.sequence => {}
        _ => return Err(Error::Corrupt("hybrid source covering authority")),
    }
    Ok(())
}
pub struct MaterializedObject {
    pub extent: Extent,
    pub bytes: Vec<u8>,
}
/// Cold replay is staged: no sink or published state is mutated on any error.
pub fn replay(record: &Record, resolver: &impl SourceResolver) -> Result<Vec<MaterializedObject>> {
    record.validate_sources(resolver)?;
    let mut out = Vec::new();
    out.try_reserve_exact(record.objects.len())
        .map_err(|_| Error::LimitExceeded)?;
    for object in &record.objects {
        let (d, bytes) = match object {
            ObjectRedo::Literal { dest, bytes } => (*dest, copy_bytes(bytes)?),
            ObjectRedo::BaseCopy {
                dest,
                source,
                receipt,
                patches,
            } => {
                let mut b = copy_bytes(resolver.resolve_committed(source, receipt)?)?;
                for p in patches {
                    let at = p.offset as usize;
                    b[at..at + p.bytes.len()].copy_from_slice(&p.bytes);
                }
                (*dest, b)
            }
        };
        out.push(MaterializedObject { extent: d, bytes });
    }
    Ok(out)
}
/// Live-path correspondence check compares bytes; it never hashes full afterimages.
pub fn verify_materialized(
    record: &Record,
    sources: &impl SourceResolver,
    destinations: &impl DestinationResolver,
) -> Result<()> {
    record.validate_sources(sources)?;
    for object in &record.objects {
        let dest = object.dest();
        let after = destinations.resolve_prepared(&dest)?;
        if after.len() != dest.length as usize {
            return Err(Error::Corrupt("hybrid materialized width"));
        }
        match object {
            ObjectRedo::Literal { bytes, .. } => {
                if after != bytes {
                    return Err(Error::Corrupt("hybrid literal materialization mismatch"));
                }
            }
            ObjectRedo::BaseCopy {
                source,
                receipt,
                patches,
                ..
            } => {
                let before = sources.resolve_committed(source, receipt)?;
                let mut end = 0;
                for p in patches {
                    let at = p.offset as usize;
                    if after[end..at] != before[end..at] || after[at..at + p.bytes.len()] != p.bytes
                    {
                        return Err(Error::Corrupt("hybrid patch materialization mismatch"));
                    }
                    end = at + p.bytes.len();
                }
                if after[end..] != before[end..] {
                    return Err(Error::Corrupt("hybrid unpatched materialization mismatch"));
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn copy_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| Error::LimitExceeded)?;
    out.extend_from_slice(bytes);
    Ok(out)
}
/// Correctness/control builder. H1's recorder will supply tracked final ranges;
/// this explicit diff helper is O(object bytes), not a performance certificate.
pub fn final_patches(before: &[u8], after: &[u8]) -> Result<Vec<Patch>> {
    if before.len() != after.len() || after.len() > MAX_OBJECT_BYTES as usize {
        return Err(Error::InvalidInput("hybrid diff widths"));
    }
    let mut out = Vec::new();
    let mut i = 0;
    // A fixed 32-byte comparison stays inline on the supported portable x86
    // targets; the compiler may choose wider instructions for native builds.
    const BLOCK: usize = 32;
    while i < after.len() {
        // Fixed-width equality lets the compiler use portable vector/word loads
        // for unchanged regions. Scan a failed block once, without rechecking a
        // fresh overlapping block at every equal byte near its first mismatch.
        let end = i + (after.len() - i).min(BLOCK);
        if end - i == BLOCK && before[i..end] == after[i..end] {
            i = end;
            continue;
        }
        while i < end && before[i] == after[i] {
            i += 1;
        }
        if i == end {
            continue;
        }
        let start = i;
        i += 1;
        while i < after.len() && before[i] != after[i] {
            i += 1;
        }
        if out.len() == MAX_PATCHES {
            return Err(Error::LimitExceeded);
        }
        out.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        out.push(Patch {
            offset: start as u32,
            bytes: copy_bytes(&after[start..i])?,
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "tests/block_diff.rs"]
mod block_diff_tests;
