use super::SnapshotSelection;
use crate::hybrid::{
    provenance::{ChunkIdentity, SourceBounds},
    redo::{RootState, WalReceipt},
};
use crate::{Error, Reference, Result};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{fs::OpenOptions, io::Read, path::Path};
pub(super) const HEADER: usize = 256;
pub(super) const ENTRY: usize = 192;
pub(super) const DOMAIN: &[u8] = b"ULMDB/hybrid/checkpoint-artifact/v1\0";
pub(super) fn put64(b: &mut [u8], at: usize, n: u64) {
    b[at..at + 8].copy_from_slice(&n.to_le_bytes());
}
fn get64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkDescription {
    pub identity: ChunkIdentity,
    pub end: u64,
    pub image_digest: [u8; 32],
    pub provenance_digest: [u8; 32],
    pub provenance_bytes: u64,
    pub extent_count: u64,
    pub bounds: SourceBounds,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointDescriptor {
    pub generation: u128,
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub selected: SnapshotSelection,
    pub required_wal_floor: u64,
    pub chunks: Vec<ChunkDescription>,
    pub digest: [u8; 32],
}
pub(super) fn header(
    generation: u128,
    uuid: [u8; 16],
    epoch: u64,
    s: SnapshotSelection,
    count: u64,
) -> [u8; HEADER] {
    let mut b = [0; HEADER];
    b[..8].copy_from_slice(b"ULMDBHC1");
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    b[16..32].copy_from_slice(&uuid);
    put64(&mut b, 32, epoch);
    b[40..56].copy_from_slice(&generation.to_le_bytes());
    put64(&mut b, 56, s.receipt.lsn);
    put64(&mut b, 64, s.receipt.sequence);
    put64(&mut b, 72, s.receipt.offset);
    put64(&mut b, 80, s.receipt.length);
    b[88..120].copy_from_slice(&s.receipt.digest);
    b[120..136].copy_from_slice(&s.commit.to_le_bytes());
    b[136..152].copy_from_slice(&s.root.catalog.encode());
    put64(&mut b, 152, s.root.next_chunk);
    put64(&mut b, 160, s.root.next_database);
    put64(&mut b, 168, count);
    put64(&mut b, 176, u64::from(s.receipt.lsn != 0));
    b
}
pub(super) fn entry(c: &ChunkDescription) -> [u8; ENTRY] {
    let mut b = [0; ENTRY];
    put64(&mut b, 0, c.identity.chunk);
    b[8..24].copy_from_slice(&c.identity.logical_generation.to_le_bytes());
    put64(&mut b, 24, c.identity.capacity);
    put64(&mut b, 32, c.end);
    b[40..72].copy_from_slice(&c.image_digest);
    b[72..104].copy_from_slice(&c.provenance_digest);
    put64(&mut b, 104, c.provenance_bytes);
    put64(&mut b, 112, c.extent_count);
    for (n, v) in [
        c.bounds.first_creation_lsn,
        c.bounds.last_creation_lsn,
        c.bounds.first_creation_sequence,
        c.bounds.last_creation_sequence,
        c.bounds.last_authority_sequence,
        c.bounds.oldest_wal_lsn,
    ]
    .iter()
    .enumerate()
    {
        put64(&mut b, 120 + n * 8, *v);
    }
    b
}
/// Bounded descriptor decoder. Hashes/geometry/cut bounds are checked here;
/// use verify_artifact separately to authenticate its referenced image files.
pub fn read_descriptor(path: &Path, max_chunks: usize) -> Result<CheckpointDescriptor> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
        .open(path)?;
    let size = f.metadata()?.len();
    let maximum = HEADER as u64 + (max_chunks.min(1_048_575) as u64) * ENTRY as u64 + 32;
    if size < HEADER as u64 + 32 || size > maximum {
        return Err(Error::Corrupt("checkpoint descriptor size"));
    }
    let mut h = [0; HEADER];
    f.read_exact(&mut h)?;
    let count = get64(&h, 168);
    // Reject invalid persisted counts before host-width conversion. A malformed
    // count is corruption on i686 too, never a retryable allocation limit.
    let count = ulmdb_core::descriptor_count::descriptor_count(count, max_chunks)
        .ok_or(Error::Corrupt("checkpoint descriptor count"))?;
    if &h[..8] != b"ULMDBHC1"
        || u32::from_le_bytes(h[8..12].try_into().unwrap()) != 1
        || u32::from_le_bytes(h[12..16].try_into().unwrap()) != HEADER as u32
        || h[184..].iter().any(|b| *b != 0)
        || count > max_chunks
        || count > 1_048_575
        || size != HEADER as u64 + count as u64 * ENTRY as u64 + 32
    {
        return Err(Error::Corrupt("checkpoint descriptor header/geometry"));
    }
    let uuid = h[16..32].try_into().unwrap();
    let epoch = get64(&h, 32);
    let generation = u128::from_le_bytes(h[40..56].try_into().unwrap());
    let selected = SnapshotSelection {
        receipt: WalReceipt {
            lsn: get64(&h, 56),
            sequence: get64(&h, 64),
            offset: get64(&h, 72),
            length: get64(&h, 80),
            digest: h[88..120].try_into().unwrap(),
        },
        commit: u128::from_le_bytes(h[120..136].try_into().unwrap()),
        root: RootState {
            catalog: Reference::decode(&h[136..152])?,
            next_chunk: get64(&h, 152),
            next_database: get64(&h, 160),
        },
    };
    let floor = get64(&h, 176);
    if uuid == [0; 16]
        || epoch == 0
        || generation == 0
        || selected.commit == 0
        || selected.root.next_chunk == 0
        || selected.root.next_database == 0
        || floor != u64::from(selected.receipt.lsn != 0)
        || selected.receipt.lsn != selected.receipt.sequence
    {
        return Err(Error::Corrupt("checkpoint descriptor identity/floor"));
    }
    let wire = crate::hybrid::journal::wire::encode_witness(
        crate::hybrid::journal::Bootstrap {
            uuid,
            epoch,
            commit: selected.commit,
        },
        crate::hybrid::journal::wire::Witness {
            receipt: selected.receipt,
            commit: selected.commit,
        },
    );
    crate::hybrid::journal::wire::decode_witness(
        crate::hybrid::journal::Bootstrap {
            uuid,
            epoch,
            commit: selected.commit,
        },
        &wire,
    )?;
    if !selected.root.catalog.is_null()
        && (selected.root.catalog.segment == 0
            || selected.root.catalog.segment >= selected.root.next_chunk
            || selected.root.catalog.offset < 4096)
    {
        return Err(Error::Corrupt("checkpoint root bounds"));
    }
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(h);
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(count)
        .map_err(|_| Error::LimitExceeded)?;
    let mut previous = 0;
    for _ in 0..count {
        let mut b = [0; ENTRY];
        f.read_exact(&mut b)?;
        hash.update(b);
        let c = ChunkDescription {
            identity: ChunkIdentity {
                uuid,
                epoch,
                chunk: get64(&b, 0),
                logical_generation: u128::from_le_bytes(b[8..24].try_into().unwrap()),
                capacity: get64(&b, 24),
            },
            end: get64(&b, 32),
            image_digest: b[40..72].try_into().unwrap(),
            provenance_digest: b[72..104].try_into().unwrap(),
            provenance_bytes: get64(&b, 104),
            extent_count: get64(&b, 112),
            bounds: SourceBounds {
                first_creation_lsn: get64(&b, 120),
                last_creation_lsn: get64(&b, 128),
                first_creation_sequence: get64(&b, 136),
                last_creation_sequence: get64(&b, 144),
                last_authority_sequence: get64(&b, 152),
                oldest_wal_lsn: get64(&b, 160),
            },
        };
        c.identity.validate()?;
        let bounds = c.bounds;
        if bounds.first_creation_lsn > bounds.last_creation_lsn
            || bounds.first_creation_sequence > bounds.last_creation_sequence
            || bounds.last_authority_sequence < bounds.last_creation_sequence
            || bounds.oldest_wal_lsn > bounds.last_creation_lsn
            || (c.extent_count == 0 && bounds != SourceBounds::default())
        {
            return Err(Error::Corrupt("checkpoint descriptor source bounds"));
        }
        if c.identity.chunk <= previous
            || c.identity.chunk >= selected.root.next_chunk
            || c.end < 4096
            || c.end > c.identity.capacity
            || c.bounds.last_creation_lsn > selected.receipt.lsn
            || c.bounds.last_creation_sequence > selected.receipt.sequence
            || c.bounds.last_authority_sequence > selected.receipt.sequence
            || c.extent_count > super::super::provenance::MAX_EXTENTS as u64
            || b[168..].iter().any(|b| *b != 0)
        {
            return Err(Error::Corrupt("checkpoint descriptor chunk order/bounds"));
        }
        previous = c.identity.chunk;
        chunks.push(c);
    }
    if !selected.root.catalog.is_null()
        && !chunks.iter().any(|c| {
            c.identity.chunk == selected.root.catalog.segment
                && selected.root.catalog.offset < c.end
        })
    {
        return Err(Error::Corrupt("checkpoint root chunk/bound absent"));
    }
    let mut digest = [0; 32];
    f.read_exact(&mut digest)?;
    if hash.finalize().as_slice() != digest {
        return Err(Error::Corrupt("checkpoint descriptor hash"));
    }
    Ok(CheckpointDescriptor {
        generation,
        uuid,
        epoch,
        selected,
        required_wal_floor: floor,
        chunks,
        digest,
    })
}
