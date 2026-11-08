//! Logical replay state, not a graph-retirement authority. The Environment must
//! certify complete graph/source closure before issuing retirement records.
use super::*;

pub struct ReplayState {
    boot: Boot,
    slots: Vec<SlotState>,
    checkpointed: bool,
    #[cfg(feature = "resident-metadata-reuse")]
    scratch: Vec<(usize, SlotState)>,
    selected: WalReceipt,
    commit: u128,
    root: Reference,
    next_database: u64,
}
/// Exclusive state borrow survives IO. Dropping it leaves all state unchanged;
/// activation allocates nothing and cannot reject a previously checked record.
pub struct Transition<'a> {
    state: &'a mut ReplayState,
    changes: Vec<(usize, SlotState)>,
    selected: WalReceipt,
    commit: u128,
    root: Reference,
    next_database: u64,
}
impl ReplayState {
    pub fn new(boot: Boot) -> Result<Self> {
        let count = boot.slots()?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(count)
            .map_err(|_| Error::LimitExceeded)?;
        slots.resize(count, SlotState::Free);
        Ok(Self {
            boot,
            slots,
            checkpointed: false,
            #[cfg(feature = "resident-metadata-reuse")]
            scratch: Vec::new(),
            selected: genesis(&boot)?,
            commit: boot.commit,
            root: Reference::NULL,
            next_database: 1,
        })
    }
    #[cfg(feature = "resident-metadata-reuse")]
    pub fn retained_transition_bytes(&self) -> usize {
        self.scratch.capacity() * std::mem::size_of::<(usize, SlotState)>()
    }
    pub(in crate::resident) fn from_checkpoint(
        boot: Boot,
        seed: super::super::checkpoint::Seed,
        slots: Vec<SlotState>,
    ) -> Result<Self> {
        seed.validate(boot)?;
        if slots.len() != boot.slots()? {
            return Err(Error::Corrupt("resident checkpoint slot count"));
        }
        let mut state = Self::new(boot)?;
        state.slots = slots;
        state.checkpointed = true;
        state.selected = seed.receipt();
        state.commit = boot
            .commit
            .checked_add(seed.sequence as u128)
            .ok_or(Error::LimitExceeded)?;
        state.root = seed.root;
        state.next_database = seed.next_database;
        if let Some(key) = super::super::wal_codec::catalog_key(boot, seed.root)? {
            if state.slot(key.slot) != Some(SlotState::Live(key.generation)) {
                return Err(Error::Corrupt("resident checkpoint catalog membership"));
            }
        }
        Ok(state)
    }
    pub fn boot(&self) -> Boot {
        self.boot
    }
    pub fn selected(&self) -> WalReceipt {
        self.selected
    }
    pub fn commit(&self) -> u128 {
        self.commit
    }
    pub fn root(&self) -> Reference {
        self.root
    }
    pub fn next_database(&self) -> u64 {
        self.next_database
    }
    pub fn slot(&self, slot: u32) -> Option<SlotState> {
        self.slots.get(slot as usize).copied()
    }
    pub fn slots(&self) -> &[SlotState] {
        &self.slots
    }

