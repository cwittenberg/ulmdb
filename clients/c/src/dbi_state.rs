//! Transaction-local LMDB DBI loading, distinct from environment-wide aliases.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Slot identity is distinct from native database identity. A closed slot is
/// quarantined until the outer transaction resolves, even if another alias is
/// later opened for the same native database.
#[derive(Default)]
pub(super) struct AliasStates {
    // One candidate per occupied public slot; empty name buckets are removed.
    // These are lookup hints only. Slot/generation and Native checks still run.
    pub(super) names: BTreeMap<Box<[u8]>, BTreeMap<u32, AliasId>>,
    pub(super) free_slots: BTreeSet<u32>,
    pub(super) active_count: usize,
    pub dormant: BTreeSet<u32>,
    pub reserved: BTreeMap<u32, AliasReservation>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AliasReservation {
    pub alias: AliasId,
    pub native: NativeId,
}
pub(super) const MAX_ALIAS_SLOTS: usize = 8194;
// One native catalog plus all quarantined identities, with the two core slots
// conservatively included. This is provenance by ID, never slot-history pairs.
pub(super) const MAX_CREATED_IDENTITIES: usize = 4096 + MAX_ALIAS_SLOTS;
pub(super) const INTERNAL_DATABASE_BUDGET: u32 = 8;
pub(super) const MAX_USER_DATABASES: u32 =
    ulmdb::CatalogCapacity::EXTENDED.get() as u32 - INTERNAL_DATABASE_BUDGET;
pub(super) fn catalog_profile(shared: &Shared) -> ulmdb::CatalogCapacity {
    if shared.maxdbs.saturating_add(INTERNAL_DATABASE_BUDGET)
        > ulmdb::CatalogCapacity::DEFAULT.get() as u32
    {
        ulmdb::CatalogCapacity::EXTENDED
    } else {
        ulmdb::CatalogCapacity::DEFAULT
    }
}
pub(super) fn alias_slot_limit(shared: &Shared) -> usize {
    if shared.maxdbs > 4095 {
        ulmdb::CatalogCapacity::EXTENDED.view_limit()
    } else {
        MAX_ALIAS_SLOTS
    }
}
pub(super) fn created_identity_limit(shared: &Shared) -> usize {
    catalog_profile(shared).get() + alias_slot_limit(shared)
}

pub(super) fn active_identity(
    handles: &[Option<AliasHandle>],
    aliases: &AliasStates,
    id: AliasId,
) -> bool {
    handles.iter().enumerate().any(|(slot, handle)| {
        handle.as_ref().is_some_and(|handle| handle.id() == id)
            && !aliases.dormant.contains(&(slot as u32))
            && !aliases.reserved.contains_key(&(slot as u32))
    })
}

pub(super) fn push_identity(identities: &mut Vec<(u32, AliasId)>, slot: u32, id: AliasId) {
    if !identities.contains(&(slot, id)) {
        identities.push((slot, id));
    }
}

pub(super) fn set_slot_identity(identities: &mut Vec<(u32, AliasId)>, slot: u32, id: AliasId) {
    if let Some(previous) = identities
        .iter_mut()
        .find(|(previous, _)| *previous == slot)
    {
        previous.1 = id;
    } else {
        identities.push((slot, id));
    }
}

#[derive(Clone, Default)]
pub(super) struct TxnDbis {
    pub publication_dirty: bool,
    pub bindings: BTreeMap<AliasId, Arc<BoundAlias>>,
    pub valid: BTreeMap<u32, AliasId>,
    pub loaded: BTreeSet<AliasId>,
    pub dirty: BTreeSet<AliasId>,
    pub detached: BTreeMap<NativeId, ulmdb::DetachedDatabase>,
    pub hidden: BTreeSet<NativeId>,
    pub retained: BTreeMap<NativeId, ulmdb::RetainedViewGroup>,
    pub settings: BTreeMap<AliasId, compat_features::Settings>,
    pub settings_dirty: BTreeSet<AliasId>,
    // Immutable creation-time options, independent of private width learning.
    pub creation_settings: BTreeMap<NativeId, compat_features::Settings>,
}

pub(super) struct ClosedDbi {
    pub dbi: u32,
    pub handle: AliasHandle,
    pub options: Option<compat_features::DbOptions>,
}

impl TxnDbis {
    pub(super) fn from_shared(shared: &Shared) -> FfiResult<Self> {
        let handles = shared.handles.lock().map_err(|_| BAD_TXN)?;
        let aliases = shared.aliases.lock().map_err(|_| BAD_TXN)?;
        Ok(Self {
            valid: handles
                .iter()
                .enumerate()
                .filter_map(|(slot, handle)| {
                    handle
                        .as_ref()
                        .filter(|handle| {
                            handle.name() != MAIN
                                && !aliases.dormant.contains(&(slot as u32))
                                && !aliases.reserved.contains_key(&(slot as u32))
                        })
                        .map(|handle| (slot as u32, handle.id()))
                })
                .collect(),
            ..Default::default()
        })
    }
}

impl MDB_txn {
    pub(super) fn preflight_activation(&self, slot: u32) -> FfiResult<()> {
        if slot as usize >= alias_slot_limit(&self.shared)
            || (!self.activated_dbis.contains_key(&slot)
                && self.activated_dbis.len() >= alias_slot_limit(&self.shared))
        {
            return Err(-30788);
        }
        Ok(())
    }
    pub(super) fn preflight_created_identity(&self) -> FfiResult<()> {
        let mut count = 0usize;
        let mut current = self;
        loop {
            count = count.checked_add(current.created_ids.len()).ok_or(-30788)?;
            if count >= created_identity_limit(&self.shared) {
                return Err(-30788);
            }
            if current.parent.is_null() {
                return Ok(());
            }
            current = unsafe { get_ref(current.parent)? };
        }
    }
    pub(super) fn detached_database(
        &self,
        id: AliasId,
    ) -> FfiResult<Option<ulmdb::DetachedDatabase>> {
        let state = self.dbis.lock().map_err(|_| BAD_TXN)?;
        if !state.loaded.contains(&id) {
            return Ok(None);
        }
        Ok(state
            .bindings
            .get(&id)
            .and_then(|binding| state.detached.get(&binding.native_id()))
            .cloned())
    }
    pub(super) fn mark_database_loaded(&self, slot: u32, handle: &AliasHandle) -> FfiResult<()> {
        if handle.is_main() {
            return Ok(());
        }
        let mut state = self.dbis.lock().map_err(|_| BAD_TXN)?;
        state.valid.insert(slot, handle.id());
        state.loaded.insert(handle.id());
        Ok(())
    }
    pub(super) fn publication_dirty(&self) -> FfiResult<bool> {
        Ok(self.dbis.lock().map_err(|_| BAD_TXN)?.publication_dirty)
    }
    pub(super) fn mark_publication_dirty(&self) -> FfiResult<()> {
        self.dbis.lock().map_err(|_| BAD_TXN)?.publication_dirty = true;
        Ok(())
    }
    pub(super) fn mark_database_dirty(&self, handle: &impl LogicalAlias) -> FfiResult<()> {
        let alias = handle.logical();
        let mut state = self.dbis.lock().map_err(|_| BAD_TXN)?;
        state.publication_dirty = true;
        if !alias.is_main() {
            state.loaded.insert(alias.id());
        }
        state.dirty.insert(alias.id());
        Ok(())
    }
    pub(super) fn valid_database(
        &self,
        slot: u32,
        handle: &AliasHandle,
        loaded: bool,
    ) -> FfiResult<()> {
        if !handle.is_main() {
            let mut state = self.dbis.lock().map_err(|_| BAD_TXN)?;
            state.valid.insert(slot, handle.id());
            if loaded {
                state.loaded.insert(handle.id());
            }
        }
        Ok(())
    }
    pub(super) fn catalog_hidden(&self, id: NativeId) -> FfiResult<bool> {
        Ok(self.dbis.lock().map_err(|_| BAD_TXN)?.hidden.contains(&id))
    }
    pub(super) fn writable_database(&mut self, handle: &ResolvedAlias) -> FfiResult<()> {
        self.live()?;
        if self.readonly {
            return Err(self.shared.readonly_error());
        }
        let retained = {
            self.dbis
                .lock()
                .map_err(|_| BAD_TXN)?
                .retained
                .get(&handle.native_id())
                .cloned()
        };
        if !handle.is_main() {
            let requires_main_touch = {
                let state = self.dbis.lock().map_err(|_| BAD_TXN)?;
                (state.hidden.contains(&handle.native_id()) || retained.is_some())
                    && !state.dirty.contains(&handle.id())
            };
            // LMDB cursor_touch tests this alias's DIRTY bit. A retained dirty
            // sibling does not grant writes; an empty MAIN fails the clean touch.
            if requires_main_touch && self.visible_main_empty()? {
                return Err(NOTFOUND);
            }
        }
        if let Some(group) = retained {
            if group.original_handle() != handle.actual() {
                return Err(BAD_DBI);
            }
            // The actual native mutation uses a separate retained capability;
            // ordinary NativeHandle and DatabaseView mutation remain strict.
            Ok(())
        } else {
            self.check_native_actual(handle.actual()).map_err(code)
        }
    }
    pub(super) fn check_native_actual(&self, actual: &DatabaseHandle) -> ulmdb::Result<()> {
        let current = operations::current_handle(self, actual.name())?;
        if current != *actual {
            return Err(Error::StaleHandle);
        }
        Ok(())
    }
    pub(super) fn seek_database_pair(
        &self,
        handle: &ResolvedAlias,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> ulmdb::Result<Option<(&[u8], &[u8])>> {
        let callbacks = self
            .ordering_callbacks(handle)
            .map_err(|_| Error::InvalidInput("callback authority"))?;
        match (&handle.binding.view, self.state.as_ref()) {
            (DataView::Writable(view), Some(State::Write(writer))) => {
                writer.view_seek_pair_with_ordering(view, bound, inclusive, reverse, &callbacks)
            }
            (DataView::Writable(view), Some(State::Child(writer))) => {
                writer.view_seek_pair_with_ordering(view, bound, inclusive, reverse, &callbacks)
            }
            (DataView::Readonly(view), Some(State::PrivateRead(read))) => read
                .read_scope()
                .seek_pair_with_ordering(view, bound, inclusive, reverse, &callbacks),
            (DataView::Direct, _) => {
                if let Some(detached) = self
                    .detached_database(handle.id())
                    .map_err(|_| Error::InvalidInput("invalid alias state"))?
                {
                    return match self.state.as_ref() {
                        Some(State::Write(w)) => w.detached_seek_pair_with_ordering(
                            &detached, bound, inclusive, reverse, &callbacks,
                        ),
                        Some(State::Child(w)) => w.detached_seek_pair_with_ordering(
                            &detached, bound, inclusive, reverse, &callbacks,
                        ),
                        _ => Err(Error::StaleHandle),
                    };
                }
                self.check_native_actual(handle.actual())?;
                match self.state.as_ref() {
                    Some(State::Read(r)) => r.client_seek_pair_with_ordering(
                        handle.actual().name(),
                        bound,
                        inclusive,
                        reverse,
                        &callbacks,
                    ),
                    Some(State::PrivateRead(r)) => r.client_seek_pair_with_ordering(
                        handle.actual().name(),
                        bound,
                        inclusive,
                        reverse,
                        &callbacks,
                    ),
                    Some(State::Write(w)) => w.client_seek_pair_with_ordering(
                        handle.actual().name(),
                        bound,
                        inclusive,
                        reverse,
                        &callbacks,
                    ),
                    Some(State::Child(w)) => w.client_seek_pair_with_ordering(
                        handle.actual().name(),
                        bound,
                        inclusive,
                        reverse,
                        &callbacks,
                    ),
                    _ => Err(Error::InvalidInput("inactive transaction")),
                }
            }
            _ => Err(Error::InvalidInput("alias view source mismatch")),
        }
    }
}
