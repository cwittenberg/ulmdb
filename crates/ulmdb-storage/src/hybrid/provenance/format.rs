//! v1 compact origins: common chunk identity, 64-byte creation dictionary,
//! 64-byte checkpoint-authority dictionary, 16-byte extent rows. WAL authority
//! is reconstructed only when exactly equal to the creation identity.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};
const HEADER: usize = 256;
const DOMAIN: &[u8] = b"ULMDB/hybrid/chunk-provenance/v1\0";
const MAX_BYTES: usize = HEADER + MAX_EXTENTS * 144 + 32;
#[derive(Clone, Copy, Debug)]
pub struct EncodedProvenance {
    pub length: u64,
    pub digest: [u8; 32],
    pub origins: u32,
    pub checkpoint_authorities: u32,
}
type Checkpoint = (u128, u64, [u8; 32]);
fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
fn get64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn get32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn get128(b: &[u8], at: usize) -> u128 {
    u128::from_le_bytes(b[at..at + 16].try_into().unwrap())
}
fn bounds_words(b: SourceBounds) -> [u64; 6] {
    [
        b.first_creation_lsn,
        b.last_creation_lsn,
        b.first_creation_sequence,
        b.last_creation_sequence,
        b.last_authority_sequence,
        b.oldest_wal_lsn,
    ]
}
/// Streaming output: dictionaries are O(distinct creation/authority count),
/// image/object bytes and the extent table are never cloned into a Vec.
pub fn encode(chunk: &ChunkProvenance, mut out: impl Write) -> Result<EncodedProvenance> {
    let mut creations = BTreeMap::<Creation, u32>::new();
    let mut authorities = BTreeMap::<Checkpoint, u32>::new();
    for o in chunk.origins() {
        creations.entry(creation_key(o.receipt)).or_insert(0);
        if let Authority::Checkpoint {
            generation,
            sequence,
            digest,
        } = o.receipt.authority
        {
            authorities
                .entry((generation, sequence, digest))
                .or_insert(0);
        }
    }
    for (n, v) in creations.values_mut().enumerate() {
        *v = n as u32;
    }
    for (n, v) in authorities.values_mut().enumerate() {
        *v = n as u32 + 1;
    }
    let mut header = [0u8; HEADER];
    header[..8].copy_from_slice(b"ULMDBHO1");
    header[8..12].copy_from_slice(&1u32.to_le_bytes());
    header[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    let id = chunk.identity();
    header[16..32].copy_from_slice(&id.uuid);
    put64(&mut header, 32, id.epoch);
    put64(&mut header, 40, id.chunk);
    header[48..64].copy_from_slice(&id.logical_generation.to_le_bytes());
    put64(&mut header, 64, id.capacity);
    put64(&mut header, 72, chunk.end());
    header[80..84].copy_from_slice(&(chunk.len() as u32).to_le_bytes());
    header[84..88].copy_from_slice(&(creations.len() as u32).to_le_bytes());
    header[88..92].copy_from_slice(&(authorities.len() as u32).to_le_bytes());
    for (n, v) in bounds_words(chunk.bounds()).iter().enumerate() {
        put64(&mut header, 96 + n * 8, *v);
    }
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    let mut length = 0u64;
    let mut emit = |bytes: &[u8]| -> Result<()> {
        out.write_all(bytes)?;
        hash.update(bytes);
        length += bytes.len() as u64;
        Ok(())
    };
    emit(&header)?;
    for &(lsn, sequence, commit, digest) in creations.keys() {
        let mut row = [0; 64];
        put64(&mut row, 0, lsn);
        put64(&mut row, 8, sequence);
        row[16..32].copy_from_slice(&commit.to_le_bytes());
        row[32..].copy_from_slice(&digest);
        emit(&row)?;
    }
    for &(generation, sequence, digest) in authorities.keys() {
        let mut row = [0; 64];
        row[..16].copy_from_slice(&generation.to_le_bytes());
        put64(&mut row, 16, sequence);
        row[24..56].copy_from_slice(&digest);
        emit(&row)?;
    }
    for o in chunk.origins() {
        let mut row = [0; 16];
        row[..4].copy_from_slice(&(o.extent.at.offset as u32).to_le_bytes());
        row[4..8].copy_from_slice(&o.extent.length.to_le_bytes());
        row[8..12].copy_from_slice(&creations[&creation_key(o.receipt)].to_le_bytes());
        let a = match o.receipt.authority {
            Authority::Wal { .. } => 0,
            Authority::Checkpoint {
                generation,
                sequence,
                digest,
            } => authorities[&(generation, sequence, digest)],
        };
        row[12..16].copy_from_slice(&a.to_le_bytes());
        emit(&row)?;
    }
    let digest: [u8; 32] = hash.finalize().into();
    out.write_all(&digest)?;
    Ok(EncodedProvenance {
        length: length + 32,
        digest,
        origins: creations.len() as u32,
        checkpoint_authorities: authorities.len() as u32,
    })
}
/// Fully checked cold decoder. It reconstructs exact SourceReceipt values;
/// decoding a table does not grant those receipts authority to resolve bytes.
pub fn decode(bytes: &[u8]) -> Result<Arc<ChunkProvenance>> {
    if bytes.len() < HEADER + 32 || bytes.len() > MAX_BYTES {
        return Err(Error::Corrupt("checkpoint provenance size"));
    }
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(&bytes[..bytes.len() - 32]);
    if hash.finalize().as_slice() != &bytes[bytes.len() - 32..] {
        return Err(Error::Corrupt("checkpoint provenance hash"));
    }
    if &bytes[..8] != b"ULMDBHO1"
        || get32(bytes, 8) != 1
        || get32(bytes, 12) != HEADER as u32
        || bytes[92..96] != [0; 4]
        || bytes[144..HEADER].iter().any(|b| *b != 0)
    {
        return Err(Error::Corrupt(
            "checkpoint provenance header/version/reserved",
        ));
    }
    let count = get32(bytes, 80) as usize;
    let nc = get32(bytes, 84) as usize;
    let na = get32(bytes, 88) as usize;
    if count > MAX_EXTENTS || nc > count || na > count || (count > 0 && nc == 0) {
        return Err(Error::Corrupt("checkpoint provenance counts"));
    }
    let expected = HEADER
        .checked_add(nc.checked_mul(64).ok_or(Error::LimitExceeded)?)
        .and_then(|n| n.checked_add(na * 64))
        .and_then(|n| n.checked_add(count * 16))
        .and_then(|n| n.checked_add(32))
        .ok_or(Error::LimitExceeded)?;
    if expected != bytes.len() {
        return Err(Error::Corrupt("checkpoint provenance geometry"));
    }
    let identity = ChunkIdentity {
        uuid: bytes[16..32].try_into().unwrap(),
        epoch: get64(bytes, 32),
        chunk: get64(bytes, 40),
        logical_generation: get128(bytes, 48),
        capacity: get64(bytes, 64),
    };
    let mut active = ActiveProvenance::new(identity)?;
    let mut creations = Vec::<Creation>::new();
    creations
        .try_reserve_exact(nc)
        .map_err(|_| Error::LimitExceeded)?;
    let mut authorities = Vec::<Checkpoint>::new();
    authorities
        .try_reserve_exact(na)
        .map_err(|_| Error::LimitExceeded)?;
    for row in bytes[HEADER..HEADER + nc * 64].chunks_exact(64) {
        let value = (
            get64(row, 0),
            get64(row, 8),
            get128(row, 16),
            row[32..64].try_into().unwrap(),
        );
        if creations.last().is_some_and(|p| *p >= value) {
            return Err(Error::Corrupt("checkpoint creation dictionary order"));
        }
        creations.push(value);
    }
    let auth_start = HEADER + nc * 64;
    for row in bytes[auth_start..auth_start + na * 64].chunks_exact(64) {
        let value = (
            get128(row, 0),
            get64(row, 16),
            row[24..56].try_into().unwrap(),
        );
        if row[56..] != [0; 8] || authorities.last().is_some_and(|p| *p >= value) {
            return Err(Error::Corrupt(
                "checkpoint authority dictionary order/reserved",
            ));
        }
        authorities.push(value);
    }
    let mut used_c = vec![false; nc];
    let mut used_a = vec![false; na];
    let rows = auth_start + na * 64;
    for row in bytes[rows..rows + count * 16].chunks_exact(16) {
        let ci = get32(row, 8) as usize;
        let ai = get32(row, 12) as usize;
        let &(lsn, sequence, commit, digest) = creations
            .get(ci)
            .ok_or(Error::Corrupt("checkpoint origin ordinal"))?;
        used_c[ci] = true;
        let authority = if ai == 0 {
            Authority::Wal {
                lsn,
                sequence,
                digest,
            }
        } else {
            let &(generation, sequence, digest) = authorities
                .get(ai - 1)
                .ok_or(Error::Corrupt("checkpoint authority ordinal"))?;
            used_a[ai - 1] = true;
            Authority::Checkpoint {
                generation,
                sequence,
                digest,
            }
        };
        active.insert(Origin {
            extent: Extent {
                at: Reference {
                    segment: identity.chunk,
                    offset: u64::from(get32(row, 0)),
                },
                logical_generation: identity.logical_generation,
                length: get32(row, 4),
            },
            receipt: SourceReceipt {
                uuid: identity.uuid,
                epoch: identity.epoch,
                creation_lsn: lsn,
                creation_sequence: sequence,
                creation_commit: commit,
                provenance_digest: digest,
                authority,
            },
        })?;
    }
    if used_c.contains(&false) || used_a.contains(&false) {
        return Err(Error::Corrupt("checkpoint unused origin dictionary"));
    }
    if bounds_words(active.bounds)
        .iter()
        .enumerate()
        .any(|(n, v)| *v != get64(bytes, 96 + n * 8))
    {
        return Err(Error::Corrupt("checkpoint source bounds mismatch"));
    }
    active.seal(get64(bytes, 72), u64::MAX, u64::MAX)
}