    pub fn prepare<'a>(&'a mut self, decoded: &Decoded) -> Result<Transition<'a>> {
        if decoded.boot != self.boot {
            return Err(Error::Corrupt("RL2 codec/state Boot mismatch"));
        }
        self.prepare_view(&decoded.record, decoded.receipt)
    }
    pub(crate) fn prepare_borrowed<'a>(
        &'a mut self,
        token: &borrowed::CheckedBorrowed<'_, '_, '_>,
    ) -> Result<Transition<'a>> {
        if token.boot != self.boot {
            return Err(Error::Corrupt("RL2 codec/state Boot mismatch"));
        }
        self.prepare_view(token.record, token.receipt)
    }
    fn prepare_view<'a>(
        &'a mut self,
        v: &impl view::RecordView,
        receipt: WalReceipt,
    ) -> Result<Transition<'a>> {
        let r = v.header();
        if r.parent != self.selected
            || r.parent_commit != self.commit
            || r.next_database < self.next_database
            || receipt.lsn != r.lsn
            || receipt.sequence != r.sequence
            || receipt.offset != self.selected.end()?
        {
            return Err(Error::Corrupt("resident replay parent/receipt/counters"));
        }
        // Decoded has private fields and immutable accessors. Its constructors
        // already checked all canonical ranges, generations and frame length
        // against exactly self.boot (checked above). Repeating that complete
        // scan here would add no stateful validation. The checks below depend
        // on this selected parent and cannot be replaced by the codec token.
        receipt.end()?;
        if r.kind == Kind::Collect && (r.root != self.root || r.next_database != self.next_database)
        {
            return Err(Error::Corrupt(
                "resident collect changes logical root/counter",
            ));
        }
        for i in 0..v.allocation_count() {
            let a = v.allocation(i);
            let mut expected = if a.previous_generation == 0 {
                SlotState::Free
            } else {
                SlotState::Retired(a.previous_generation)
            };
            if self.checkpointed
                && a.destination.slot >= self.boot.kv_pages
                && a.previous_generation == 0
            {
                if let SlotState::Retired(g) = self.slots[a.destination.slot as usize] {
                    if a.destination.generation > g {
                        expected = SlotState::Retired(g);
                    }
                }
            }
            if self.slots[a.destination.slot as usize] != expected {
                return Err(Error::Corrupt(
                    "resident allocate requires exact free version",
                ));
            }
        }
        for i in 0..v.allocation_count() {
            let a = v.allocation(i);
            if let view::BodyRef::Copy { source, .. } = a.body
                && self.slot(source.slot) != Some(SlotState::Live(source.generation))
            {
                return Err(Error::Corrupt(
                    "RL2 source requires exact parent Live version",
                ));
            }
        }
        for key in &r.retired {
            if self.slots[key.slot as usize] != SlotState::Live(key.generation) {
                return Err(Error::Corrupt("resident retirement requires live version"));
            }
        }
        // Validate the candidate catalog against the projected slot state.
        // Complete child/source membership is still the pre-IO graph callback.
        if let Some(key) = catalog_key(self.boot, r.root)? {
            let found = match view::find(v, key.slot) {
                Some(n) => SlotState::Live(v.allocation(n).destination.generation),
                None if r
                    .retired
                    .binary_search_by_key(&key.slot, |k| k.slot)
                    .is_ok() =>
                {
                    SlotState::Retired(key.generation)
                }
                None => self.slots[key.slot as usize],
            };
            if found != SlotState::Live(key.generation) {
                return Err(Error::Corrupt("resident catalog is not a live version"));
            }
        }
        #[cfg(feature = "resident-metadata-reuse")]
        if v.allocation_count() + r.retired.len() > self.slots.len() {
            return Err(Error::LimitExceeded);
        }
        #[cfg(not(feature = "resident-metadata-reuse"))]
        let mut changes = Vec::new();
        #[cfg(feature = "resident-metadata-reuse")]
        let mut changes = std::mem::take(&mut self.scratch);
        if changes
            .try_reserve_exact(v.allocation_count() + r.retired.len())
            .is_err()
        {
            #[cfg(feature = "resident-metadata-reuse")]
            {
                self.scratch = changes;
            }
            return Err(Error::LimitExceeded);
        }
        #[cfg(feature = "resident-metadata-reuse")]
        if changes.capacity() > self.slots.len() {
            // Geometry is the explicit hard cap; no retained capacity can exceed it.
            return Err(Error::LimitExceeded);
        }
        for key in &r.retired {
            changes.push((key.slot as usize, SlotState::Retired(key.generation)));
        }
        for i in 0..v.allocation_count() {
            let a = v.allocation(i);
            changes.push((
                a.destination.slot as usize,
                SlotState::Live(a.destination.generation),
            ));
        }
        Ok(Transition {
            state: self,
            changes,
            selected: receipt,
            commit: r.commit,
            root: r.root,
            next_database: r.next_database,
        })
    }

    pub fn apply(&mut self, decoded: &Decoded) -> Result<()> {
        self.prepare(decoded)?.activate();
        Ok(())
    }
}
impl Transition<'_> {
    pub fn selected(&self) -> WalReceipt {
        self.selected
    }
    pub fn commit(&self) -> u128 {
        self.commit
    }
    pub fn root(&self) -> Reference {
        self.root
    }
    pub fn next_database(&self) -> u64 {
        self.next_database
    }
    pub fn slot(&self, slot: u32) -> Option<SlotState> {
        self.changes
            .iter()
            .find(|(n, _)| *n == slot as usize)
            .map(|(_, s)| *s)
            .or_else(|| self.state.slot(slot))
    }
    #[cfg(not(feature = "resident-metadata-reuse"))]
    pub fn activate(self) {
        for (slot, next) in self.changes {
            self.state.slots[slot] = next;
        }
        self.state.selected = self.selected;
        self.state.commit = self.commit;
        self.state.root = self.root;
        self.state.next_database = self.next_database;
    }
}

#[cfg(feature = "resident-metadata-reuse")]
impl Transition<'_> {
    pub fn activate(mut self) {
        for (slot, next) in self.changes.drain(..) {
            self.state.slots[slot] = next;
        }
        self.state.selected = self.selected;
        self.state.commit = self.commit;
        self.state.root = self.root;
        self.state.next_database = self.next_database;
    }
}
#[cfg(feature = "resident-metadata-reuse")]
impl Drop for Transition<'_> {
    fn drop(&mut self) {
        // Applies both to rejected/aborted candidates and successful activation.
        // Only row capacity survives; it cannot retain object/View/Version owners.
        self.changes.clear();
        self.state.scratch = std::mem::take(&mut self.changes);
    }
}

#[cfg(all(test, feature = "resident-metadata-reuse"))]
#[path = "scratch_tests.rs"]
mod metadata_reuse_scratch_tests;
