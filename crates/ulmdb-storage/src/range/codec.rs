//! Explicit format5 envelope. Indexed descriptors are logical projections:
//! physical data bindings are in placements, never encoded in cohort IDs.
use super::pack::{BLOCK, MAX_PAYLOAD, Receipt};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
pub(super) const HEADER: usize = 4096;
pub(super) const ORIGIN: usize = 112;
pub(super) const PLACE: usize = 24;
#[derive(Clone, Copy, Debug)]
pub(super) struct Origin {
    pub receipt: Receipt,
    pub header_hash: [u8; 32],
}
#[derive(Clone, Debug)]
pub(super) struct Header {
    pub uuid: [u8; 16],
    pub generation: u128,
    pub data_len: u64,
    pub descriptor_len: u64,
    pub origins: usize,
    pub placements: usize,
    pub payload_len: u64,
    pub segment: u64,
    pub descriptor_hash: [u8; 32],
    pub hash: [u8; 32],
}
fn n<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N]> {
    b.get(at..at + N)
        .ok_or(Error::Corrupt("short range envelope"))?
        .try_into()
        .map_err(|_| Error::Corrupt("range number"))
}
pub(super) fn header(bytes: &[u8]) -> Result<Header> {
    if bytes.len() != HEADER
        || &bytes[..8] != b"ULMDBR05"
        || bytes[8..10] != [0; 2]
        || bytes[10..12] != 5u16.to_le_bytes()
        || bytes[12..16] != (HEADER as u32).to_le_bytes()
    {
        return Err(Error::Corrupt("range envelope version/header"));
    }
    if bytes[128..HEADER - 32].iter().any(|v| *v != 0) {
        return Err(Error::Corrupt("range envelope reserved"));
    }
    let hash: [u8; 32] = Sha256::digest(&bytes[..HEADER - 32]).into();
    if hash != n(bytes, HEADER - 32)? {
        return Err(Error::Corrupt("range envelope header hash"));
    }
    let h = Header {
        uuid: n(bytes, 16)?,
        generation: u128::from_le_bytes(n(bytes, 32)?),
        data_len: u64::from_le_bytes(n(bytes, 48)?),
        descriptor_len: u64::from_le_bytes(n(bytes, 56)?),
        origins: usize::try_from(u64::from_le_bytes(n(bytes, 64)?))
            .map_err(|_| Error::LimitExceeded)?,
        placements: usize::try_from(u64::from_le_bytes(n(bytes, 72)?))
            .map_err(|_| Error::LimitExceeded)?,
        payload_len: u64::from_le_bytes(n(bytes, 80)?),
        segment: u64::from_le_bytes(n(bytes, 88)?),
        descriptor_hash: n(bytes, 96)?,
        hash,
    };
    if h.uuid == [0; 16]
        || h.generation == 0
        || h.origins > 1_048_575
        || h.placements > 1_048_575
        || h.descriptor_len < 256
        || h.descriptor_len > 268_435_456
        || h.data_len > 268_435_456
        || (h.data_len == 0) != (h.segment == 0)
        || (h.data_len != 0 && h.data_len < 4096)
    {
        return Err(Error::Corrupt("range envelope geometry"));
    }
    let total = (HEADER as u64)
        .checked_add(h.data_len)
        .and_then(|n| n.checked_add(h.descriptor_len))
        .and_then(|n| n.checked_add(h.origins as u64 * ORIGIN as u64))
        .and_then(|n| n.checked_add(h.placements as u64 * PLACE as u64))
        .ok_or(Error::LimitExceeded)?;
    if total != h.payload_len || total > MAX_PAYLOAD {
        return Err(Error::Corrupt("range envelope extent"));
    }
    Ok(h)
}
pub(super) fn encode(
    uuid: [u8; 16],
    generation: u128,
    data: &[u8],
    descriptor: &[u8],
    origins: &BTreeMap<u128, Origin>,
    placements: &BTreeMap<u64, u128>,
    segment: u64,
) -> Result<(Vec<u8>, [u8; 32])> {
    if origins.len() > 1_048_575 || placements.len() > 1_048_575 {
        return Err(Error::LimitExceeded);
    }
    let length = HEADER
        .checked_add(data.len())
        .and_then(|n| n.checked_add(descriptor.len()))
        .and_then(|n| n.checked_add(origins.len().checked_mul(ORIGIN)?))
        .and_then(|n| n.checked_add(placements.len().checked_mul(PLACE)?))
        .filter(|n| *n as u64 <= MAX_PAYLOAD)
        .ok_or(Error::LimitExceeded)?;
    let mut b = vec![0; length];
    b[..8].copy_from_slice(b"ULMDBR05");
    b[10..12].copy_from_slice(&5u16.to_le_bytes());
    b[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    b[16..32].copy_from_slice(&uuid);
    b[32..48].copy_from_slice(&generation.to_le_bytes());
    for (at, v) in [
        (48, data.len() as u64),
        (56, descriptor.len() as u64),
        (64, origins.len() as u64),
        (72, placements.len() as u64),
        (80, length as u64),
        (88, segment),
    ] {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    b[96..128].copy_from_slice(&Sha256::digest(descriptor));
    let hash: [u8; 32] = Sha256::digest(&b[..HEADER - 32]).into();
    b[HEADER - 32..HEADER].copy_from_slice(&hash);
    let mut at = HEADER;
    b[at..at + data.len()].copy_from_slice(data);
    at += data.len();
    b[at..at + descriptor.len()].copy_from_slice(descriptor);
    at += descriptor.len();
    for (generation, o) in origins {
        b[at..at + 16].copy_from_slice(&generation.to_le_bytes());
        b[at + 16..at + 24].copy_from_slice(&o.receipt.offset.to_le_bytes());
        b[at + 24..at + 32].copy_from_slice(&o.receipt.ordinal.to_le_bytes());
        b[at + 32..at + 40].copy_from_slice(&o.receipt.payload_length.to_le_bytes());
        b[at + 40..at + 72].copy_from_slice(&o.receipt.hash);
        b[at + 72..at + 104].copy_from_slice(&o.header_hash);
        at += ORIGIN;
    }
    for (id, generation) in placements {
        b[at..at + 8].copy_from_slice(&id.to_le_bytes());
        b[at + 8..at + 24].copy_from_slice(&generation.to_le_bytes());
        at += PLACE;
    }
    header(&b[..HEADER])?;
    Ok((b, hash))
}
pub(super) fn directories(
    bytes: &[u8],
    h: &Header,
    selected: Receipt,
) -> Result<(BTreeMap<u128, Origin>, BTreeMap<u64, u128>)> {
    if bytes.len() as u64 != h.payload_len {
        return Err(Error::Corrupt("range payload length"));
    }
    let mut at = HEADER
        + usize::try_from(h.data_len + h.descriptor_len).map_err(|_| Error::LimitExceeded)?;
    let mut origins = BTreeMap::new();
    let mut prior = 0;
    for _ in 0..h.origins {
        let generation = u128::from_le_bytes(n(bytes, at)?);
        let receipt = Receipt {
            offset: u64::from_le_bytes(n(bytes, at + 16)?),
            ordinal: u64::from_le_bytes(n(bytes, at + 24)?),
            payload_length: u64::from_le_bytes(n(bytes, at + 32)?),
            hash: n(bytes, at + 40)?,
        };
        receipt.validate()?;
        if generation <= prior
            || generation == h.generation
            || receipt.ordinal == 0
            || receipt.ordinal >= selected.ordinal
            || receipt.frame_end()? > selected.offset
            || bytes[at + 104..at + 112] != [0; 8]
        {
            return Err(Error::Corrupt("range origin ordering/extent"));
        }
        prior = generation;
        origins.insert(
            generation,
            Origin {
                receipt,
                header_hash: n(bytes, at + 72)?,
            },
        );
        at += ORIGIN;
    }
    let mut placements = BTreeMap::new();
    let mut prior = 0;
    for _ in 0..h.placements {
        let id = u64::from_le_bytes(n(bytes, at)?);
        let generation = u128::from_le_bytes(n(bytes, at + 8)?);
        if id <= prior
            || id == u64::MAX
            || generation == 0
            || (generation != h.generation && !origins.contains_key(&generation))
        {
            return Err(Error::Corrupt("range placement directory"));
        }
        prior = id;
        placements.insert(id, generation);
        at += PLACE;
    }
    origins.insert(
        h.generation,
        Origin {
            receipt: selected,
            header_hash: h.hash,
        },
    );
    Ok((origins, placements))
}
pub(super) fn data_offset(o: Origin) -> Result<u64> {
    o.receipt
        .offset
        .checked_add(BLOCK as u64 + HEADER as u64)
        .ok_or(Error::LimitExceeded)
}
