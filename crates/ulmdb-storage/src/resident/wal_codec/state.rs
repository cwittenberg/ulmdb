//! Logical replay state, not a graph-retirement authority. The Environment must
//! certify complete graph/source closure before issuing retirement records.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotState {
    Free,
    Live(u64),
    Retired(u64),
}

pub struct ReplayState {
    boot: Boot,
    slots: Vec<SlotState>,
    checkpointed: bool,
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
            selected: boot.genesis()?,
            commit: boot.commit,
            root: Reference::NULL,
            next_database: 1,
        })
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
        let r = &decoded.record;
        r.validate(self.boot)?;
        if r.parent != self.selected
            || r.parent_commit != self.commit
            || r.next_database < self.next_database
            || decoded.receipt.lsn != r.lsn
            || decoded.receipt.sequence != r.sequence
            || decoded.receipt.offset != self.selected.end()?
            || decoded.receipt.length != r.encoded_length(self.boot)? as u64
        {
            return Err(Error::Corrupt("resident replay parent/receipt/counters"));
        }
        decoded.receipt.end()?;
        if r.kind == Kind::Collect && (r.root != self.root || r.next_database != self.next_database)
        {
            return Err(Error::Corrupt(
                "resident collect changes logical root/counter",
            ));
        }
        for a in &r.allocations {
            let mut expected = if a.expected_previous_generation == 0 {
                SlotState::Free
            } else {
                SlotState::Retired(a.expected_previous_generation)
            };
            if self.checkpointed
                && a.key.slot >= self.boot.kv_pages
                && a.expected_previous_generation == 0
            {
                if let SlotState::Retired(g) = self.slots[a.key.slot as usize] {
                    if a.key.generation > g {
                        expected = SlotState::Retired(g);
                    }
                }
            }
            if self.slots[a.key.slot as usize] != expected {
                return Err(Error::Corrupt(
                    "resident allocate requires exact free version",
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
            let found = match r
                .allocations
                .binary_search_by_key(&key.slot, |a| a.key.slot)
            {
                Ok(n) => SlotState::Live(r.allocations[n].key.generation),
                Err(_)
                    if r.retired
                        .binary_search_by_key(&key.slot, |k| k.slot)
                        .is_ok() =>
                {
                    SlotState::Retired(key.generation)
                }
                Err(_) => self.slots[key.slot as usize],
            };
            if found != SlotState::Live(key.generation) {
                return Err(Error::Corrupt("resident catalog is not a live version"));
            }
        }
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(r.allocations.len() + r.retired.len())
            .map_err(|_| Error::LimitExceeded)?;
        for key in &r.retired {
            changes.push((key.slot as usize, SlotState::Retired(key.generation)));
        }
        for a in &r.allocations {
            changes.push((a.key.slot as usize, SlotState::Live(a.key.generation)));
        }
        Ok(Transition {
            state: self,
            changes,
            selected: decoded.receipt,
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
