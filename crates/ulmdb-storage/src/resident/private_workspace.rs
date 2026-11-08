//! Writer-only private lookup. Cached buffers are always empty of live owners.
use super::store::Private;
#[cfg(feature = "resident-private-index")]
use crate::Error;
use crate::Result;
#[derive(Clone, Copy, Debug, Default)]
pub struct PrivateWorkspaceStats {
    pub enabled: bool,
    pub configured_slots: usize,
    pub index_capacity_bytes: usize,
    pub packed_capacity_bytes: usize,
    #[cfg(feature = "resident-metadata-reuse")]
    pub frozen_capacity_bytes: usize,
    pub charged_capacity_bytes: usize,
    pub aggregate_reserved_bytes: u64,
    pub aggregate_budget_bytes: u64,
    pub leased: bool,
    pub leases: u64,
}
#[cfg(not(feature = "resident-private-index"))]
pub(super) struct Pages {
    map: std::collections::BTreeMap<u32, Private>,
}
#[cfg(not(feature = "resident-private-index"))]
impl Pages {
    pub fn new(_: &super::pool::Pool) -> Result<Self> {
        Ok(Self {
            map: Default::default(),
        })
    }
    #[inline]
    pub fn get(&self, slot: &u32) -> Result<Option<&Private>> {
        Ok(self.map.get(slot))
    }
    #[inline]
    pub fn get_mut(&mut self, slot: &u32) -> Result<Option<&mut Private>> {
        Ok(self.map.get_mut(slot))
    }
    pub fn insert(&mut self, slot: u32, value: Private) -> Result<()> {
        self.map.insert(slot, value);
        Ok(())
    }
    pub fn retain(&mut self, keep: impl FnMut(&Private) -> bool) -> Result<()> {
        let mut keep = keep;
        self.map.retain(|_, p| keep(p));
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn drain_sorted(
        &mut self,
    ) -> Result<std::collections::btree_map::IntoValues<u32, Private>> {
        Ok(std::mem::take(&mut self.map).into_values())
    }
    pub fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(feature = "resident-private-index")]
mod indexed {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Buffers {
        positions: Vec<u16>,
        entries: Vec<Private>,
        #[cfg(feature = "resident-metadata-reuse")]
        sealed: Vec<super::super::store::NewVersion>,
        // Fields drop in order: free both allocations before releasing quota.
        reservation: super::super::private_quota::Reservation,
    }
    impl Buffers {
        fn allocate(count: usize, quota: &Arc<super::super::private_quota::Quota>) -> Result<Self> {
            #[cfg(feature = "resident-metadata-reuse")]
            let extra = std::mem::size_of::<super::super::store::NewVersion>();
            #[cfg(not(feature = "resident-metadata-reuse"))]
            let extra = 0;
            let minimum = count
                .checked_mul(std::mem::size_of::<Private>() + 2 + extra)
                .and_then(|n| n.checked_add(Self::headers()))
                .ok_or(Error::LimitExceeded)?;
            let reservation = quota.reserve(minimum as u64)?;
            let mut positions = Vec::new();
            let mut entries = Vec::new();
            positions
                .try_reserve_exact(count)
                .map_err(|_| Error::LimitExceeded)?;
            positions.resize(count, 0);
            entries
                .try_reserve_exact(count)
                .map_err(|_| Error::LimitExceeded)?;
            #[cfg(feature = "resident-metadata-reuse")]
            let sealed = {
                let mut v = Vec::new();
                v.try_reserve_exact(count)
                    .map_err(|_| Error::LimitExceeded)?;
                v
            };
            let mut buffers = Self {
                positions,
                entries,
                #[cfg(feature = "resident-metadata-reuse")]
                sealed,
                reservation,
            };
            buffers.reservation.reconcile(buffers.charged()? as u64)?;
            Ok(buffers)
        }
        fn headers() -> usize {
            // Explicit conservative padding, not a malloc-overhead/RSS bound.
            std::mem::size_of::<Buffers>() + std::mem::size_of::<Pages>() + 256
        }
        fn frozen_bytes(&self) -> usize {
            #[cfg(feature = "resident-metadata-reuse")]
            {
                self.sealed.capacity() * std::mem::size_of::<super::super::store::NewVersion>()
            }
            #[cfg(not(feature = "resident-metadata-reuse"))]
            {
                0
            }
        }
        fn charged(&self) -> Result<usize> {
            self.positions
                .capacity()
                .checked_mul(2)
                .and_then(|n| {
                    n.checked_add(
                        self.entries
                            .capacity()
                            .checked_mul(std::mem::size_of::<Private>())?,
                    )
                })
                .and_then(|n| n.checked_add(Self::headers()))
                .and_then(|n| n.checked_add(self.frozen_bytes()))
                .ok_or(Error::LimitExceeded)
        }
        fn position(&self, slot: u32) -> Result<Option<usize>> {
            let Some(&value) = self.positions.get(slot as usize) else {
                return Ok(None);
            };
            if value == 0 {
                return Ok(None);
            }
            let pos = usize::from(value - 1);
            if self.entries.get(pos).is_none_or(|p| p.slot != slot) {
                return Err(Error::Corrupt("resident private index position/slot"));
            }
            Ok(Some(pos))
        }
        fn clear(&mut self) -> Result<()> {
            let mut bad = false;
            for (pos, entry) in self.entries.iter().enumerate() {
                if let Some(value) = self.positions.get_mut(entry.slot as usize) {
                    bad |= usize::from(*value) != pos + 1;
                    *value = 0;
                } else {
                    bad = true;
                }
            }
            // Drop every Permit BEFORE taking the cache mutex: Permit::drop can
            // acquire the Pool mutex. No live owner may enter the cached state.
            self.entries.clear();
            #[cfg(feature = "resident-metadata-reuse")]
            self.sealed.clear();
            if bad {
                Err(Error::Corrupt("resident private index return"))
            } else {
                Ok(())
            }
        }
    }
    struct CacheState {
        buffers: Option<Buffers>,
        epoch: u64,
        leased: bool,
        poisoned: bool,
    }
    pub struct PrivateCache {
        geometry: (usize, usize),
        quota: Arc<super::super::private_quota::Quota>,
        stats: PrivateWorkspaceStats,
        state: Mutex<CacheState>,
        // Drop cached buffers/state before releasing the cache-header charge.
        _header: super::super::private_quota::Reservation,
    }
    impl PrivateCache {
        pub fn create(pool: &super::super::pool::Pool) -> Result<Arc<Self>> {
            let geometry = pool.geometry();
            let count = geometry
                .0
                .checked_add(geometry.1)
                .filter(|n| geometry.0 > 0 && *n <= super::super::pool::MAX_SLOTS)
                .ok_or(Error::LimitExceeded)?;
            let header = pool
                .private_quota
                .reserve((std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>()) as u64)?;
            let buffers = Buffers::allocate(count, &pool.private_quota)?;
            let stats = PrivateWorkspaceStats {
                enabled: true,
                configured_slots: count,
                index_capacity_bytes: buffers.positions.capacity() * 2,
                packed_capacity_bytes: buffers.entries.capacity() * std::mem::size_of::<Private>(),
                #[cfg(feature = "resident-metadata-reuse")]
                frozen_capacity_bytes: buffers.frozen_bytes(),
                charged_capacity_bytes: buffers
                    .charged()?
                    .checked_add(header.bytes() as usize)
                    .ok_or(Error::LimitExceeded)?,
                aggregate_reserved_bytes: 0,
                aggregate_budget_bytes: pool.private_quota.limit(),
                leased: false,
                leases: 0,
            };
            Ok(Arc::new(Self {
                geometry,
                quota: Arc::clone(&pool.private_quota),
                stats,
                state: Mutex::new(CacheState {
                    buffers: Some(buffers),
                    epoch: 0,
                    leased: false,
                    poisoned: false,
                }),
                _header: header,
            }))
        }
        pub fn stats(&self) -> Result<PrivateWorkspaceStats> {
            let state = self.state.lock().map_err(|_| Error::Busy)?;
            Ok(PrivateWorkspaceStats {
                leased: state.leased,
                aggregate_reserved_bytes: self.quota.used(),
                leases: state.epoch,
                ..self.stats
            })
        }
        pub(in crate::resident) fn take(
            self: &Arc<Self>,
            pool: &super::super::pool::Pool,
        ) -> Result<Pages> {
            if pool.geometry() != self.geometry || !Arc::ptr_eq(&pool.private_quota, &self.quota) {
                return Err(Error::InvalidInput("resident private cache geometry"));
            }
            let mut state = self.state.lock().map_err(|_| Error::Busy)?;
            if state.poisoned {
                return Err(Error::Corrupt("resident private cache poisoned"));
            }
            if state.leased {
                return Err(Error::Busy);
            }
            let epoch = state.epoch.checked_add(1).ok_or(Error::LimitExceeded)?;
            let buffers = state
                .buffers
                .take()
                .ok_or(Error::Corrupt("resident private cache empty"))?;
            if !buffers.entries.is_empty() || buffers.positions.len() != self.stats.configured_slots
            {
                state.poisoned = true;
                drop(state);
                drop(buffers);
                return Err(Error::Corrupt("resident private cache nonempty/geometry"));
            }
            state.epoch = epoch;
            state.leased = true;
            Ok(Pages {
                buffers: Some(buffers),
                home: Some(Arc::clone(self)),
                epoch,
            })
        }
    }
    pub(in crate::resident) struct Pages {
        buffers: Option<Buffers>,
        home: Option<Arc<PrivateCache>>,
        epoch: u64,
    }
    impl Pages {
        pub fn new(pool: &super::super::pool::Pool) -> Result<Self> {
            let geometry = pool.geometry();
            let count = geometry
                .0
                .checked_add(geometry.1)
                .filter(|n| geometry.0 > 0 && *n <= super::super::pool::MAX_SLOTS)
                .ok_or(Error::LimitExceeded)?;
            Ok(Self {
                buffers: Some(Buffers::allocate(count, &pool.private_quota)?),
                home: None,
                epoch: 0,
            })
        }
        #[inline]
        pub fn get(&self, slot: &u32) -> Result<Option<&Private>> {
            let b = self
                .buffers
                .as_ref()
                .ok_or(Error::Corrupt("resident closed private workspace"))?;
            Ok(b.position(*slot)?.map(|p| &b.entries[p]))
        }
        #[inline]
        pub fn get_mut(&mut self, slot: &u32) -> Result<Option<&mut Private>> {
            let b = self
                .buffers
                .as_mut()
                .ok_or(Error::Corrupt("resident closed private workspace"))?;
            let p = b.position(*slot)?;
            Ok(p.map(|p| &mut b.entries[p]))
        }
        pub fn retain(&mut self, mut keep: impl FnMut(&Private) -> bool) -> Result<()> {
            let b = self
                .buffers
                .as_mut()
                .ok_or(Error::Corrupt("resident closed private workspace"))?;
            for (pos, p) in b.entries.iter().enumerate() {
                if b.positions.get(p.slot as usize).copied().map(usize::from) != Some(pos + 1) {
                    return Err(Error::Corrupt("resident private retain position"));
                }
            }
            for p in &b.entries {
                b.positions[p.slot as usize] = 0;
            }
            b.entries.retain(|p| keep(p));
            for (pos, p) in b.entries.iter().enumerate() {
                b.positions[p.slot as usize] = (pos + 1) as u16;
            }
            Ok(())
        }
        pub fn insert(&mut self, slot: u32, value: Private) -> Result<()> {
            let b = self
                .buffers
                .as_mut()
                .ok_or(Error::Corrupt("resident closed private workspace"))?;
            if slot != value.slot
                || slot as usize >= b.positions.len()
                || b.positions[slot as usize] != 0
                || b.entries.len() == b.positions.len()
                || b.entries.len() == b.entries.capacity()
            {
                return Err(Error::Corrupt("resident private insert slot/capacity"));
            }
            // Capacity was admitted before publication; both updates below are
            // infallible and there is no fallible gap after the index is written.
            let position = u16::try_from(b.entries.len() + 1).map_err(|_| Error::LimitExceeded)?;
            b.entries.push(value);
            b.positions[slot as usize] = position;
            Ok(())
        }
        #[cfg(feature = "resident-metadata-reuse")]
        pub fn seal_private(&mut self) -> Result<()> {
            let b = self
                .buffers
                .as_mut()
                .ok_or(Error::Corrupt("resident closed private workspace"))?;
            if !b.sealed.is_empty() || b.sealed.capacity() < b.entries.len() {
                return Err(Error::Corrupt("resident frozen scratch capacity"));
            }
            for (pos, p) in b.entries.iter().enumerate() {
                if b.positions.get(p.slot as usize).copied().map(usize::from) != Some(pos + 1) {
                    return Err(Error::Corrupt("resident private drain position"));
                }
            }
            for p in &b.entries {
                b.positions[p.slot as usize] = 0;
            }
            b.entries.sort_unstable_by_key(|p| p.slot);
            for p in b.entries.drain(..) {
                if let Some(v) = super::super::store::NewVersion::seal(p) {
                    b.sealed.push(v);
                }
            }
            Ok(())
        }
        #[cfg(feature = "resident-metadata-reuse")]
        pub fn sealed(&self) -> &[super::super::store::NewVersion] {
            &self.buffers.as_ref().expect("live frozen workspace").sealed
        }
        #[cfg(feature = "resident-metadata-reuse")]
        pub fn take_sealed(&mut self) -> Vec<super::super::store::NewVersion> {
            std::mem::take(&mut self.buffers.as_mut().expect("live frozen workspace").sealed)
        }
        #[cfg(feature = "resident-metadata-reuse")]
        pub fn restore_sealed(&mut self, sealed: Vec<super::super::store::NewVersion>) {
            let b = self.buffers.as_mut().expect("live frozen workspace");
            assert!(b.sealed.is_empty() && b.entries.is_empty() && sealed.is_empty());
            b.sealed = sealed;
        }
        pub fn len(&self) -> usize {
            self.buffers.as_ref().map_or(0, |b| b.entries.len())
        }
        #[cfg(any(not(feature = "resident-metadata-reuse"), test))]
        pub fn drain_sorted(&mut self) -> Result<std::vec::Drain<'_, Private>> {
            let b = self
                .buffers
                .as_mut()
                .ok_or(Error::Corrupt("resident closed private workspace"))?;
            for (pos, entry) in b.entries.iter().enumerate() {
                let cell = b
                    .positions
                    .get_mut(entry.slot as usize)
                    .ok_or(Error::Corrupt("resident private drain slot"))?;
                if usize::from(*cell) != pos + 1 {
                    return Err(Error::Corrupt("resident private drain position"));
                }
            }
            for entry in &b.entries {
                b.positions[entry.slot as usize] = 0;
            }
            b.entries.sort_unstable_by_key(|p| p.slot);
            Ok(b.entries.drain(..))
        }
        pub fn finish(&mut self) -> Result<()> {
            let Some(mut buffers) = self.buffers.take() else {
                return Ok(());
            };
            let cleared = buffers.clear();
            if let Some(home) = self.home.take() {
                let mut state = home.state.lock().map_err(|_| Error::Busy)?;
                if cleared.is_err()
                    || !state.leased
                    || state.epoch != self.epoch
                    || state.buffers.is_some()
                {
                    state.poisoned = true;
                    return Err(Error::Corrupt("resident private lease return"));
                }
                state.buffers = Some(buffers);
                state.leased = false;
            }
            cleared
        }
    }
    impl Drop for Pages {
        fn drop(&mut self) {
            let _ = self.finish();
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        include!("private_workspace_tests.rs");
    }
}
#[cfg(feature = "resident-private-index")]
pub(super) use indexed::Pages;
#[cfg(feature = "resident-private-index")]
pub use indexed::PrivateCache;
