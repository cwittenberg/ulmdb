use super::*;
use crate::resident::{
    extent::*,
    pool::{PAGE, tagged},
};
use crate::{Error, ReadStore, Reference, Result};
use sha2::{Digest, Sha256};
impl Arena {
    pub fn overflow_record_bound(&self, value_bytes: usize) -> Result<u64> {
        if value_bytes > MAX_OVERFLOW_VALUE_BYTES {
            return Err(Error::InvalidInput("resident value limit16MiB"));
        }
        let chunks = (value_bytes + 16).div_ceil(PAGE);
        let count = chunks + chunks.div_ceil(TABLE_ITEMS) + 1;
        (self.private.len() as u64 + count as u64 + 256)
            .checked_mul(6148)
            .and_then(|n| n.checked_add(self.base.member_count() as u64 * 16 + 512))
            .ok_or(Error::LimitExceeded)
    }
    pub(super) fn read_overflow(&self, at: Reference, len: usize) -> Result<&[u8]> {
        if let Some(p) = self.pending_extents.get(&at.segment) {
            return p.backing.read(at.offset, len);
        }
        self.base.read(at, len)
    }
    pub(super) fn put_overflow(&mut self, value: &[u8]) -> Result<Reference> {
        self.pool.ensure_live()?;
        if value.len() <= 4080 || value.len() > MAX_OVERFLOW_VALUE_BYTES {
            return Err(Error::InvalidInput("resident overflow size"));
        }
        let total = value.len() + 16;
        let chunks = total.div_ceil(PAGE);
        let tables = chunks.div_ceil(TABLE_ITEMS);
        let count = chunks + tables + 1;
        self.allocation_serial
            .checked_add(count as u64)
            .ok_or(Error::LimitExceeded)?;
        let metadata = self
            .pool
            .private_quota
            .reserve((count * 128 + 512) as u64)?;
        let (mut map, mapped, decoded) = self.pool.extent_backing(total)?;
        map[..2].copy_from_slice(&1u16.to_le_bytes());
        map[4..8].copy_from_slice(&16u32.to_le_bytes());
        map[8..16].copy_from_slice(&(value.len() as u64).to_le_bytes());
        map[16..].copy_from_slice(value);
        let digest = Sha256::digest(&map[..]);
        let mut permits = Vec::new();
        permits
            .try_reserve_exact(count)
            .map_err(|_| Error::LimitExceeded)?;
        for _ in 0..chunks + tables {
            permits.push(self.pool.reserve(false)?)
        }
        permits.push(self.pool.reserve(true)?);
        let mut refs = Vec::new();
        refs.try_reserve_exact(count)
            .map_err(|_| Error::LimitExceeded)?;
        for p in &mut permits {
            p.ranges.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
            refs.push(tagged(p.slot, p.generation, 0));
        }
        for (index, p) in permits[..chunks].iter_mut().enumerate() {
            let mut page = [0; PAGE];
            let used = usize::min(PAGE, total - index * PAGE);
            page[..used].copy_from_slice(&map[index * PAGE..index * PAGE + used]);
            p.write(0, &page)?;
            p.ranges.push((0, PAGE as u16));
            p.cursor = PAGE;
        }
        for (index, p) in permits[chunks..chunks + tables].iter_mut().enumerate() {
            let n = usize::min(TABLE_ITEMS, chunks - index * TABLE_ITEMS);
            let mut page = [0; PAGE];
            page[..8].copy_from_slice(TABLE_MAGIC);
            page[8..12].copy_from_slice(&(n as u32).to_le_bytes());
            page[12..16].copy_from_slice(&(index as u32).to_le_bytes());
            page[16..24].copy_from_slice(&(chunks as u64).to_le_bytes());
            page[24..40].copy_from_slice(
                &if index + 1 == tables {
                    Reference::NULL
                } else {
                    refs[chunks + index + 1]
                }
                .encode(),
            );
            for i in 0..n {
                page[TABLE_HEADER + i * 16..TABLE_HEADER + (i + 1) * 16]
                    .copy_from_slice(&refs[index * TABLE_ITEMS + i].encode())
            }
            p.write(0, &page)?;
            p.ranges.push((0, PAGE as u16));
            p.cursor = PAGE;
        }
        let mut header = [0; ROOT_BYTES];
        header[..8].copy_from_slice(ROOT_MAGIC);
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        header[12..16].copy_from_slice(&(ROOT_BYTES as u32).to_le_bytes());
        header[16..24].copy_from_slice(&(value.len() as u64).to_le_bytes());
        header[24..28].copy_from_slice(&(chunks as u32).to_le_bytes());
        header[28..32].copy_from_slice(&(tables as u32).to_le_bytes());
        header[32..48].copy_from_slice(&refs[chunks].encode());
        header[48..].copy_from_slice(&digest);
        let root = permits.last_mut().unwrap();
        root.write(0, &header)?;
        root.ranges.push((0, ROOT_BYTES as u16));
        root.cursor = ROOT_BYTES;
        let descriptor = *refs.last().unwrap();
        let backing = Backing::seal(&self.pool, map, mapped, decoded)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(count)
            .map_err(|_| Error::LimitExceeded)?;
        sources.push((descriptor, ROOT_BYTES, 0));
        for r in &refs[chunks..chunks + tables] {
            sources.push((*r, PAGE, 1))
        }
        for r in &refs[..chunks] {
            sources.push((*r, PAGE, 2))
        }
        self.tick()?;
        for p in permits {
            let slot = p.slot;
            let mut private = Private::new(p);
            private.protected = true;
            self.insert_private(slot, private)?;
        }
        self.pending_extents.insert(
            descriptor.offset,
            Pending {
                backing,
                refs: sources,
                serial: self.allocation_serial,
                metadata,
            },
        );
        Ok(Reference {
            segment: descriptor.offset,
            offset: 0,
        })
    }
}
