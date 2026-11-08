use super::{
    Encoded, FOOTER_BYTES, FOOTER_MAGIC, HEADER_BYTES, MAGIC, MAX_RECORD_BYTES, digest, fields::*,
};
use crate::hybrid::{
    redo::Record,
    view::{Object, View},
};
use crate::{Error, Result};
fn add(total: &mut usize, n: usize) -> Result<()> {
    *total = total
        .checked_add(n)
        .filter(|x| *x <= MAX_RECORD_BYTES)
        .ok_or(Error::LimitExceeded)?;
    Ok(())
}
pub fn encode(record: &Record) -> Result<Encoded> {
    let checked = crate::hybrid::checked::CheckedRecord::new(record)?;
    encode_checked(&checked)
}
/// Private token is issued only after complete structural checks in this call.
pub(crate) fn encode_checked<V: View>(
    checked: &crate::hybrid::checked::CheckedRecord<'_, V>,
) -> Result<Encoded> {
    let mut bytes = Vec::new();
    let digest = encode_checked_into(checked, &mut bytes, MAX_RECORD_BYTES)?;
    Ok(Encoded { bytes, digest })
}
pub(crate) fn encode_checked_into<V: View>(
    checked: &crate::hybrid::checked::CheckedRecord<'_, V>,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<[u8; 32]> {
    let view = checked.record();
    let record = view.header();
    let mut size = HEADER_BYTES + FOOTER_BYTES;
    add(
        &mut size,
        record
            .chunks
            .len()
            .checked_mul(64)
            .ok_or(Error::LimitExceeded)?,
    )?;
    for index in 0..view.object_count() {
        let object = view.object(index);
        add(&mut size, 48)?;
        match object {
            Object::Literal { bytes, .. } => add(&mut size, bytes.len())?,
            Object::BaseCopy { patches, .. } => {
                add(&mut size, 208)?;
                for p in patches {
                    add(&mut size, 8)?;
                    add(&mut size, p.bytes.len())?;
                }
            }
        }
    }
    let length = size
        .checked_add(7)
        .map(|n| n / 8 * 8)
        .filter(|n| *n <= MAX_RECORD_BYTES)
        .ok_or(Error::LimitExceeded)?;
    if length > limit {
        return Err(Error::LimitExceeded);
    }
    out.clear();
    out.try_reserve_exact(length)
        .map_err(|_| Error::LimitExceeded)?;
    out.extend_from_slice(MAGIC);
    u32v(out, 1);
    u32v(out, HEADER_BYTES as u32);
    out.extend_from_slice(&record.uuid);
    u64v(out, record.epoch);
    u64v(out, record.sequence);
    u64v(out, record.lsn);
    u128v(out, record.commit);
    u128v(out, record.parent_commit);
    receipt(out, record.parent);
    out.extend_from_slice(&record.root.catalog.encode());
    u64v(out, record.root.next_chunk);
    u64v(out, record.root.next_database);
    u32v(out, record.chunks.len() as u32);
    u32v(out, view.object_count() as u32);
    u64v(out, length as u64);
    out.resize(HEADER_BYTES, 0);
    for c in &record.chunks {
        u64v(out, c.chunk);
        u128v(out, c.logical_generation);
        u64v(out, c.before);
        u64v(out, c.after);
        u64v(out, c.capacity);
        out.extend_from_slice(&[0; 16]);
    }
    for index in 0..view.object_count() {
        let object = view.object(index);
        match object {
            Object::Literal { dest, bytes } => {
                u64v(out, 0);
                extent(out, dest);
                out.extend_from_slice(bytes);
            }
            Object::BaseCopy {
                dest,
                source: base,
                receipt: proof,
                patches,
            } => {
                u64v(out, 1);
                extent(out, dest);
                extent(out, *base);
                source(out, *proof);
                u32v(out, patches.len() as u32);
                u32v(out, 0);
                for p in patches {
                    u32v(out, p.offset);
                    u32v(out, p.bytes.len() as u32);
                    out.extend_from_slice(p.bytes);
                }
            }
        }
    }
    if out.len() > length - FOOTER_BYTES {
        return Err(Error::Corrupt("hybrid encoder length arithmetic"));
    }
    out.resize(length - FOOTER_BYTES, 0);
    let hash = digest(out);
    out.extend_from_slice(FOOTER_MAGIC);
    u64v(out, length as u64);
    u64v(out, record.lsn);
    u64v(out, record.sequence);
    out.extend_from_slice(&hash);
    debug_assert_eq!(out.len(), length);
    Ok(hash)
}
