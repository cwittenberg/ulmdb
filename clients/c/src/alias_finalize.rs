//! Catalog removal and outer-only selection of independently loaded DBI roots.
//! Logical names/slots choose the winner; only genuine native capabilities can
//! remove a catalog identity or transfer its selected root into a new identity.
use super::*;
use std::collections::BTreeSet;

impl MDB_txn {
    /// Retain precisely the existing branches before unlinking this identity.
    /// A later public CREATE issues a different genuine native destination.
    pub(super) fn retire_actual_aliases(&mut self, actual: &DatabaseHandle) -> FfiResult<()> {
        let id = NativeId::of(actual);
        let views: Vec<_> = {
            let state = self.dbis.lock().map_err(|_| BAD_TXN)?;
            if state.retained.contains_key(&id) {
                return Err(BAD_DBI);
            }
            state
                .bindings
                .values()
                .filter_map(|binding| {
                    if binding.native_id() != id {
                        return None;
                    }
                    match &binding.view {
                        DataView::Writable(view) => Some(view.clone()),
                        _ => None,
                    }
                })
                .collect()
        };
        if views.is_empty() {
            self.writer()?.drop_database_handle(actual).map_err(code)?;
            if self.writer()?.open_database(compat_features::META).is_ok() {
                self.writer()?
                    .delete(compat_features::META, &id.bytes())
                    .map_err(code)?;
            }
            self.remove_database_name(actual.name())?;
        } else {
            let members: Vec<_> = views.iter().collect();
            let group = self
                .writer()?
                .retain_database_views(actual, &members)
                .map_err(code)?;
            self.dbis
                .lock()
                .map_err(|_| BAD_TXN)?
                .retained
                .insert(id, group);
        }
        self.dbis.lock().map_err(|_| BAD_TXN)?.hidden.insert(id);
        Ok(())
    }

    pub(super) fn clear_main_catalog(&mut self) -> FfiResult<()> {
        self.prune_closed_alias_bindings()?;
        let names = self.database_names()?;
        for name in names {
            if self.external_database_name(&name)?.is_none() {
                continue;
            }
            let actual = self.handle_by_name(&name)?;
            let id = NativeId::of(&actual);
            if self.shared.catalog_terminator {
                // The caller already checked every transaction-valid named
                // slot. Internal unexported catalog entries can be removed.
                self.writer()?.drop_database_handle(&actual).map_err(code)?;
            } else if matches!(
                actual.kind(),
                DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
            ) {
                self.retire_actual_aliases(&actual)?;
            } else {
                // Native prefix capabilities retain the established exact-ID
                // extension; they never obtain an LMDB writable view cap.
                let state = self.dbis.lock().map_err(|_| BAD_TXN)?.clone();
                let bound: Vec<_> = state
                    .bindings
                    .iter()
                    .filter(|(_, b)| b.native_id() == id)
                    .collect();
                if bound.iter().any(|(alias, _)| state.dirty.contains(alias)) {
                    self.dbis.lock().map_err(|_| BAD_TXN)?.hidden.insert(id);
                    continue;
                }
                if bound.iter().any(|(alias, _)| state.loaded.contains(alias)) {
                    if state.detached.len() >= WriteTxn::MAX_DETACHED_DATABASES {
                        self.capacity_failed = true;
                        return Err(-30792);
                    }
                    let detached = self
                        .writer()?
                        .detach_database_handle(&actual)
                        .map_err(code)?;
                    self.dbis
                        .lock()
                        .map_err(|_| BAD_TXN)?
                        .detached
                        .insert(id, detached);
                } else {
                    self.writer()?.drop_database_handle(&actual).map_err(code)?;
                }
            }
            // Reverse mappings remain authenticated while old loaded views
            // exist. Cleanup is deferred until the final current catalog is
            // known, so a fresh same-name identity cannot lose its own map.
        }
        Ok(())
    }

    fn named_comparison_key(&self, name: &[u8]) -> Vec<u8> {
        let mut key = name.to_vec();
        if self.shared.catalog_terminator {
            key.push(0);
        }
        key
    }

