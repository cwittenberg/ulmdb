use std::cell::RefCell;
use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};

/// Checked benchmark-only contiguous private arena, frozen before querying.
/// It does not provide a database, persistence, reclamation or transaction timing.
pub struct Arena {
    pub bytes: Vec<u8>,
}
impl Default for Arena {
    fn default() -> Self {
        Self { bytes: vec![0; 16] }
    }
}
impl ReadStore for Arena {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("benchmark segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
        self.bytes
            .get(start..end)
            .ok_or(Error::Corrupt("benchmark bounds"))
    }
}
impl WriteStore for Arena {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if !alignment.is_power_of_two() {
            return Err(Error::InvalidInput("alignment"));
        }
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.read(old, bytes.len())?;
        if !old.offset.is_multiple_of(alignment as u64) {
            return Err(Error::Corrupt("rewrite alignment"));
        }
        let start = old.offset as usize;
        self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(old)
    }
}

/// Count the union of byte ranges actually reached by a complete checked scan.
/// Overlapping header/body reads count once. This excludes alignment padding.
pub struct Trace<'a> {
    pub arena: &'a Arena,
    pub ranges: RefCell<Vec<(usize, usize)>>,
}
impl ReadStore for Trace<'_> {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        let bytes = self.arena.read(at, length)?;
        self.ranges
            .borrow_mut()
            .push((at.offset as usize, at.offset as usize + length));
        Ok(bytes)
    }
}
impl Trace<'_> {
    pub fn reachable_bytes(&self) -> usize {
        let mut ranges = self.ranges.borrow_mut();
        ranges.sort_unstable();
        let mut total = 0;
        let mut end = 0;
        for &(lo, hi) in ranges.iter() {
            total += hi.saturating_sub(end.max(lo));
            end = end.max(hi);
        }
        total
    }
}
