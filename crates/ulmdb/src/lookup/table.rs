use super::{Algorithm, HashLimits, LookupStats, Native};
use crate::{Error, Result};
use std::{collections::hash_map::RandomState, hash::BuildHasher, mem::size_of, ops::Bound};

#[derive(Clone, Copy)]
struct Entry<'a> {
    hash: u64,
    key: &'a [u8],
    value: &'a [u8],
}

pub(super) struct Table<'a> {
    slots: Vec<Option<Entry<'a>>>,
    hash: RandomState,
    stats: LookupStats,
}

fn capacity(entries: usize) -> Result<usize> {
    if entries == 0 {
        return Ok(0);
    }
    entries
        .checked_mul(2)
        .and_then(usize::checked_next_power_of_two)
        .ok_or(Error::LimitExceeded)
}
pub(super) fn index_bytes(entries: usize) -> Result<usize> {
    capacity(entries)?
        .checked_mul(size_of::<Option<Entry<'_>>>())
        .ok_or(Error::LimitExceeded)
}

impl<'a> Table<'a> {
    pub(super) fn build(native: &Native<'a>, limits: HashLimits) -> Result<Self> {
        // Preflight before allocating the table. Native cursor decoding retains
        // its established validation/resource policy; its workspace is not an
        // index-owned allocation. No unbounded copy of keys or values is made.
        let mut entries = 0usize;
        let mut referenced_key_bytes = 0usize;
        let mut total_value_bytes = 0usize;
        for pair in native.cursor(Bound::Unbounded, Bound::Unbounded, false)? {
            let (key, value) = pair?;
            entries = entries.checked_add(1).ok_or(Error::LimitExceeded)?;
            referenced_key_bytes = referenced_key_bytes
                .checked_add(key.len())
                .ok_or(Error::LimitExceeded)?;
            total_value_bytes = total_value_bytes
                .checked_add(value.len())
                .ok_or(Error::LimitExceeded)?;
            if entries > limits.max_entries
                || value.len() > limits.max_value_bytes
                || total_value_bytes > limits.max_total_value_bytes
            {
                return Err(Error::LimitExceeded);
            }
        }
        let count = capacity(entries)?;
        let expected = index_bytes(entries)?;
        if expected > limits.max_index_bytes {
            return Err(Error::LimitExceeded);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(count)
            .map_err(|_| Error::LimitExceeded)?;
        let allocated = slots
            .capacity()
            .checked_mul(size_of::<Option<Entry<'_>>>())
            .ok_or(Error::LimitExceeded)?;
        if allocated > limits.max_index_bytes {
            return Err(Error::LimitExceeded);
        }
        slots.resize(count, None);
        let mut result = Self {
            slots,
            hash: RandomState::new(),
            stats: LookupStats {
                algorithm: Algorithm::Hash,
                enumerated: true,
                entries,
                capacity: count,
                index_bytes: allocated,
                referenced_key_bytes,
                total_value_bytes,
            },
        };
        let mut inserted = 0usize;
        for pair in native.cursor(Bound::Unbounded, Bound::Unbounded, false)? {
            let (key, value) = pair?;
            if inserted >= entries {
                return Err(Error::Corrupt("hash source count changed"));
            }
            result.insert(key, value)?;
            inserted += 1;
        }
        if inserted != entries {
            return Err(Error::Corrupt("hash source count changed"));
        }
        Ok(result)
    }

    fn insert(&mut self, key: &'a [u8], value: &'a [u8]) -> Result<()> {
        let hash = self.hash.hash_one(key);
        let mask = self.slots.len() - 1;
        let mut slot = hash as usize & mask;
        for _ in 0..self.slots.len() {
            match self.slots[slot] {
                None => {
                    self.slots[slot] = Some(Entry { hash, key, value });
                    return Ok(());
                }
                Some(entry) if entry.hash == hash && entry.key == key => {
                    return Err(Error::Corrupt("duplicate hash source key"));
                }
                Some(_) => slot = (slot + 1) & mask,
            }
        }
        Err(Error::Corrupt("hash capacity exhausted"))
    }

    #[inline]
    pub(super) fn get(&self, key: &[u8]) -> Option<&'a [u8]> {
        if self.slots.is_empty() {
            return None;
        }
        let hash = self.hash.hash_one(key);
        let mask = self.slots.len() - 1;
        let mut slot = hash as usize & mask;
        // One vacant slot is guaranteed by <=50% occupancy; retain a hard bound
        // so even an internal invariant failure cannot spin forever.
        for _ in 0..self.slots.len() {
            match self.slots[slot] {
                None => return None,
                Some(entry) if entry.hash == hash && entry.key == key => return Some(entry.value),
                Some(_) => slot = (slot + 1) & mask,
            }
        }
        None
    }

    pub(super) fn stats(&self) -> LookupStats {
        self.stats
    }
}
