//! Immutable per-public-alias bindings. Shared callback/native handles never
//! retarget; each writer alias receives its own authenticated native branch.
use super::*;

/// A single call's freshly checked public identity and cached immutable metadata.
/// Never retained in the transaction/cursor or carried through a user callback.
pub(super) enum OperationAlias {
    Cached(ResolvedAlias, compat_features::Settings),
    Uncached(AliasHandle, bool, Option<Arc<BoundAlias>>),
}

impl MDB_txn {
    fn immutable_operation_source(&self) -> bool {
        matches!(self.state.as_ref(), Some(State::Read(read))
            if read.external_synchronization().is_none() && read.transform_context().is_none())
    }
    /// Caller has just completed live/flush_reserves, with no intervening
    /// callback or mutation. Other sources keep their existing refresh path.
    pub(super) fn operation_alias_after_live(
        &self,
        slot: u32,
    ) -> FfiResult<Option<OperationAlias>> {
        if !self.immutable_operation_source() {
            return Ok(None);
        }
        let (alias, dormant) = {
            let handles = self.shared.handles.lock().map_err(|_| BAD_TXN)?;
            let aliases = self.shared.aliases.lock().map_err(|_| BAD_TXN)?;
            (
                handles
                    .get(slot as usize)
                    .and_then(Option::as_ref)
                    .cloned()
                    .ok_or(BAD_DBI)?,
                aliases.dormant.contains(&slot),
            )
        };
        let (binding, settings) = {
            let state = self.dbis.lock().map_err(|_| BAD_TXN)?;
            if !alias.is_main() && !dormant && state.valid.get(&slot) != Some(&alias.id()) {
                return Err(libc::EINVAL);
            }
            let binding = state.bindings.get(&alias.id()).cloned();
            let settings = binding.as_ref().map(|binding| {
                state
                    .settings
                    .get(&alias.id())
                    .copied()
                    .unwrap_or(binding.seed_settings)
            });
            (binding, settings)
        };
        if dormant && binding.is_none() {
            let actual =
                operations::current_handle(self, alias.native().name()).map_err(|_| BAD_DBI)?;
            if NativeId::of(&actual) != alias.native_id() || actual.kind() != alias.kind() {
                return Err(BAD_DBI);
            }
        }
        Ok(Some(match (binding, settings) {
            (Some(binding), Some(settings)) if matches!(binding.view, DataView::Direct) => {
                OperationAlias::Cached(ResolvedAlias { alias, binding }, settings)
            }
            (binding, _) => OperationAlias::Uncached(alias, dormant, binding),
        }))
    }
    pub(super) fn operation_handle(
        &mut self,
        slot: u32,
    ) -> FfiResult<(ResolvedAlias, Option<compat_features::Settings>)> {
        if !self.immutable_operation_source() {
            return self.load_public_alias(slot).map(|handle| (handle, None));
        }
        self.live()?;
        match self.operation_alias_after_live(slot)?.unwrap() {
            OperationAlias::Cached(handle, settings) => Ok((handle, Some(settings))),
            OperationAlias::Uncached(alias, dormant, binding) => self
                .load_public_alias_binding(slot, alias, dormant, binding)
                .map(|handle| (handle, None)),
        }
    }
    pub(super) fn peek_public_alias(&self, slot: u32) -> FfiResult<(AliasHandle, bool)> {
        self.live()?;
        let (alias, dormant) = {
            let handles = self.shared.handles.lock().map_err(|_| BAD_TXN)?;
            let aliases = self.shared.aliases.lock().map_err(|_| BAD_TXN)?;
            (
                handles
                    .get(slot as usize)
                    .and_then(Option::as_ref)
                    .cloned()
                    .ok_or(BAD_DBI)?,
                aliases.dormant.contains(&slot),
            )
        };
        let valid = self.dbis.lock().map_err(|_| BAD_TXN)?.valid.get(&slot) == Some(&alias.id());
        if !alias.is_main() && !dormant && !valid {
            return Err(libc::EINVAL);
        }
        if dormant {
            let bound = self
                .dbis
                .lock()
                .map_err(|_| BAD_TXN)?
                .bindings
                .contains_key(&alias.id());
            if !bound {
                let current =
                    operations::current_handle(self, alias.native().name()).map_err(|_| BAD_DBI)?;
                if NativeId::of(&current) != alias.native_id() || current.kind() != alias.kind() {
                    return Err(BAD_DBI);
                }
            }
        }
        Ok((alias, dormant))
    }
    pub(super) fn prune_closed_alias_bindings(&self) -> FfiResult<()> {
        let mut state = self.dbis.lock().map_err(|_| BAD_TXN)?;
        let dead: Vec<_> = state
            .bindings
            .iter()
            .filter_map(|(id, binding)| {
                (*id != AliasId::MAIN && !binding.logical_life.alive() && !state.dirty.contains(id))
                    .then_some(*id)
            })
            .collect();
        for id in dead {
            state.bindings.remove(&id);
            state.settings.remove(&id);
            state.loaded.remove(&id);
            state.valid.retain(|_, current| *current != id);
        }
        // A clean removed group has no disposition once its last caller cap
        // ends. Native admission may then prune that exact group, so do not
        // retain a stale C token. Dirty bindings are excluded above; a named
        // drop's rollback ledger also keeps its AliasLife/cap alive. Private
        // readers have their own cloned ledger and never finalize the writer.
        if !state.retained.is_empty() {
            let writable_members: std::collections::BTreeSet<_> = state
                .bindings
                .values()
                .filter_map(|binding| {
                    matches!(binding.view, DataView::Writable(_)).then_some(binding.native_id())
                })
                .collect();
            state.retained.retain(|id, _| writable_members.contains(id));
        }
        Ok(())
    }
    fn seed_settings(
        &self,
        actual: &DatabaseHandle,
        origin: ulmdb::DatabaseViewOrigin,
    ) -> FfiResult<compat_features::Settings> {
        use ulmdb::DatabaseViewOrigin::{CreatedEmpty, TransactionBase};
        match origin {
            CreatedEmpty => self
                .dbis
                .lock()
                .map_err(|_| BAD_TXN)?
                .creation_settings
                .get(&NativeId::of(actual))
                .copied()
                .ok_or(-30796),
            TransactionBase => {
                let read = match self.state.as_ref() {
                    Some(State::Write(w)) => w.transaction_start_snapshot(),
                    Some(State::Child(w)) => w.transaction_start_snapshot(),
                    Some(State::PrivateRead(r)) => r.transaction_start_snapshot(),
                    _ => return Err(BAD_TXN),
                }
                .map_err(code)?;
                let base = read.open_database(actual.name()).map_err(code)?;
                if NativeId::of(&base) != NativeId::of(actual) || base.kind() != actual.kind() {
                    return Err(BAD_DBI);
                }
                match read.get(compat_features::META, &NativeId::of(actual).bytes()) {
                    Ok(Some(bytes)) => compat_features::Settings::decode(bytes),
                    Ok(None) | Err(Error::MissingDatabase) => {
                        Ok(compat_features::Settings::for_kind(actual.kind()))
                    }
                    Err(error) => Err(code(error)),
                }
            }
        }
    }
    pub(super) fn load_public_alias(&mut self, slot: u32) -> FfiResult<ResolvedAlias> {
        let (alias, dormant) = self.peek_public_alias(slot)?;
        let existing = self
            .dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .bindings
            .get(&alias.id())
            .cloned();
        self.load_public_alias_binding(slot, alias, dormant, existing)
    }
    fn load_public_alias_binding(
        &mut self,
        slot: u32,
        alias: AliasHandle,
        dormant: bool,
        existing: Option<Arc<BoundAlias>>,
    ) -> FfiResult<ResolvedAlias> {
        if alias.is_main() {
            if let Some(binding) = existing {
                return Ok(ResolvedAlias { alias, binding });
            }
            let actual = operations::current_handle(self, MAIN).map_err(code)?;
            let seed_settings = self.native_settings(&actual)?;
            let binding = Arc::new(BoundAlias {
                slot,
                logical_life: alias.life(),
                actual,
                view: DataView::Direct,
                seed_settings,
            });
            self.dbis
                .lock()
                .map_err(|_| BAD_TXN)?
                .bindings
                .insert(AliasId::MAIN, binding.clone());
            return Ok(ResolvedAlias { binding, alias });
        }
        if let Some(binding) = existing {
            // A child inherits current parent-local widths, not base META. The
            // immutable scope loans precisely the parent's already-loaded root.
            if let (DataView::Writable(parent), Some(State::PrivateRead(read))) =
                (&binding.view, self.state.as_ref())
            {
                let view = read.read_scope().loan_database_view(parent).map_err(code)?;
                let replacement = Arc::new(BoundAlias {
                    slot,
                    logical_life: alias.life(),
                    actual: binding.actual.clone(),
                    view: DataView::Readonly(view),
                    seed_settings: binding.seed_settings,
                });
                self.dbis
                    .lock()
                    .map_err(|_| BAD_TXN)?
                    .bindings
                    .insert(alias.id(), replacement.clone());
                return Ok(ResolvedAlias {
                    alias,
                    binding: replacement,
                });
            }
            return Ok(ResolvedAlias { alias, binding });
        }
        self.prune_closed_alias_bindings()?;
        let actual = if alias.strict() {
            let actual = operations::current_handle(self, alias.native().name()).map_err(code)?;
            if NativeId::of(&actual) != alias.native_id() || actual.kind() != alias.kind() {
                return Err(BAD_DBI);
            }
            actual
        } else {
            match self.visible_named(alias.name())? {
                alias::VisibleName::Named(actual) => actual,
                alias::VisibleName::Ordinary => return Err(INCOMPATIBLE),
                alias::VisibleName::Absent => return Err(self.missing_named_code(&alias)?),
            }
        };
        if dormant && NativeId::of(&actual) != alias.native_id() {
            return Err(BAD_DBI);
        }
        let binding = self.build_alias_binding(slot, &alias, actual)?;
        {
            let mut state = self.dbis.lock().map_err(|_| BAD_TXN)?;
            state.bindings.insert(alias.id(), binding.clone());
            state.settings.insert(alias.id(), binding.seed_settings);
            state.loaded.insert(alias.id());
        }
        self.mark_database_loaded(slot, &alias)?;
        Ok(ResolvedAlias { alias, binding })
    }
    pub(super) fn build_alias_binding(
        &mut self,
        slot: u32,
        alias: &AliasHandle,
        actual: DatabaseHandle,
    ) -> FfiResult<Arc<BoundAlias>> {
        if matches!(
            alias.kind(),
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
        ) && !matches!(
            actual.kind(),
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
        ) {
            return Err(INCOMPATIBLE);
        }
        // Persistent flag rejection precedes cap allocation. Learned widths
        // below still come from the native constructor's authenticated seed.
        let observed = self.native_settings(&actual)?;
        if matches!(actual.kind(), DatabaseKind::Duplicates { .. })
            != (observed.flags & DUPSORT != 0)
        {
            return Err(-30796);
        }
        if (observed.flags ^ alias.exported_settings().flags) & compat_features::DB_FLAGS != 0 {
            return Err(INCOMPATIBLE);
        }
        let branch = !alias.strict()
            && matches!(
                actual.kind(),
                DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
            );
        let (view, seed_settings) = if branch {
            match self.state.as_ref() {
                Some(State::Read(_)) => (DataView::Direct, self.native_settings(&actual)?),
                Some(State::PrivateRead(read)) => {
                    let cap = read
                        .read_scope()
                        .open_database_view_with_ordering(&actual, &self.ordering_callbacks(alias)?)
                        .map_err(code)?;
                    let settings = self.seed_settings(&actual, cap.origin())?;
                    (DataView::Readonly(cap), settings)
                }
                Some(State::Write(_) | State::Child(_)) => {
                    let callbacks = self.ordering_callbacks(alias)?;
                    let cap = self
                        .writer()?
                        .open_database_view_with_ordering(&actual, &callbacks)
                        .map_err(code)?;
                    let settings = self.seed_settings(&actual, cap.origin())?;
                    (DataView::Writable(cap), settings)
                }
                _ => return Err(BAD_TXN),
            }
        } else {
            (DataView::Direct, self.native_settings(&actual)?)
        };
        if (seed_settings.flags ^ alias.exported_settings().flags) & compat_features::DB_FLAGS != 0
        {
            return Err(INCOMPATIBLE);
        }
        if matches!(actual.kind(), DatabaseKind::Duplicates { .. })
            != (seed_settings.flags & DUPSORT != 0)
        {
            return Err(-30796);
        }
        Ok(Arc::new(BoundAlias {
            slot,
            logical_life: alias.life(),
            actual,
            view,
            seed_settings,
        }))
    }
    pub(super) fn refresh_view_alias(&self, logical: &ResolvedAlias) -> FfiResult<()> {
        match &logical.binding.view {
            DataView::Writable(view) if view.handle() != logical.actual() => Err(BAD_DBI),
            DataView::Readonly(view) if view.handle() != logical.actual() => Err(BAD_DBI),
            _ => Ok(()),
        }
    }
}
