//! New format6 scalar/root descriptor; no v4 encoder or NodeRef interpretation.
use super::{reference::PhysicalRef, tree::Node};
use crate::{Error, Inventory, Manifest, Reference, Result};
use sha2::{Digest, Sha256};
use std::sync::Arc;
pub(super) const HEADER: usize = 4096;
pub(super) const FEATURES: u64 = 35; // canonical segment v2 + required direct-locator feature32
pub(super) struct Header {
    pub manifest: Manifest,
    pub root: Option<PhysicalRef>,
    pub data_len: u64,
    pub blob_len: u64,
    pub own_id: u64,
    pub local: u32,
    pub file: [u8; 16],
    pub count: usize,
}
fn num<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N]> {
    b.get(at..at + N)
        .ok_or(Error::Corrupt("short direct root"))?
        .try_into()
        .map_err(|_| Error::Corrupt("direct field width"))
}
pub(super) fn scalar_bytes(m: &Manifest) -> [u8; 192] {
    let mut b = [0; 192];
    b[..8].copy_from_slice(b"ULMDBR06");
    b[10..12].copy_from_slice(&6u16.to_le_bytes());
    b[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    b[16..32].copy_from_slice(&m.uuid);
    b[32..48].copy_from_slice(&m.generation.to_le_bytes());
    b[48..56].copy_from_slice(&m.epoch.to_le_bytes());
    b[56..64].copy_from_slice(&m.sequence.to_le_bytes());
    b[64..80].copy_from_slice(&m.commit.to_le_bytes());
    b[80..96].copy_from_slice(&m.parent.to_le_bytes());
    b[96..112].copy_from_slice(&m.catalog.encode());
    b[112..116].copy_from_slice(&(m.segments.len() as u32).to_le_bytes());
    b[120..128].copy_from_slice(&FEATURES.to_le_bytes());
    b[128..144].copy_from_slice(&m.previous.to_le_bytes());
    b[176..184].copy_from_slice(&m.next_database.to_le_bytes());
    b[184..192].copy_from_slice(&m.next_segment.to_le_bytes());
    b
}
pub(super) fn scalar_hash(m: &Manifest) -> [u8; 32] {
    Sha256::digest(scalar_bytes(m)).into()
}
pub(super) fn blob_offset(frame_start: u64, data_len: u64) -> Result<u64> {
    frame_start
        .checked_add(8192)
        .and_then(|n| n.checked_add(data_len))
        .and_then(|n| n.checked_add(31))
        .map(|n| n / 32 * 32)
        .ok_or(Error::LimitExceeded)
}
pub(super) struct EncodeInput<'a> {
    pub file: [u8; 16],
    pub frame_start: u64,
    pub manifest: &'a Manifest,
    pub data: &'a [u8],
    pub root: Option<&'a Arc<Node>>,
    pub blob: &'a [u8],
    pub local: u64,
    pub own_id: u64,
}
pub(super) fn encode(input: EncodeInput<'_>) -> Result<Vec<u8>> {
    let EncodeInput {
        file,
        frame_start,
        manifest,
        data,
        root,
        blob,
        local,
        own_id,
    } = input;
    let start = usize::try_from(blob_offset(frame_start, data.len() as u64)? - frame_start - 4096)
        .map_err(|_| Error::LimitExceeded)?;
    let length = start
        .checked_add(blob.len())
        .filter(|n| *n as u64 <= super::super::pack::MAX_PAYLOAD)
        .ok_or(Error::LimitExceeded)?;
    let mut b = vec![0; length];
    b[..192].copy_from_slice(&scalar_bytes(manifest));
    b[116..120].copy_from_slice(
        &u32::try_from(local)
            .map_err(|_| Error::LimitExceeded)?
            .to_le_bytes(),
    );
    if let Some(root) = root {
        b[192..256].copy_from_slice(
            &root
                .location
                .ok_or(Error::InvalidInput("unsealed direct root"))?
                .encode(),
        );
    }
    b[256..264].copy_from_slice(&(data.len() as u64).to_le_bytes());
    b[264..272].copy_from_slice(&(blob.len() as u64).to_le_bytes());
    b[272..280].copy_from_slice(&(length as u64).to_le_bytes());
    b[280..288].copy_from_slice(&own_id.to_le_bytes());
    b[320..336].copy_from_slice(&file);
    let h = Sha256::digest(&b[..HEADER - 32]);
    b[HEADER - 32..HEADER].copy_from_slice(&h);
    b[HEADER..HEADER + data.len()].copy_from_slice(data);
    b[start..].copy_from_slice(blob);
    Ok(b)
}
pub(super) fn decode(b: &[u8], frame_start: u64) -> Result<Header> {
    if b.len() < HEADER
        || &b[..8] != b"ULMDBR06"
        || b[8..10] != [0; 2]
        || b[10..12] != 6u16.to_le_bytes()
        || b[12..16] != (HEADER as u32).to_le_bytes()
        || b[120..128] != FEATURES.to_le_bytes()
        || b[144..176] != [0; 32]
        || b[288..320] != [0; 32]
        || b[336..HEADER - 32].iter().any(|v| *v != 0)
        || <[u8; 32]>::from(Sha256::digest(&b[..HEADER - 32])) != b[HEADER - 32..HEADER]
    {
        return Err(Error::Corrupt("direct root header/features/hash"));
    }
    let count = u32::from_le_bytes(num(b, 112)?) as usize;
    let local = u32::from_le_bytes(num(b, 116)?);
    let root = PhysicalRef::decode(&b[192..256])?;
    let data_len = u64::from_le_bytes(num(b, 256)?);
    let blob_len = u64::from_le_bytes(num(b, 264)?);
    let payload_len = u64::from_le_bytes(num(b, 272)?);
    let own_id = u64::from_le_bytes(num(b, 280)?);
    let file = num(b, 320)?;
    let relative = blob_offset(frame_start, data_len)?
        .checked_sub(frame_start + 4096)
        .ok_or(Error::Corrupt("direct blob underflow"))?;
    if count >= 1_048_576
        || local > 65
        || local as u64 * 96 > blob_len
        || blob_len > local as u64 * 160
        || (count == 0) != root.is_none()
        || data_len > super::super::pack::MAX_PAYLOAD
        || (data_len == 0) != (own_id == 0)
        || own_id == u64::MAX
        || (data_len != 0 && data_len < 4096)
        || payload_len != b.len() as u64
        || payload_len != relative.checked_add(blob_len).ok_or(Error::LimitExceeded)?
        || payload_len > super::super::pack::MAX_PAYLOAD
        || file == [0; 16]
    {
        return Err(Error::Corrupt("direct root extent/count"));
    }
    let data_end = HEADER + usize::try_from(data_len).map_err(|_| Error::LimitExceeded)?;
    let blob_start = usize::try_from(relative).map_err(|_| Error::LimitExceeded)?;
    if b[data_end..blob_start].iter().any(|v| *v != 0) {
        return Err(Error::Corrupt("direct root alignment padding"));
    }
    let manifest = Manifest {
        transform: None,
        page_size: Default::default(),
        value_limit: Default::default(),
        format_minor: 6,
        uuid: num(b, 16)?,
        generation: u128::from_le_bytes(num(b, 32)?),
        epoch: u64::from_le_bytes(num(b, 48)?),
        sequence: u64::from_le_bytes(num(b, 56)?),
        commit: u128::from_le_bytes(num(b, 64)?),
        parent: u128::from_le_bytes(num(b, 80)?),
        previous: u128::from_le_bytes(num(b, 128)?),
        catalog: Reference::decode(&b[96..112])?,
        next_database: u64::from_le_bytes(num(b, 176)?),
        next_segment: u64::from_le_bytes(num(b, 184)?),
        segments: Inventory::range_from_entries(Vec::new())?,
        index: None,
    };
    if manifest.uuid == [0; 16]
        || manifest.generation == 0
        || manifest.generation == manifest.previous
        || manifest.commit == 0
        || manifest.epoch == 0
        || manifest.next_database == 0
        || manifest.next_segment == 0
    {
        return Err(Error::Corrupt("direct root scalar identity"));
    }
    // Keep expected count separate until the authenticated tree is decoded.
    let mut h = Header {
        manifest,
        root,
        data_len,
        blob_len,
        own_id,
        local,
        file,
        count,
    };
    h.manifest.segments = Inventory::default();
    Ok(h)
}
