//! Writer-private prefix closures, admitted by complete typed audits. No View,
//! Snapshot, Version, byte borrow or serialized authority is retained here.
mod audit;
mod capture;
mod control;
use super::catalog_codec;
use crate::{DatabaseKind, Error, Result, catalog::Database};
use capture::Capture;
use std::{
    mem::size_of,
    sync::{Arc, Weak},
};
use ulmdb_storage::{
    Reference,
    resident::{Pool, VersionKey, View},
};

/// Accounted owned metadata, excluding allocator overhead/RSS and the existing
/// full auditor's live set. Retained/prepared/scratch phases each use <=1/3 of
/// max_bytes; only one of each can exist under the serialized writer.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_entries: usize,
    /// Membership and zero-length source probes per Collect; exhaustion falls
    /// back to the full graph audit, rather than trusting an unchecked closure.
    pub max_validations: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 8 << 20,
            max_entries: 64,
            max_validations: 2_000_000,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<()> {
        if self.max_bytes > 32 << 20 || self.max_entries == 0 || self.max_entries > 4096 {
            return Err(Error::InvalidInput("resident Collect cache limits"));
        }
        Ok(())
    }
    fn phase(self) -> usize {
        self.max_bytes / 3
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub hits: u64,
    pub misses: u64,
    pub membership_checks: u64,
    pub endpoint_checks: u64,
    pub admitted: u64,
    pub admission_skips: u64,
    pub entries: usize,
    pub retained_bytes: usize,
    pub byte_limit: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct EmptyBinding {
    at: Reference,
    version: VersionKey,
}
struct Entry {
    name: Vec<u8>,
    id: u64,
    kind: DatabaseKind,
    root: Reference,
    keys: Vec<VersionKey>,
    empty: Vec<EmptyBinding>,
    records: u64,
    charged: usize,
}
impl Entry {
    fn charge(name_capacity: usize, key_capacity: usize, empty_capacity: usize) -> Option<usize> {
        size_of::<Self>()
            .checked_add(2 * size_of::<usize>() + std::mem::align_of::<Self>())?
            .checked_add(name_capacity)?
            .checked_add(key_capacity.checked_mul(size_of::<VersionKey>())?)?
            .checked_add(empty_capacity.checked_mul(size_of::<EmptyBinding>())?)
    }
}
/// Not Clone: a Collect owns at most the retained cache and one private next
/// cache. Entry Arcs share hit data; conservative accounting charges both lists.
pub(super) struct Cache {
    owner: Weak<Pool>,
    entries: Vec<Arc<Entry>>,
    limits: Limits,
    charged: usize,
    pub(super) stats: Stats,
}
impl Cache {
    pub(super) fn new(pool: &Arc<Pool>, limits: Limits) -> Result<Self> {
        limits.validate()?;
        let requested = limits
            .max_entries
            .checked_mul(size_of::<Arc<Entry>>())
            .and_then(|n| n.checked_add(size_of::<Self>()))
            .ok_or(Error::LimitExceeded)?;
        if requested > limits.phase() {
            return Err(Error::LimitExceeded);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(limits.max_entries)
            .map_err(|_| Error::LimitExceeded)?;
        let charged = entries
            .capacity()
            .checked_mul(size_of::<Arc<Entry>>())
            .and_then(|n| n.checked_add(size_of::<Self>()))
            .ok_or(Error::LimitExceeded)?;
        if charged > limits.phase() {
            return Err(Error::LimitExceeded);
        }
        Ok(Self {
            owner: Arc::downgrade(pool),
            entries,
            limits,
            charged,
            stats: Stats {
                retained_bytes: charged,
                byte_limit: limits.max_bytes,
                ..Stats::default()
            },
        })
    }
    fn find(&self, pool: &Arc<Pool>, name: &[u8], db: &Database) -> Option<&Arc<Entry>> {
        if !Weak::ptr_eq(&self.owner, &Arc::downgrade(pool)) {
            return None;
        }
        let i = self.entries.binary_search_by_key(&db.id, |e| e.id).ok()?;
        let entry = &self.entries[i];
        (entry.name == name && entry.kind == db.kind && entry.root == db.root).then_some(entry)
    }
    fn usable(&self, view: &View, entry: &Entry, stats: &mut Stats) -> bool {
        let Some(required) = entry
            .keys
            .len()
            .checked_add(entry.empty.len())
            .and_then(|n| u64::try_from(n).ok())
        else {
            return false;
        };
        let used = stats.membership_checks + stats.endpoint_checks;
        if required > self.limits.max_validations.saturating_sub(used) {
            return false;
        }
        for key in &entry.keys {
            stats.membership_checks += 1;
            if !view.contains_version(*key) {
                return false;
            }
        }
        // R2 current-first empty endpoints can acquire a different adjacent
        // owner even when all original object bytes and versions remain live.
        for binding in &entry.empty {
            stats.endpoint_checks += 1;
            if !view
                .read_with_key(binding.at, 0)
                .is_ok_and(|(_, key)| key == binding.version)
            {
                return false;
            }
        }
        true
    }
    fn admit(&mut self, entry: Arc<Entry>) -> bool {
        if self.entries.len() == self.limits.max_entries
            || entry.charged > self.limits.phase().saturating_sub(self.charged)
        {
            return false;
        }
        self.charged += entry.charged;
        self.entries.push(entry);
        true
    }
    pub(super) fn audit(
        &self,
        pool: &Arc<Pool>,
        view: &View,
        root: Reference,
        next_database: u64,
        budget: u64,
    ) -> Result<(super::audit::Audit, Self)> {
        audit::run(self, pool, view, root, next_database, budget)
    }
}
#[cfg(test)]
mod tests;
