//! First-touch page undo for private Resident pages. Abort releases new permits;
//! pool generations remain consumed. No immutable View or pool is copied.
use super::{Arena, Private};
use crate::{Error, Result};
use std::{collections::BTreeMap, sync::Arc};
const MAX_DEPTH: usize = 64;
const MAX_UNDO_BYTES: usize = 256 << 20;
// Full page, <=256 allocation ranges, and conservative map/header overhead.
const PAGE_CHARGE: usize = 4096 + 2048;
#[derive(Debug)]
pub struct ArenaCheckpoint {
    owner: Arc<()>,
    id: u64,
}
struct Before {
    bytes: Vec<u8>,
    ranges: Vec<(u16, u16)>,
    cursor: usize,
}
struct Frame {
    id: u64,
    boundary: u64,
    revision: u64,
    tiny: Option<u32>,
    before: BTreeMap<u32, Before>,
}
pub(super) struct Journal {
    owner: Arc<()>,
    next: u64,
    frames: Vec<Frame>,
    bytes: usize,
    reservation: super::super::private_quota::Reservation,
    base_charge: u64,
}
impl Journal {
    fn new(pool: &super::super::pool::Pool) -> Result<Self> {
        let base_charge =
            (MAX_DEPTH * std::mem::size_of::<Frame>() + std::mem::size_of::<Self>() + 128) as u64;
        let mut reservation = pool.private_quota.reserve(base_charge)?;
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(MAX_DEPTH)
            .map_err(|_| Error::LimitExceeded)?;
        let base_charge =
            base_charge + ((frames.capacity() - MAX_DEPTH) * std::mem::size_of::<Frame>()) as u64;
        reservation.reconcile(base_charge)?;
        Ok(Self {
            owner: Arc::new(()),
            next: 1,
            frames,
            bytes: 0,
            reservation,
            base_charge,
        })
    }
    fn position(&self, token: &ArenaCheckpoint) -> Result<usize> {
        if !Arc::ptr_eq(&self.owner, &token.owner) {
            return Err(Error::InvalidInput("foreign resident checkpoint"));
        }
        self.frames
            .iter()
            .position(|f| f.id == token.id)
            .ok_or(Error::InvalidInput("inactive resident checkpoint"))
    }
    fn remember(&mut self, slot: u32, page: &Private) -> Result<()> {
        let needed = self
            .frames
            .iter()
            .filter(|f| page.born <= f.boundary && !f.before.contains_key(&slot))
            .count();
        let total = self
            .bytes
            .checked_add(
                needed
                    .checked_mul(PAGE_CHARGE)
                    .ok_or(Error::LimitExceeded)?,
            )
            .ok_or(Error::LimitExceeded)?;
        if total > MAX_UNDO_BYTES {
            return Err(Error::LimitExceeded);
        }
        // Preallocate every image before installing any ancestor before-image.
        self.reservation
            .reconcile(self.base_charge + total as u64)?;
        let images_result = (|| {
            let mut images = Vec::new();
            images
                .try_reserve_exact(needed)
                .map_err(|_| Error::LimitExceeded)?;
            for _ in 0..needed {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(4096)
                    .map_err(|_| Error::LimitExceeded)?;
                bytes.extend_from_slice(page.snapshot_bytes());
                let mut ranges = Vec::new();
                ranges
                    .try_reserve_exact(page.ranges.len())
                    .map_err(|_| Error::LimitExceeded)?;
                ranges.extend_from_slice(&page.ranges);
                images.push(Before {
                    bytes,
                    ranges,
                    cursor: page.cursor,
                });
            }
            Ok(images)
        })();
        let mut images = match images_result {
            Ok(images) => images,
            Err(error) => {
                self.reservation
                    .reconcile(self.base_charge + self.bytes as u64)?;
                return Err(error);
            }
        };
        for frame in &mut self.frames {
            if page.born <= frame.boundary && !frame.before.contains_key(&slot) {
                frame.before.insert(slot, images.pop().unwrap());
            }
        }
        self.bytes = total;
        Ok(())
    }
}
impl Arena {
    pub const MAX_SAVEPOINT_DEPTH: usize = MAX_DEPTH;
    pub const MAX_SAVEPOINT_UNDO_BYTES: usize = MAX_UNDO_BYTES;
    pub fn savepoint_depth(&self) -> usize {
        self.journal.as_ref().map_or(0, |j| j.frames.len())
    }
    pub fn savepoint_undo_bytes(&self) -> usize {
        self.journal.as_ref().map_or(0, |j| j.bytes)
    }
    pub fn checkpoint(&mut self) -> Result<ArenaCheckpoint> {
        self.pool.ensure_live()?;
        if self.journal.is_none() {
            self.journal = Some(Journal::new(&self.pool)?);
        }
        let journal = self.journal.as_mut().unwrap();
        if journal.frames.len() == MAX_DEPTH {
            return Err(Error::LimitExceeded);
        }
        let next = journal.next.checked_add(1).ok_or(Error::LimitExceeded)?;
        journal
            .frames
            .try_reserve(1)
            .map_err(|_| Error::LimitExceeded)?;
        let id = journal.next;
        journal.frames.push(Frame {
            id,
            boundary: self.allocation_serial,
            revision: self.revision,
            tiny: self.tiny,
            before: BTreeMap::new(),
        });
        journal.next = next;
        // Do not append child objects into an older tiny generation: rollback
        // would otherwise make the same (generation,offset) allocatable twice.
        self.tiny = None;
        Ok(ArenaCheckpoint {
            owner: Arc::clone(&journal.owner),
            id,
        })
    }
    pub fn release_checkpoint(&mut self, token: &ArenaCheckpoint) -> Result<()> {
        self.pool.ensure_live()?;
        let journal = self
            .journal
            .as_mut()
            .ok_or(Error::InvalidInput("inactive resident checkpoint"))?;
        let index = journal.position(token)?;
        if index + 1 != journal.frames.len() {
            return Err(Error::InvalidInput(
                "release newest resident checkpoint first",
            ));
        }
        let frame = journal.frames.pop().unwrap();
        let charge = frame.before.len() * PAGE_CHARGE;
        drop(frame);
        journal.bytes -= charge;
        journal
            .reservation
            .reconcile(journal.base_charge + journal.bytes as u64)?;
        Ok(())
    }
    pub fn rollback_checkpoint(&mut self, token: &ArenaCheckpoint) -> Result<()> {
        self.pool.ensure_live()?;
        let journal = self
            .journal
            .as_mut()
            .ok_or(Error::InvalidInput("inactive resident checkpoint"))?;
        let index = journal.position(token)?;
        let boundary = journal.frames[index].boundary;
        for (&slot, before) in &journal.frames[index].before {
            let page = self
                .private
                .get(&slot)?
                .ok_or(Error::Corrupt("resident undo page missing"))?;
            if page.born > boundary || before.bytes.len() != 4096 {
                return Err(Error::Corrupt("resident undo boundary"));
            }
        }
        self.private.retain(|p| p.born <= boundary)?;
        while journal.frames.len() > index + 1 {
            let f = journal.frames.pop().unwrap();
            journal.bytes -= f.before.len() * PAGE_CHARGE;
        }
        let frame = journal.frames.pop().unwrap();
        journal.bytes -= frame.before.len() * PAGE_CHARGE;
        for (slot, before) in frame.before {
            let page = self.private.get_mut(&slot)?.unwrap();
            page.write(0, &before.bytes)?;
            page.ranges = before.ranges;
            page.cursor = before.cursor;
        }
        self.pending_extents.retain(|_, p| p.serial <= boundary);
        self.tiny = frame.tiny;
        self.revision = frame.revision;
        journal
            .reservation
            .reconcile(journal.base_charge + journal.bytes as u64)?;
        Ok(())
    }
    pub(super) fn remember_private(&mut self, slot: u32) -> Result<()> {
        if self.private.get(&slot)?.is_some_and(|p| p.protected) {
            return Err(Error::InvalidInput("immutable private overflow allocation"));
        }
        if let Some(journal) = &mut self.journal {
            if journal.frames.is_empty() {
                return Ok(());
            }
            let page = self
                .private
                .get(&slot)?
                .ok_or(Error::Corrupt("resident undo private missing"))?;
            journal.remember(slot, page)?;
        }
        Ok(())
    }
    pub(super) fn insert_private(&mut self, slot: u32, mut page: Private) -> Result<()> {
        let serial = self
            .allocation_serial
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        page.born = serial;
        self.private.insert(slot, page)?;
        self.allocation_serial = serial;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resident::{Config, Pool};
    use crate::{ReadStore, WriteStore};
    fn arena() -> Arena {
        let pool = Pool::create(Config {
            kv_pages: 10,
            tiny_pages: 8,
            max_reserved_bytes: 1 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 16,
        })
        .unwrap();
        pool.begin(pool.empty().unwrap()).unwrap()
    }
    #[test]
    fn first_touch_undo_nested_release_and_all_private_mutation_paths() {
        let mut a = arena();
        let page = a.allocate(&[1; 4096], 4096).unwrap();
        let tiny = a.allocate(b"parent", 16).unwrap();
        let before = a.pool.stats().unwrap().allocations;
        let outer = a.checkpoint().unwrap();
        assert_eq!(a.pool.stats().unwrap().allocations, before);
        assert_eq!(a.savepoint_undo_bytes(), 0);
        a.patch(page, 4096, 10, b"outer", 4096).unwrap();
        let charge = a.savepoint_undo_bytes();
        assert_eq!(charge, PAGE_CHARGE);
        a.patch(page, 4096, 20, b"again", 4096).unwrap();
        assert_eq!(a.savepoint_undo_bytes(), charge);
        a.rewrite(tiny, b"OUTER!", 16).unwrap();
        let inner = a.checkpoint().unwrap();
        a.rewrite(page, &[3; 4096], 4096).unwrap();
        #[cfg(feature = "resident-direct-page-patch")]
        a.try_patch_many(page, 4096, &[(0, b"multi"), (300, b"span")], 4096)
            .unwrap()
            .unwrap();
        a.rewrite(tiny, b"INNER!", 16).unwrap();
        a.release_checkpoint(&inner).unwrap();
        assert_eq!(a.read(tiny, 6).unwrap(), b"INNER!");
        a.rollback_checkpoint(&outer).unwrap();
        assert_eq!(a.read(page, 4096).unwrap(), &[1; 4096]);
        assert_eq!(a.read(tiny, 6).unwrap(), b"parent");
        assert_eq!(a.revision(), 2);
        assert_eq!(a.savepoint_undo_bytes(), 0);
        assert_eq!(a.savepoint_depth(), 0);
    }
    #[test]
    fn aborted_full_and_tiny_allocations_never_reuse_generation_references() {
        let mut a = arena();
        let parent = a.allocate(b"parent", 16).unwrap();
        let cp = a.checkpoint().unwrap();
        let page = a.allocate(&[7; 4096], 4096).unwrap();
        let tiny = a.allocate(b"child", 16).unwrap();
        a.rollback_checkpoint(&cp).unwrap();
        assert!(a.read(page, 4096).is_err());
        assert!(a.read(tiny, 5).is_err());
        let page2 = a.allocate(&[8; 4096], 4096).unwrap();
        let tiny2 = a.allocate(b"after", 16).unwrap();
        assert_ne!(page, page2);
        assert_ne!(tiny, tiny2);
        assert!(a.read(page, 4096).is_err());
        assert!(a.read(tiny, 5).is_err());
        assert_eq!(a.read(parent, 6).unwrap(), b"parent");
    }
    #[test]
    fn undo_admission_shares_the_pool_metadata_budget_before_mutation() {
        let config = Config {
            kv_pages: 4,
            tiny_pages: 2,
            max_reserved_bytes: 1 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 4,
        };
        let probe = Pool::create(config).unwrap();
        let base = probe.stats().unwrap().metadata_bound_bytes;
        let mut a = probe.begin(probe.empty().unwrap()).unwrap();
        let workspace = probe.private_quota.used();
        let cp = a.checkpoint().unwrap();
        let journal = probe.private_quota.used() - workspace;
        a.rollback_checkpoint(&cp).unwrap();
        drop(a);
        for admitted in [false, true] {
            let p = Pool::create(Config {
                max_metadata_bytes: base + workspace + journal + PAGE_CHARGE as u64
                    - u64::from(!admitted),
                ..config
            })
            .unwrap();
            let mut a = p.begin(p.empty().unwrap()).unwrap();
            let page = a.allocate(&[1; 4096], 4096).unwrap();
            let cp = a.checkpoint().unwrap();
            let result = a.rewrite(page, &[2; 4096], 4096);
            if admitted {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result, Err(Error::LimitExceeded)));
                assert_eq!(a.read(page, 4096).unwrap(), &[1; 4096]);
                assert_eq!(a.savepoint_undo_bytes(), 0);
            }
            a.rollback_checkpoint(&cp).unwrap();
            assert_eq!(a.read(page, 4096).unwrap(), &[1; 4096]);
            a.rewrite(page, &[3; 4096], 4096).unwrap();
            drop(a);
            assert_eq!(p.private_quota.used(), 0);
        }
    }
    #[test]
    fn opaque_tokens_depth_bound_and_ancestor_rollback() {
        let mut a = arena();
        let mut other = arena();
        let foreign = other.checkpoint().unwrap();
        assert!(a.rollback_checkpoint(&foreign).is_err());
        let page = a.allocate(&[0; 4096], 4096).unwrap();
        let mut tokens = Vec::new();
        for _ in 0..64 {
            tokens.push(a.checkpoint().unwrap());
        }
        assert!(matches!(a.checkpoint(), Err(Error::LimitExceeded)));
        a.rewrite(page, &[4; 4096], 4096).unwrap();
        assert_eq!(a.savepoint_undo_bytes(), PAGE_CHARGE * 64);
        assert!(a.release_checkpoint(&tokens[0]).is_err());
        a.rollback_checkpoint(&tokens[0]).unwrap();
        assert_eq!(a.read(page, 4096).unwrap(), &[0; 4096]);
        assert!(a.release_checkpoint(&tokens[1]).is_err());
        assert_eq!(a.savepoint_depth(), 0);
        a.freeze().unwrap();
    }
}
