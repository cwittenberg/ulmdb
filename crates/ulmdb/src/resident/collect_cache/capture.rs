use super::*;
/// Temporary cache-only capture, separately bounded from the full Trace.live.
/// Fixed slot generations avoid per-read tree allocations while recording.
pub(super) struct Capture {
    versions: Vec<u64>,
    empty: Vec<EmptyBinding>,
    limit: usize,
}
impl Capture {
    pub(super) fn new(slots: usize, limit: usize) -> Option<Self> {
        if slots
            .checked_mul(size_of::<u64>())?
            .checked_add(size_of::<Self>())?
            > limit
        {
            return None;
        }
        let mut versions = Vec::new();
        versions.try_reserve_exact(slots).ok()?;
        versions.resize(slots, 0);
        let capture = Self {
            versions,
            empty: Vec::new(),
            limit,
        };
        (capture.bytes()? <= limit).then_some(capture)
    }
    fn bytes(&self) -> Option<usize> {
        size_of::<Self>()
            .checked_add(self.versions.capacity().checked_mul(size_of::<u64>())?)?
            .checked_add(
                self.empty
                    .capacity()
                    .checked_mul(size_of::<EmptyBinding>())?,
            )
    }
    pub(super) fn record(&mut self, at: Reference, len: usize, key: VersionKey) -> bool {
        let Some(version) = self.versions.get_mut(key.slot as usize) else {
            return false;
        };
        if *version != 0 && *version != key.generation {
            return false;
        }
        *version = key.generation;
        if len == 0 {
            if self
                .bytes()
                .and_then(|b| b.checked_add(size_of::<EmptyBinding>()))
                .is_none_or(|b| b > self.limit)
                || self.empty.try_reserve_exact(1).is_err()
            {
                return false;
            }
            if self.bytes().is_none_or(|b| b > self.limit) {
                return false;
            }
            self.empty.push(EmptyBinding { at, version: key });
        }
        true
    }
    pub(super) fn finish(
        mut self,
        name: &[u8],
        db: &Database,
        records: u64,
        available: usize,
    ) -> Option<Arc<Entry>> {
        self.empty.sort_unstable();
        self.empty.dedup();
        let count = self.versions.iter().filter(|g| **g != 0).count();
        if Entry::charge(name.len(), count, self.empty.capacity())? > available {
            return None;
        }
        let mut keys = Vec::new();
        keys.try_reserve_exact(count).ok()?;
        for (slot, generation) in self.versions.iter().copied().enumerate() {
            if generation != 0 {
                keys.push(VersionKey {
                    slot: u32::try_from(slot).ok()?,
                    generation,
                });
            }
        }
        let mut owned_name = Vec::new();
        owned_name.try_reserve_exact(name.len()).ok()?;
        owned_name.extend_from_slice(name);
        let charged = Entry::charge(
            owned_name.capacity(),
            keys.capacity(),
            self.empty.capacity(),
        )?;
        if charged > available {
            return None;
        }
        Some(Arc::new(Entry {
            name: owned_name,
            id: db.id,
            kind: db.kind,
            root: db.root,
            keys,
            empty: self.empty,
            records,
            charged,
        }))
    }
}