    /// Run once, after child release/reserve flush and before consuming the
    /// outer writer. Savepoints preserve all branch records without selecting.
    pub(super) fn finalize_alias_roots(&mut self) -> FfiResult<()> {
        if self.readonly || !self.parent.is_null() {
            return Ok(());
        }
        self.live()?;
        self.prune_closed_alias_bindings()?;
        let state = self.dbis.lock().map_err(|_| BAD_TXN)?.clone();
        let handles = self.shared.handles.lock().map_err(|_| BAD_TXN)?.clone();
        let mut dirty = Vec::new();
        for (slot, alias) in handles
            .into_iter()
            .enumerate()
            .filter_map(|(slot, h)| h.map(|h| (slot, h)))
        {
            if alias.is_main() || !state.dirty.contains(&alias.id()) {
                continue;
            }
            let Some(binding) = state.bindings.get(&alias.id()).cloned() else {
                return Err(BAD_DBI);
            };
            if binding.slot != slot as u32 {
                return Err(BAD_DBI);
            }
            if matches!(binding.view, DataView::Writable(_)) {
                dirty.push(ResolvedAlias { alias, binding });
            }
        }
        let main = self.main_alias()?;
        dirty.sort_by(|a, b| {
            self.key_order(
                &main,
                &self.named_comparison_key(a.name()),
                &self.named_comparison_key(b.name()),
            )
            .then(a.binding.slot.cmp(&b.binding.slot))
        });
        let mut winners: Vec<ResolvedAlias> = Vec::new();
        for alias in dirty {
            if winners.last().is_some_and(|prior| {
                self.key_order(
                    &main,
                    &self.named_comparison_key(prior.name()),
                    &self.named_comparison_key(alias.name()),
                )
                .is_eq()
            }) {
                *winners.last_mut().unwrap() = alias;
            } else {
                winners.push(alias);
            }
        }
        let mut live = Vec::new();
        let mut transfers = Vec::new();
        let mut chosen_groups = BTreeSet::new();
        let mut settings = Vec::new();
        for winner in &winners {
            let DataView::Writable(view) = &winner.binding.view else {
                unreachable!()
            };
            if let Some(group) = state.retained.get(&winner.native_id()) {
                let source = self
                    .writer()?
                    .retained_database_view(group, view)
                    .map_err(code)?;
                let mut destination = match self.visible_named(winner.name())? {
                    alias::VisibleName::Ordinary => return Err(INCOMPATIBLE),
                    alias::VisibleName::Named(actual) => Some(actual),
                    alias::VisibleName::Absent => None,
                };
                if destination
                    .as_ref()
                    .is_some_and(|d| d.kind() != winner.kind())
                {
                    let prior = destination.take().unwrap();
                    // Preserve the current catalog key spelling, while the
                    // intermediate kind obtains an explicit discard decision.
                    let name = prior.name().to_vec();
                    self.retire_actual_aliases(&prior)?;
                    self.preflight_created_identity()?;
                    self.writer()?
                        .create_database(&name, winner.kind())
                        .map_err(code)?;
                    let actual = operations::current_handle(self, &name).map_err(code)?;
                    self.created_ids.insert(NativeId::of(&actual));
                    destination = Some(actual);
                }
                let destination = if let Some(actual) = destination {
                    actual
                } else {
                    self.preflight_created_identity()?;
                    let name = self.resolve_database_name(winner.name())?;
                    self.writer()?
                        .create_database(&name, winner.kind())
                        .map_err(code)?;
                    let actual = operations::current_handle(self, &name).map_err(code)?;
                    self.created_ids.insert(NativeId::of(&actual));
                    self.record_database_name(winner.name(), actual.name())?;
                    actual
                };
                if !self.created_ids.contains(&NativeId::of(&destination)) {
                    return Err(BAD_DBI);
                }
                chosen_groups.insert(winner.native_id());
                settings.push((destination.clone(), self.settings(winner)?));
                transfers.push((source, destination));
            } else {
                self.check_native_actual(winner.actual()).map_err(code)?;
                live.push(view.clone());
                if state.settings_dirty.contains(&winner.id()) {
                    settings.push((winner.actual().clone(), self.settings(winner)?));
                }
            }
        }
        let groups = self.dbis.lock().map_err(|_| BAD_TXN)?.retained.clone();
        let discarded: Vec<_> = groups
            .iter()
            .filter_map(|(id, g)| (!chosen_groups.contains(id)).then_some(g))
            .collect();
        let live_refs: Vec<_> = live.iter().collect();
        let transfer_refs: Vec<_> = transfers
            .iter()
            .map(|(source, destination)| ulmdb::DatabaseViewTransfer {
                source,
                destination,
            })
            .collect();
        let overwritten: Vec<_> = transfers
            .iter()
            .map(|(_, destination)| destination)
            .collect();
        self.writer()?
            .finalize_database_view_groups(&live_refs, &transfer_refs, &discarded, &overwritten)
            .map_err(code)?;
        for (actual, options) in settings {
            self.persist_native_settings(&actual, options)?;
        }
        if state.settings_dirty.contains(&AliasId::MAIN) {
            let actual = operations::current_handle(self, MAIN).map_err(code)?;
            let options = state.settings.get(&AliasId::MAIN).copied().ok_or(BAD_TXN)?;
            self.persist_native_settings(&actual, options)?;
        }
        // Old physical-ID settings are never authority for a fresh identity.
        // A surviving same-name identity retains its collision-checked mapping.
        for group in groups.values() {
            let original = group.original_handle();
            if self.writer()?.open_database(compat_features::META).is_ok() {
                self.writer()?
                    .delete(compat_features::META, &NativeId::of(original).bytes())
                    .map_err(code)?;
            }
            if matches!(
                operations::current_handle(self, original.name()),
                Err(Error::MissingDatabase)
            ) {
                self.remove_database_name(original.name())?;
            }
        }
        Ok(())
    }
}
