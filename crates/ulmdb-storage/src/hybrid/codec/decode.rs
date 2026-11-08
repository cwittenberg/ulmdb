use super::{
    Decoded, FOOTER_BYTES, FOOTER_MAGIC, HEADER_BYTES, MAGIC, MAX_RECORD_BYTES, digest,
    fields::Cursor,
};
use crate::hybrid::redo::{
    ChunkDelta, MAX_CHUNKS, MAX_OBJECTS, MAX_PATCHES, ObjectRedo, Patch, Record, RootState,
    WalReceipt, copy_bytes,
};
use crate::{Error, Result};
pub fn decode(bytes: &[u8], offset: u64) -> Result<Decoded> {
    if bytes.len() < HEADER_BYTES + FOOTER_BYTES
        || bytes.len() > MAX_RECORD_BYTES
        || !bytes.len().is_multiple_of(8)
        || !offset.is_multiple_of(8)
    {
        return Err(Error::Corrupt("hybrid record frame extent"));
    }
    let split = bytes.len() - FOOTER_BYTES;
    let mut f = Cursor {
        bytes: &bytes[split..],
        at: 0,
    };
    if f.take(8)? != FOOTER_MAGIC || f.u64()? != bytes.len() as u64 {
        return Err(Error::Corrupt("hybrid footer/length"));
    }
    let footer_lsn = f.u64()?;
    let footer_sequence = f.u64()?;
    let expected = f.array()?;
    if digest(&bytes[..split]) != expected {
        return Err(Error::Corrupt("hybrid canonical record digest"));
    }
    let mut h = Cursor {
        bytes: &bytes[..HEADER_BYTES],
        at: 0,
    };
    if h.take(8)? != MAGIC || h.u32()? != 1 || h.u32()? != HEADER_BYTES as u32 {
        return Err(Error::Corrupt("hybrid record version/header"));
    }
    let uuid = h.array()?;
    let epoch = h.u64()?;
    let sequence = h.u64()?;
    let lsn = h.u64()?;
    let commit = h.u128()?;
    let parent_commit = h.u128()?;
    let parent = h.receipt()?;
    let root = RootState {
        catalog: h.reference()?,
        next_chunk: h.u64()?,
        next_database: h.u64()?,
    };
    let chunk_count = h.u32()? as usize;
    let object_count = h.u32()? as usize;
    let length = h.u64()?;
    h.zero(h.remaining())?;
    if length != bytes.len() as u64
        || lsn != footer_lsn
        || sequence != footer_sequence
        || chunk_count > MAX_CHUNKS
        || object_count > MAX_OBJECTS
    {
        return Err(Error::Corrupt("hybrid record counts/footer identity"));
    }
    let mut c = Cursor {
        bytes: &bytes[HEADER_BYTES..split],
        at: 0,
    };
    let min = chunk_count
        .checked_mul(64)
        .and_then(|n| object_count.checked_mul(48).and_then(|m| n.checked_add(m)))
        .ok_or(Error::LimitExceeded)?;
    if min > c.remaining() {
        return Err(Error::Corrupt("hybrid counts exceed record"));
    }
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(chunk_count)
        .map_err(|_| Error::LimitExceeded)?;
    for _ in 0..chunk_count {
        let d = ChunkDelta {
            chunk: c.u64()?,
            logical_generation: c.u128()?,
            before: c.u64()?,
            after: c.u64()?,
            capacity: c.u64()?,
        };
        c.zero(16)?;
        chunks.push(d);
    }
    let mut objects = Vec::new();
    objects
        .try_reserve_exact(object_count)
        .map_err(|_| Error::LimitExceeded)?;
    for _ in 0..object_count {
        let tag = c.u64()?;
        let dest = c.extent()?;
        let object = match tag {
            0 => ObjectRedo::Literal {
                dest,
                bytes: copy_bytes(c.take(dest.length as usize)?)?,
            },
            1 => {
                let source = c.extent()?;
                let receipt = c.source()?;
                let count = c.u32()? as usize;
                c.zero(4)?;
                if count > MAX_PATCHES
                    || count.checked_mul(8).ok_or(Error::LimitExceeded)? > c.remaining()
                {
                    return Err(Error::Corrupt("hybrid patch count"));
                }
                let mut patches = Vec::new();
                patches
                    .try_reserve_exact(count)
                    .map_err(|_| Error::LimitExceeded)?;
                for _ in 0..count {
                    let offset = c.u32()?;
                    let n = c.u32()? as usize;
                    patches.push(Patch {
                        offset,
                        bytes: copy_bytes(c.take(n)?)?,
                    });
                }
                ObjectRedo::BaseCopy {
                    dest,
                    source,
                    receipt,
                    patches,
                }
            }
            _ => return Err(Error::Corrupt("hybrid object tag")),
        };
        objects.push(object);
    }
    if c.remaining() > 7 {
        return Err(Error::Corrupt("hybrid noncanonical trailing payload"));
    }
    c.zero(c.remaining())?;
    let record = Record {
        uuid,
        epoch,
        lsn,
        sequence,
        commit,
        parent_commit,
        parent,
        root,
        chunks,
        objects,
    };
    record.validate()?;
    let receipt = WalReceipt {
        lsn,
        sequence,
        offset,
        length,
        digest: expected,
    };
    receipt.end()?;
    Ok(Decoded { record, receipt })
}
