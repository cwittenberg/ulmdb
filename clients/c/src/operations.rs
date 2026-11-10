use super::*;
pub(super) fn current_handle(t: &MDB_txn, name: &[u8]) -> ulmdb::Result<DatabaseHandle> {
    match t.state.as_ref() {
        Some(State::Read(r)) => r.open_database(name),
        Some(State::PrivateRead(r)) => r.open_database(name),
        Some(State::Write(w)) => w.open_database(name),
        Some(State::Child(w)) => w.open_database(name),
        _ => Err(Error::MissingDatabase),
    }
}
fn capacity(t: &MDB_txn, name: &[u8]) -> FfiResult<()> {
    if name == MAIN {
        return Ok(());
    }
    // Keep the existing handles -> aliases serialization with close/restore;
    // only the full-vector counting work is replaced by the bounded ledger.
    let _handles = t.shared.handles.lock().map_err(|_| BAD_TXN)?;
    let active = t.shared.aliases.lock().map_err(|_| BAD_TXN)?.active_count;
    if active >= t.shared.maxdbs as usize {
        Err(-30791)
    } else {
        Ok(())
    }
}
fn reusable_alias_slot(
    handles: &[Option<AliasHandle>],
    aliases: &dbi_state::AliasStates,
) -> Option<usize> {
    let slot = aliases.free_slot();
    debug_assert!(
        slot.is_none_or(|slot| handles.get(slot).is_some_and(Option::is_none)
            && !aliases.reserved.contains_key(&(slot as u32)))
    );
    slot
}
fn alias_slot_preflight(t: &MDB_txn, name: &[u8]) -> FfiResult<usize> {
    if name == MAIN {
        return Ok(1);
    }
    let handles = t.shared.handles.lock().map_err(|_| BAD_TXN)?;
    let aliases = t.shared.aliases.lock().map_err(|_| BAD_TXN)?;
    let slot = reusable_alias_slot(&handles, &aliases).unwrap_or(handles.len());
    if slot >= dbi_state::alias_slot_limit(&t.shared) {
        return Err(-30791);
    }
    t.preflight_activation(slot as u32)?;
    Ok(slot)
}
fn open_db(
    t: &mut MDB_txn,
    external: &[u8],
    requested: Option<DatabaseKind>,
    flags: u32,
    strict_kind: bool,
) -> FfiResult<u32> {
    t.flush_reserves()?;
    t.live()?;
    let physical = t.resolve_database_name(external)?;
    if external != MAIN {
        let existing = {
            let handles = t.shared.handles.lock().map_err(|_| BAD_TXN)?;
            let aliases = t.shared.aliases.lock().map_err(|_| BAD_TXN)?;
            let candidate = |slot: u32, id: AliasId| {
                handles
                    .get(slot as usize)
                    .and_then(Option::as_ref)
                    .filter(|h| {
                        h.id() == id
                            && h.name() == external
                            && !aliases.reserved.contains_key(&slot)
                    })
            };
            aliases.candidates(external).and_then(|slots| {
                slots
                    .iter()
                    .find_map(|(&slot, &id)| {
                        candidate(slot, id)
                            .filter(|_| !aliases.dormant.contains(&slot))
                            .map(|h| (slot, h.clone(), false))
                    })
                    .or_else(|| {
                        slots.iter().find_map(|(&slot, &id)| {
                            candidate(slot, id)
                                .filter(|h| {
                                    aliases.dormant.contains(&slot)
                                        && current_handle(t, h.native().name()).is_ok_and(
                                            |actual| {
                                                NativeId::of(&actual) == h.native_id()
                                                    && actual.kind() == h.kind()
                                            },
                                        )
                                })
                                .map(|h| (slot, h.clone(), true))
                        })
                    })
            })
        };
        if let Some((slot, alias, dormant)) = existing {
            if !dormant && t.dbis.lock().map_err(|_| BAD_TXN)?.valid.get(&slot) != Some(&alias.id())
            {
                return Ok(slot);
            }
            if strict_kind && requested.is_some_and(|kind| kind != alias.kind()) {
                return Err(INCOMPATIBLE);
            }
            if dormant {
                capacity(t, external)?;
                t.preflight_activation(slot)?;
                let mut aliases = t.shared.aliases.lock().map_err(|_| BAD_TXN)?;
                t.shared
                    .options
                    .lock()
                    .map_err(|_| BAD_TXN)?
                    .remove(&alias.id());
                aliases.set_dormant(slot, Some(&alias), false);
                t.activated_dbis.insert(slot, alias.id());
            }
            t.valid_database(slot, &alias, dormant)?;
            if dormant {
                t.load_public_alias(slot)?;
            } else {
                t.settings(&alias)?;
            }
            return Ok(slot);
        }
    }
    // Capacity, scalar issuance and native cap admission precede native create,
    // reverse mapping, META or globally visible public alias registration.
    capacity(t, external)?;
    t.prune_closed_alias_bindings()?;
    let proposed = alias_slot_preflight(t, external)?;
    let identity = if external == MAIN {
        AliasId::MAIN
    } else {
        t.shared.alias_ids.preflight()?;
        t.shared.alias_ids.issue()?
    };
    let found = if external == MAIN || strict_kind {
        match current_handle(t, &physical) {
            Ok(h) => Some(h),
            Err(Error::MissingDatabase) => None,
            Err(e) => return Err(code(e)),
        }
    } else {
        match t.visible_named(external)? {
            alias::VisibleName::Named(h) => Some(h),
            alias::VisibleName::Ordinary => return Err(INCOMPATIBLE),
            alias::VisibleName::Absent => None,
        }
    };
    let mut created = false;
    let mut created_quota = None;
    let actual = if let Some(actual) = found {
        if requested.is_some_and(|kind| kind != actual.kind()) && (external == MAIN || strict_kind)
        {
            if external == MAIN
                && !t.readonly
                && actual.kind() == DatabaseKind::Kv
                && t.visible_main_empty()?
            {
                t.finish_reserves()?;
                t.preflight_created_identity()?;
                t.writer()?.drop_database_handle(&actual).map_err(code)?;
                t.writer()?
                    .create_database(MAIN, requested.unwrap())
                    .map_err(code)?;
                created = true;
                current_handle(t, MAIN).map_err(code)?
            } else {
                return Err(INCOMPATIBLE);
            }
        } else {
            actual
        }
    } else {
        if flags & CREATE == 0 {
            return Err(NOTFOUND);
        }
        t.preflight_created_identity()?;
        if external != MAIN
            && matches!(
                requested.unwrap_or(DatabaseKind::Kv),
                DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
            )
        {
            t.writer()?.preflight_database_view().map_err(code)?;
        }
        t.finish_reserves()?;
        // Keep readonly/parent/resource preflights ahead of map admission.
        t.writer()?;
        created_quota = Some(t.quota_project(physical.len() + 48, 0)?);
        t.writer()?
            .create_database(&physical, requested.unwrap_or(DatabaseKind::Kv))
            .map_err(code)?;
        created = true;
        t.changed();
        current_handle(t, &physical).map_err(code)?
    };
    let mut settings = if external == MAIN {
        // An explicit public MAIN open resets comparators, not the current
        // transaction's deferred flag/width record. Internal bootstrap also
        // keeps this stable logical authority across physical ID issuance.
        let local = t
            .dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .settings
            .get(&AliasId::MAIN)
            .copied();
        local.unwrap_or(t.native_settings(&actual)?)
    } else {
        t.native_settings(&actual)?
    };
    if matches!(actual.kind(), DatabaseKind::Duplicates { .. }) {
        settings.flags |= DUPSORT;
    }
    if created && external != MAIN && !strict_kind {
        settings.flags = flags & compat_features::DB_FLAGS;
    }
    if created {
        t.created_ids.insert(NativeId::of(&actual));
        t.dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .creation_settings
            .insert(NativeId::of(&actual), settings);
        if let Err(error) = t.record_database_name(external, actual.name()) {
            if t.writer()?.drop_database_handle(&actual).is_err() {
                t.capacity_failed = true;
            }
            return Err(error);
        }
    }
    let native_identity = matches!(actual.kind(), DatabaseKind::Ipv4 | DatabaseKind::Ipv6);
    let alias = AliasHandle::new(
        identity,
        external,
        actual.clone(),
        settings,
        native_identity,
    );
    let binding = if external == MAIN {
        Arc::new(BoundAlias {
            slot: 1,
            logical_life: alias.life(),
            actual,
            view: DataView::Direct,
            seed_settings: settings,
        })
    } else {
        match t.build_alias_binding(proposed as u32, &alias, actual) {
            Ok(binding) => binding,
            Err(error) => {
                if created && t.writer()?.drop_database_handle(alias.native()).is_err() {
                    t.capacity_failed = true;
                }
                return Err(error);
            }
        }
    };
    let private_readers = match t.state.as_ref() {
        Some(State::PrivateRead(view)) => Some(view.readers().clone()),
        _ => None,
    };
    let mut private_slots = private_readers
        .as_ref()
        .map(|r| r.opened_slots())
        .transpose()?;
    let mut handles = t.shared.handles.lock().map_err(|_| BAD_TXN)?;
    let mut aliases = t.shared.aliases.lock().map_err(|_| BAD_TXN)?;
    let mut options = t.shared.options.lock().map_err(|_| BAD_TXN)?;
    let index = if external == MAIN {
        1
    } else if let Some(slot) = reusable_alias_slot(&handles, &aliases) {
        slot
    } else {
        if handles.len() >= dbi_state::alias_slot_limit(&t.shared) {
            if created {
                t.capacity_failed = true;
            }
            return Err(-30791);
        }
        handles.push(None);
        handles.len() - 1
    };
    if let Some(prior) =
        aliases.replace_handle(index as u32, &mut handles[index], Some(alias.clone()))
    {
        if prior.id() != alias.id() {
            options.remove(&prior.id());
        }
    }
    aliases.set_dormant(index as u32, handles[index].as_ref(), false);
    if !alias.is_main() {
        options.entry(alias.id()).or_default().flags = Some(settings.flags);
    }
    drop(options);
    drop(aliases);
    drop(handles);
    let binding = if binding.slot != index as u32 {
        Arc::new(BoundAlias {
            slot: index as u32,
            logical_life: alias.life(),
            actual: binding.actual.clone(),
            view: binding.view.clone(),
            seed_settings: binding.seed_settings,
        })
    } else {
        binding
    };
    {
        let mut state = t.dbis.lock().map_err(|_| BAD_TXN)?;
        state.bindings.insert(alias.id(), binding);
        state.settings.insert(alias.id(), settings);
        if created && !strict_kind {
            state.settings_dirty.insert(alias.id());
        }
    }
    t.valid_database(index as u32, &alias, external != MAIN)?;
    if created {
        t.mark_database_dirty(&alias)?;
    }
    if external != MAIN {
        t.activated_dbis.insert(index as u32, alias.id());
    }
    if let Some(slots) = private_slots.as_mut() {
        dbi_state::set_slot_identity(slots, index as u32, alias.id());
    }
    if let Some(used) = created_quota {
        t.quota_used = Some(used);
    }
    Ok(index as u32)
}
pub(super) fn record_created_slot(
    t: &mut MDB_txn,
    _slot: u32,
    _alias: AliasId,
    native: NativeId,
    newly_created: bool,
) {
    if newly_created {
        t.created_ids.insert(native);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_dbi_open(
    txn: *mut MDB_txn,
    nameptr: *const c_char,
    flags: u32,
    out: *mut u32,
) -> c_int {
    boundary(|| {
        if flags & !(CREATE | compat_features::DB_FLAGS) != 0 {
            return Err(libc::ENOTSUP);
        }
        if flags & (0x10 | 0x20 | 0x40) != 0 && flags & DUPSORT == 0 {
            return Err(libc::EINVAL);
        }
        if flags & 0x20 != 0 && flags & 0x10 == 0 {
            return Err(libc::EINVAL);
        }
        let t = unsafe { get_mut(txn)? };
        let n = unsafe { t.checked_database_name(nameptr)? };
        let kind = if flags & DUPSORT != 0 {
            Some(DatabaseKind::Duplicates { width: None })
        } else {
            None
        };
        let dbi = open_db(t, n, kind, flags, false)?;
        if n == MAIN && !t.readonly && flags & compat_features::DB_FLAGS != 0 {
            let handle = t.handle(dbi)?;
            if t.visible_main_empty()? {
                let mut settings = t.settings(&handle)?;
                let requested = settings.flags | (flags & compat_features::DB_FLAGS);
                if requested != settings.flags {
                    t.mark_database_dirty(&handle)?;
                }
                settings.flags = requested;
                t.store_settings(&handle, settings)?;
            }
        }
        if n == MAIN
            && let Some(options) = t
                .shared
                .options
                .lock()
                .map_err(|_| BAD_TXN)?
                .get_mut(&compat_features::MAIN_CALLBACK_KEY)
        {
            // Upstream reinitializes both default comparators on every public
            // MAIN open. Internal virtual-to-native bootstrap is not an open.
            options.key_cmp = None;
            options.dup_cmp = None;
            options.native_key = None;
            options.native_dup = None;
        }
        *unsafe { get_mut(out)? } = dbi;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_dbi_open(
    txn: *mut MDB_txn,
    nameptr: *const c_char,
    kind: u32,
    flags: u32,
    out: *mut u32,
) -> c_int {
    boundary(|| {
        if flags & !CREATE != 0 {
            return Err(libc::EINVAL);
        }
        let kind = match kind {
            0 => DatabaseKind::Kv,
            1 => DatabaseKind::Duplicates { width: None },
            4 => DatabaseKind::Ipv4,
            6 => DatabaseKind::Ipv6,
            _ => return Err(libc::EINVAL),
        };
        let t = unsafe { get_mut(txn)? };
        let n = unsafe { t.checked_database_name(nameptr)? };
        let dbi = open_db(t, n, Some(kind), flags, true)?;
        if t.peek_handle(dbi)?.kind() != kind {
            return Err(INCOMPATIBLE);
        }
        *unsafe { get_mut(out)? } = dbi;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_dbi_close(env: *mut MDB_env, dbi: u32) {
    if dbi < 2 {
        return;
    }
    if let Some(e) = unsafe { env.as_ref() }
        && let Some(shared) = &e.shared
        && let Ok(mut handles) = shared.handles.lock()
        && let Ok(mut aliases) = shared.aliases.lock()
        && !aliases.reserved.contains_key(&dbi)
        && let Some(slot) = handles.get_mut(dbi as usize)
        && slot.is_some()
    {
        let handle = aliases.replace_handle(dbi, slot, None).unwrap();
        aliases.set_dormant(dbi, None, false);
        if !handles
            .iter()
            .flatten()
            .any(|bound| bound.id() == handle.id())
            && let Ok(mut options) = shared.options.lock()
        {
            options.remove(&handle.id());
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_dbi_flags(txn: *mut MDB_txn, dbi: u32, out: *mut u32) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        if let Some(flags) = t.pending_flags(dbi)? {
            *unsafe { get_mut(out)? } = flags;
            return Ok(());
        }
        t.flush_reserves()?;
        let h = t.peek_handle(dbi)?;
        *unsafe { get_mut(out)? } = t.db_flags(&h)?;
        Ok(())
    })
}

/// Admission lasts for this operation only. Immutable bytewise snapshots need
/// no alias refresh or comparator binding after the checked native view opens.
/// Transforms/external authorities and mutable/private views retain the general
/// path, including its callback observation order and reserve semantics.
fn ordinary_read_value<'a>(
    t: &'a MDB_txn,
    h: &ResolvedAlias,
    key: &[u8],
    settings: compat_features::Settings,
) -> FfiResult<Option<Option<&'a [u8]>>> {
    if h.kind() != DatabaseKind::Kv
        || settings.flags != 0
        || !matches!(h.binding.view, DataView::Direct)
    {
        return Ok(None);
    }
    let Some(State::Read(read)) = t.state.as_ref() else {
        return Ok(None);
    };
    if read.external_synchronization().is_some() || read.transform_context().is_some() {
        return Ok(None);
    }
    // These catalog observations cannot invoke user code. A failed/nonbytewise
    // observation falls back, preserving the existing error/callback ordering.
    if !read
        .database_ordering(h.actual().name())
        .is_ok_and(|order| order.keys == ulmdb::ordering::OrderingSpec::Bytewise)
    {
        return Ok(None);
    }
    {
        let options = t.shared.options.lock().map_err(|_| BAD_TXN)?;
        if options
            .get(&compat_features::callback_key(h))
            .is_some_and(|options| options.key_cmp.is_some())
        {
            return Ok(None);
        }
    }
    // Unlike check_native_actual + get_with_ordering, this validates the exact
    // snapshot/owner/ID once without allocating a replacement DatabaseHandle.
    let view = read.kv_handle(h.actual()).map_err(code)?;
    Ok(Some(view.get(key).map_err(code)?))
}

fn ordinary_write_alias(
    t: &MDB_txn,
    h: &ResolvedAlias,
    settings: compat_features::Settings,
) -> FfiResult<bool> {
    if h.kind() != DatabaseKind::Kv
        || settings.flags != 0
        || !matches!(h.binding.view, DataView::Direct)
        || !matches!(t.state, Some(State::Write(_)))
        || t.shared.transform.is_some()
        || t.shared.external.is_some()
    {
        return Ok(false);
    }
    {
        let options = t.shared.options.lock().map_err(|_| BAD_TXN)?;
        if options
            .get(&compat_features::callback_key(h))
            .is_some_and(|options| options.key_cmp.is_some())
        {
            return Ok(false);
        }
    }
    if t.actual_native_order(h)?.keys != ulmdb::ordering::OrderingSpec::Bytewise {
        return Ok(false);
    }
    let state = t.dbis.lock().map_err(|_| BAD_TXN)?;
    Ok(!state.detached.contains_key(&h.native_id())
        && !state.retained.contains_key(&h.native_id())
        && !state.hidden.contains(&h.native_id()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_get(
    txn: *mut MDB_txn,
    dbi: u32,
    key: *mut MDB_val,
    value: *mut MDB_val,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        unsafe {
            get_mut(key)?;
            get_mut(value)?;
        }
        let (h, checked_settings) = t.operation_handle(dbi)?;
        let key = unsafe { bytes(key)? };
        let settings = match checked_settings {
            Some(settings) => {
                t.check_key_settings(key, settings)?;
                settings
            }
            None => t.checked_key_settings(&h, key)?,
        };
        if let Some(reserved) = t.reserve_value(&h, key) {
            return set_value(value, reserved);
        }
        #[cfg(feature = "sql-hash")]
        if let Some(found) =
            crate::cursor_stream::sql_batch::sql_hash::ordinary_get(t, &h, key, settings)?
        {
            let found = found.ok_or(NOTFOUND)?;
            *unsafe { get_mut(value)? } = found;
            return Ok(());
        }
        if let Some(found) = ordinary_read_value(t, &h, key, settings)? {
            if let Some(found) = found {
                return set_value(value, found);
            }
            if h.is_main() && t.catalog_handle_for_key(key)?.is_some() {
                let (_, descriptor) = t
                    .catalog_pairs()?
                    .into_iter()
                    .find(|(name, _)| name == key)
                    .ok_or(NOTFOUND)?;
                return t.retain_pair_output(value, descriptor);
            }
            return Err(NOTFOUND);
        }
        if h.name() == MAIN
            && t.lookup(&h, key)?.is_none()
            && t.catalog_handle_for_key(key)?.is_some()
        {
            let (_, descriptor) = t
                .catalog_pairs()?
                .into_iter()
                .find(|(name, _)| name == key)
                .ok_or(NOTFOUND)?;
            return t.retain_pair_output(value, descriptor);
        }
        if (t.custom_key(&h)
            || (matches!(h.kind(), DatabaseKind::Duplicates { .. })
                && (t.db_flags(&h)? & 0x60 != 0 || t.custom_dup(&h))))
            && !t.native_order_matches(&h)?
        {
            let row = t
                .compat_pairs(&h)?
                .into_iter()
                .find(|(stored, _)| t.key_order(&h, stored, key).is_eq())
                .ok_or(NOTFOUND)?;
            t.retain_pair_output(value, row.1)
        } else {
            set_value(value, t.lookup(&h, key)?.ok_or(NOTFOUND)?)
        }
    })
}
fn put_one(
    t: &mut MDB_txn,
    h: &ResolvedAlias,
    key: &[u8],
    data: &[u8],
    flags: u32,
    out: *mut MDB_val,
) -> FfiResult<()> {
    // Public input buffers have been copied by the caller before an update
    // materializes and retires a prior writable reserve view.
    t.finish_reserves()?;
    // A first ordered insert may have replaced this empty branch's cap during
    // an earlier MULTIPLE element; refresh only the same exact logical/actual binding.
    let current = {
        let state = t.dbis.lock().map_err(|_| BAD_TXN)?;
        state.bindings.get(&h.id()).cloned()
    };
    let refreshed = current
        .filter(|b| b.actual == *h.actual())
        .map(|binding| ResolvedAlias {
            alias: h.alias.clone(),
            binding,
        });
    let h = refreshed.as_ref().unwrap_or(h);
    if h.name() == MAIN && t.catalog_handle_for_key_unmasked(key)?.is_some() {
        return Err(INCOMPATIBLE);
    }
    let settings = t.checked_sizes(h, key, Some(data), true)?;
    if t.readonly {
        return Err(t.shared.readonly_error());
    }
    #[cfg(feature = "sql-batch")]
    if flags == 0x10
        && cursor_stream::sql_batch::sql_write::try_nooverwrite(t, h, key, data, out, settings)?
    {
        return Ok(());
    }
    if flags == 0 && ordinary_write_alias(t, h, settings)? {
        // With bytewise equality the canonical key is the already-copied
        // input. Reuse this one presence/length read for map quota admission.
        let previous = t.lookup(h, key)?.map(<[u8]>::len);
        t.writable_database(h)?;
        let added = data.len()
            + if previous.is_none() {
                key.len() + 16
            } else {
                0
            };
        t.quota_adjust(added, previous.unwrap_or(0))?;
        // Flags zero cannot learn integer/fixed widths. Keep fresh mutation
        // ordering/capability admission and all value-limit/reserve handling.
        let ordered = t.configure_native_order(h)?;
        t.put_alias(&ordered, key, data)?;
        t.mark_database_dirty(h)?;
        t.changed();
        return Ok(());
    }
    let existing = t.canonical_key(h, key)?;
    let canonical = existing.as_deref().unwrap_or(key);
    let duplicate = matches!(h.kind(), DatabaseKind::Duplicates { .. });
    if flags & 0x10 != 0 && existing.is_some() {
        let value = if t.native_order_matches(h)? {
            t.lookup(h, canonical)?.map(|v| v.to_vec())
        } else {
            t.compat_pairs(h)?
                .into_iter()
                .find(|(stored, _)| t.key_order(h, stored, canonical).is_eq())
                .map(|(_, v)| v)
        };
        if let Some(value) = value {
            t.retain_pair_output(out, value)?;
        }
        return Err(KEYEXIST);
    }
    let ordered = t.native_order_matches(h)?;
    let rows = if !ordered && flags & (0x20000 | 0x40000 | 0x20) != 0 {
        t.compat_pairs(h)?
    } else {
        Vec::new()
    };
    let append_edge = if ordered && flags & 0x20000 != 0 {
        cursor_stream::select(t, h, cursor_stream::Query::edge(true))?.map(|s| s.position().clone())
    } else {
        rows.last().cloned()
    };
    let dup_edge = if ordered && duplicate && flags & 0x40000 != 0 {
        cursor_stream::select(
            t,
            h,
            cursor_stream::Query::bounded(canonical, None, true, true).same_key(canonical),
        )?
        .map(|s| s.position().clone())
    } else {
        rows.iter()
            .rev()
            .find(|(stored, _)| t.key_order(h, stored, canonical).is_eq())
            .cloned()
    };
    if flags & 0x20000 != 0
        && append_edge
            .as_ref()
            .is_some_and(|(last, _)| !t.key_order(h, last, key).is_lt())
    {
        return Err(KEYEXIST);
    }
    if duplicate
        && flags & 0x40000 != 0
        && dup_edge
            .as_ref()
            .is_some_and(|(_, last)| !t.value_order(h, last, data).is_lt())
    {
        t.writable_database(h)?;
        t.mark_database_dirty(h)?;
        return Err(KEYEXIST);
    }
    if duplicate {
        let equivalent = if t.custom_dup(h) && !t.native_order_matches(h)? {
            t.compat_pairs(h)?.into_iter().any(|(stored, value)| {
                t.key_order(h, &stored, canonical).is_eq() && t.value_order(h, &value, data).is_eq()
            })
        } else {
            t.seek_database_pair(h, Some((canonical, Some(data))), true, false)
                .map_err(code)?
                .is_some_and(|(key, value)| {
                    t.key_order(h, key, canonical).is_eq() && t.value_order(h, value, data).is_eq()
                })
        };
        if equivalent {
            t.writable_database(h)?;
            // LMDB touches the outer duplicate tree before equality testing,
            // including the recoverable NODUPDATA/KEYEXIST branch.
            t.mark_database_dirty(h)?;
            if flags & 0x20 != 0 {
                return Err(KEYEXIST);
            }
            return Ok(());
        }
        t.writable_database(h)?;
        t.quota_adjust(canonical.len() + data.len() + 16, 0)?;
        t.remember_sizes(h, canonical, data)?;
        let ordered = t.configure_native_order(h)?;
        t.insert_alias_duplicate(&ordered, canonical, data)?;
    } else if h.kind() == DatabaseKind::Kv {
        t.writable_database(h)?;
        let previous = t.lookup(h, canonical)?.map(<[u8]>::len);
        let added = data.len()
            + if previous.is_none() {
                canonical.len() + 16
            } else {
                0
            };
        t.quota_adjust(added, previous.unwrap_or(0))?;
        t.remember_sizes(h, canonical, data)?;
        let ordered = t.configure_native_order(h)?;
        t.put_alias(&ordered, canonical, data)?;
    } else {
        return Err(INCOMPATIBLE);
    }
    t.mark_database_dirty(h)?;
    t.changed();
    Ok(())
}
fn put_reserve(
    t: &mut MDB_txn,
    h: &ResolvedAlias,
    key: &[u8],
    size: usize,
    flags: u32,
    out: *mut MDB_val,
) -> FfiResult<()> {
    t.finish_reserves()?;
    if h.name() == MAIN && t.catalog_handle_for_key_unmasked(key)?.is_some() {
        return Err(INCOMPATIBLE);
    }
    t.check_sizes(h, key, None, true)?;
    let existing = t.canonical_key(h, key)?;
    let canonical = existing.as_deref().unwrap_or(key);
    if flags & 0x10 != 0 && existing.is_some() {
        let previous = if t.native_order_matches(h)? {
            t.lookup(h, canonical)?.map(|v| v.to_vec())
        } else {
            t.compat_pairs(h)?
                .into_iter()
                .find(|(stored, _)| t.key_order(h, stored, canonical).is_eq())
                .map(|(_, value)| value)
        };
        if let Some(value) = previous {
            t.retain_pair_output(out, value)?;
        }
        return Err(KEYEXIST);
    }
    if flags & 0x20000 != 0 {
        let incompatible = if t.native_order_matches(h)? {
            t.seek_database_pair(h, None, false, true)
                .map_err(code)?
                .is_some_and(|(last, _)| !t.key_order(h, last, key).is_lt())
        } else {
            t.compat_pairs(h)?
                .last()
                .is_some_and(|(last, _)| !t.key_order(h, last, key).is_lt())
        };
        if incompatible {
            return Err(KEYEXIST);
        }
    }
    t.writable_database(h)?;
    let previous = t.lookup(h, canonical)?.map(<[u8]>::len);
    let added = size
        + if previous.is_none() {
            canonical.len() + 16
        } else {
            0
        };
    t.quota_adjust(added, previous.unwrap_or(0))?;
    t.remember_sizes(h, canonical, &[])?;
    let ordered = t.configure_native_order(h)?;
    let reservation = t.reserve_alias(&ordered, canonical, size)?;
    t.mark_database_dirty(h)?;
    t.changed();
    // All graph/metadata allocations finish before exporting the actual Arena
    // pointer. Reads and cursors do not relocate this initialized allocation.
    let pointer = t
        .writer()?
        .reservation_value_mut(&reservation)
        .map_err(code)?
        .as_mut_ptr();
    unsafe { get_mut(out)? }.mv_data = pointer.cast();
    t.pending_reserves
        .push(compat_features::Reserve { reservation });
    Ok(())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_put(
    txn: *mut MDB_txn,
    dbi: u32,
    key: *mut MDB_val,
    value: *mut MDB_val,
    flags: u32,
) -> c_int {
    // MULTIPLE belongs to cursor_put, not the ordinary LMDB put entry point.
    if flags & 0x80000 != 0 {
        return libc::EINVAL;
    }
    unsafe { put_value(txn, dbi, key, value, flags) }
}
pub(super) unsafe fn put_value(
    txn: *mut MDB_txn,
    dbi: u32,
    key: *mut MDB_val,
    value: *mut MDB_val,
    flags: u32,
) -> c_int {
    boundary(|| {
        if flags & !(0x10 | 0x20 | 0x10000 | 0x20000 | 0x40000 | 0x80000) != 0 {
            return Err(libc::EINVAL);
        }
        let t = unsafe { get_mut(txn)? };
        t.live()?;
        if t.readonly {
            return Err(t.shared.readonly_error());
        }
        unsafe {
            get_ref(value)?;
        }
        let key = unsafe { bytes(key)? }.to_vec();
        t.flush_reserves()?;
        let h = t.handle(dbi)?;
        if flags & 0x10000 != 0 {
            if matches!(h.kind(), DatabaseKind::Duplicates { .. }) {
                return Err(INCOMPATIBLE);
            }
            let size = unsafe { value.as_ref() }.ok_or(libc::EINVAL)?.mv_size;
            t.check_file_value_size(size)?;
            return put_reserve(t, &h, &key, size, flags & !0x10000, value);
        }
        if flags & 0x80000 != 0 {
            let count = unsafe { value.add(1).as_ref() }
                .ok_or(libc::EINVAL)?
                .mv_size;
            unsafe { (*value.add(1)).mv_size = 0 };
            if t.db_flags(&h)? & 0x10 == 0 {
                return Err(INCOMPATIBLE);
            }
            let input = unsafe { value.as_ref() }.ok_or(libc::EINVAL)?;
            let width = input.mv_size;
            let original = input.mv_data;
            if width == 0 || width > t.shared.max_key_size() {
                return Err(BAD_VALSIZE);
            }
            let total = count.checked_mul(width).ok_or(BAD_VALSIZE)?;
            if total > isize::MAX as usize || (total != 0 && input.mv_data.is_null()) {
                return Err(libc::EINVAL);
            }
            let data = if total == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(input.mv_data.cast::<u8>(), total) }.to_vec()
            };
            for (index, chunk) in data.chunks_exact(width).enumerate() {
                // LMDB advances the input view to the element being attempted,
                // including the first failed element of a partial insertion.
                unsafe { (*value).mv_data = original.cast::<u8>().add(index * width).cast() };
                put_one(t, &h, &key, chunk, flags & !0x80000, value)?;
                unsafe {
                    (*value.add(1)).mv_size += 1;
                }
            }
            return Ok(());
        }
        t.check_file_value_size(unsafe { value.as_ref() }.ok_or(libc::EINVAL)?.mv_size)?;
        let data = unsafe { bytes(value)? }.to_vec();
        put_one(t, &h, &key, &data, flags, value)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_del(
    txn: *mut MDB_txn,
    dbi: u32,
    key: *mut MDB_val,
    value: *mut MDB_val,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        if t.readonly {
            return Err(t.shared.readonly_error());
        }
        let key = unsafe { bytes(key)? }.to_vec();
        t.finish_reserves()?;
        let h = t.handle(dbi)?;
        let data = if value.is_null() || !matches!(h.kind(), DatabaseKind::Duplicates { .. }) {
            None
        } else {
            Some(unsafe { bytes(value)? }.to_vec())
        };
        let settings = t.checked_sizes(&h, &key, data.as_deref(), false)?;
        if h.name() == MAIN && t.catalog_handle_for_key_unmasked(&key)?.is_some() {
            return Err(INCOMPATIBLE);
        }
        let canonical = if h.is_main()
            && settings.flags == 0
            && h.kind() == DatabaseKind::Kv
            && matches!(h.binding.view, DataView::Direct)
            && matches!(t.state, Some(State::Write(_)))
            && t.shared.transform.is_none()
            && t.shared.external.is_none()
        {
            // Run the same presence/comparator pipeline. Only its ordinary
            // input-key allocation is avoidable; custom canonical keys remain
            // owned, and deletion/cursor notification ordering is unchanged.
            t.canonical_key_view(&h, &key)?.ok_or(NOTFOUND)?
        } else {
            std::borrow::Cow::Owned(t.canonical_key(&h, &key)?.ok_or(NOTFOUND)?)
        };
        let pair = if matches!(h.kind(), DatabaseKind::Duplicates { .. }) {
            if let Some(value) = &data {
                if t.native_order_matches(&h)? {
                    Some(
                        t.seek_database_pair(&h, Some((&canonical, Some(value))), true, false)
                            .map_err(code)?
                            .filter(|(k, v)| {
                                t.key_order(&h, k, &canonical).is_eq()
                                    && t.value_order(&h, v, value).is_eq()
                            })
                            .ok_or(NOTFOUND)?
                            .1
                            .to_vec(),
                    )
                } else {
                    Some(
                        t.compat_pairs(&h)?
                            .into_iter()
                            .find(|(k, v)| {
                                t.key_order(&h, k, &canonical).is_eq()
                                    && t.value_order(&h, v, value).is_eq()
                            })
                            .ok_or(NOTFOUND)?
                            .1,
                    )
                }
            } else {
                None
            }
        } else {
            None
        };
        let existed = match h.kind() {
            DatabaseKind::Kv => {
                t.delete_alias(&h, &canonical)?;
                true
            }
            DatabaseKind::Duplicates { .. } => match pair {
                Some(value) => t.delete_alias_duplicate(&h, &canonical, &value)?,
                None => t.delete_alias_duplicates(&h, &canonical)? != 0,
            },
            _ => return Err(INCOMPATIBLE),
        };
        if !existed {
            return Err(NOTFOUND);
        }
        t.changed();
        t.mark_database_dirty(&h)?;
        t.quota_used = None;
        cursor::refresh_deleted_positions(t, dbi)?;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_drop(txn: *mut MDB_txn, dbi: u32, delete: c_int) -> c_int {
    boundary(|| {
        if !matches!(delete, 0 | 1) {
            return Err(libc::EINVAL);
        }
        let t = unsafe { get_mut(txn)? };
        if t.readonly {
            return Err(t.shared.readonly_error());
        }
        // The modern main-DB busy check precedes its blocked cursor-open
        // check upstream, including for a pending prepared transaction.
        if matches!(t.state, Some(State::Prepared(_)))
            && dbi == 1
            && t.shared.catalog_terminator
            && !t.dbis.lock().map_err(|_| BAD_TXN)?.valid.is_empty()
        {
            return Err(-30773);
        }
        t.finish_reserves()?;
        let h = t.handle(dbi)?;
        t.quota_used = None;
        // LMDB 1.0 rejects clearing the main catalog while named capabilities
        // remain transaction-valid. The 0.9 interface retains loaded roots.
        t.writer()?;
        if h.name() == MAIN && t.shared.catalog_terminator {
            if !t.dbis.lock().map_err(|_| BAD_TXN)?.valid.is_empty() {
                return Err(-30773);
            }
        }
        // LMDB 1.0 returns before dirtying an already-empty ordinary main DB.
        if h.name() == MAIN
            && t.shared.catalog_terminator
            && t.db_flags(&h)? & (DUPSORT | 8) == 0
            && t.entry_count(&h)? == 0
        {
            return Ok(());
        }
        let invalidated = [dbi];
        if h.is_main() {
            if let Err(error) = t.clear_main_catalog() {
                // A partially executed catalog clear cannot be acknowledged
                // by a subsequent commit. The owner can always abort safely.
                t.capacity_failed = true;
                return Err(error);
            }
            t.clear_alias(&h)?;
        } else if delete != 0 {
            if matches!(h.kind(), DatabaseKind::Kv | DatabaseKind::Duplicates { .. }) {
                // Drop's tree clear is independent of put's MAIN cursor-touch
                // permission. Removing the descriptor below is source-ordered.
                t.clear_alias(&h)?;
                let actual = match t.visible_named(h.name())? {
                    alias::VisibleName::Named(actual) => actual,
                    alias::VisibleName::Ordinary => {
                        t.capacity_failed = true;
                        return Err(INCOMPATIBLE);
                    }
                    alias::VisibleName::Absent => {
                        t.capacity_failed = true;
                        return Err(NOTFOUND);
                    }
                };
                t.retire_actual_aliases(&actual)?;
            } else {
                t.check_native_actual(h.actual()).map_err(code)?;
                t.writer()?.drop_database_handle(h.actual()).map_err(code)?;
                t.remove_database_name(h.actual().name())?;
            }
            transaction::reserve_closed_handle(t, dbi, &h)?;
            let mut state = t.dbis.lock().map_err(|_| BAD_TXN)?;
            state.dirty.remove(&h.id());
            state.settings_dirty.remove(&h.id());
        } else {
            t.clear_alias(&h)?;
        }
        for &pointer in &t.cursors {
            let cursor = unsafe { get_mut(pointer)? };
            if invalidated.contains(&cursor.dbi) {
                cursor.position = None;
                cursor.deleted = false;
                cursor.deleted_from = None;
                cursor.multiple.clear();
            }
        }
        if delete == 0 || h.name() == MAIN {
            t.mark_database_dirty(&h)?;
            let mut settings = t.settings(&h)?;
            if settings.key_width != 0 || settings.dup_width != 0 {
                settings.key_width = 0;
                settings.dup_width = 0;
                t.store_settings(&h, settings)?;
            }
        }
        t.mark_publication_dirty()?;
        t.changed();
        Ok(())
    })
}
pub(super) fn logical_stat(entries: usize, page_size: usize) -> MDB_stat {
    MDB_stat {
        ms_psize: page_size as u32,
        ms_depth: 0,
        ms_branch_pages: 0,
        ms_leaf_pages: 0,
        ms_overflow_pages: 0,
        ms_entries: entries,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_stat(txn: *mut MDB_txn, dbi: u32, out: *mut MDB_stat) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        unsafe {
            get_mut(out)?;
        }
        t.flush_reserves()?;
        t.peek_handle(dbi)?;
        let result = match t.handle(dbi) {
            Ok(handle) => t.alias_physical_stat(&handle)?,
            // Upstream ignores cursor-init refresh failure here. Missing stale
            // counts are not an oracle; preserve bounded empty fallback fields.
            Err(NOTFOUND | BAD_DBI | INCOMPATIBLE) => logical_stat(0, t.shared.page_size.get()),
            Err(error) => return Err(error),
        };
        *unsafe { get_mut(out)? } = result;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_set_compare(
    txn: *mut MDB_txn,
    dbi: u32,
    callback: *mut c_void,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        let h = t.peek_handle(dbi)?;
        let callback = if callback.is_null() {
            None
        } else {
            Some(unsafe {
                std::mem::transmute::<*mut c_void, compat_features::Comparator>(callback)
            })
        };
        {
            let mut all = t.shared.options.lock().map_err(|_| BAD_TXN)?;
            let o = all.entry(compat_features::callback_key(&h)).or_default();
            o.key_cmp = callback;
            o.native_key = callback.map(native_order::native_callback);
        }
        t.changed();
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_set_dupsort(
    txn: *mut MDB_txn,
    dbi: u32,
    callback: *mut c_void,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        let h = t.peek_handle(dbi)?;
        if !matches!(h.kind(), DatabaseKind::Duplicates { .. }) {
            return Err(INCOMPATIBLE);
        }
        let callback = if callback.is_null() {
            None
        } else {
            Some(unsafe {
                std::mem::transmute::<*mut c_void, compat_features::Comparator>(callback)
            })
        };
        {
            let mut all = t.shared.options.lock().map_err(|_| BAD_TXN)?;
            let o = all.entry(compat_features::callback_key(&h)).or_default();
            o.dup_cmp = callback;
            o.native_dup = callback.map(native_order::native_callback);
        }
        t.changed();
        Ok(())
    })
}

#[cfg(test)]
mod get_tests {
    use super::*;
    use std::ffi::CString;

    struct Fixture {
        env: *mut MDB_env,
        path: std::path::PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            Self::with_mapsize(0)
        }
        fn with_mapsize(mapsize: usize) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ulmdb-c-get-{}-{}",
                std::process::id(),
                ulmdb_storage::new_identity().unwrap()
            ));
            let name = CString::new(path.to_str().unwrap()).unwrap();
            let mut env = ptr::null_mut();
            unsafe {
                assert_eq!(environment::mdb_env_create(&mut env), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(env, 4), 0);
                if mapsize != 0 {
                    assert_eq!(environment::mdb_env_set_mapsize(env, mapsize), 0);
                }
                assert_eq!(
                    environment::mdb_env_open(env, name.as_ptr(), 0x200000, 0o600),
                    0
                );
            }
            Self { env, path }
        }
        fn begin(&self, flags: u32) -> *mut MDB_txn {
            let mut txn = ptr::null_mut();
            assert_eq!(
                unsafe { transaction::mdb_txn_begin(self.env, ptr::null_mut(), flags, &mut txn) },
                0
            );
            txn
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe { environment::mdb_env_close(self.env) };
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
    fn val(bytes: &[u8]) -> MDB_val {
        MDB_val {
            mv_size: bytes.len(),
            mv_data: bytes.as_ptr().cast_mut().cast(),
        }
    }
    fn open(txn: *mut MDB_txn, name: *const c_char, flags: u32) -> u32 {
        let mut dbi = u32::MAX;
        assert_eq!(unsafe { mdb_dbi_open(txn, name, flags, &mut dbi) }, 0);
        dbi
    }
    fn put(txn: *mut MDB_txn, dbi: u32, key: &[u8], value: &[u8]) {
        assert_eq!(
            unsafe { mdb_put(txn, dbi, &mut val(key), &mut val(value), 0) },
            0
        );
    }
    fn get(txn: *mut MDB_txn, dbi: u32, key: &[u8]) -> Result<MDB_val, i32> {
        let mut result = val(&[]);
        match unsafe { mdb_get(txn, dbi, &mut val(key), &mut result) } {
            0 => Ok(result),
            code => Err(code),
        }
    }
    fn contents(value: MDB_val) -> Vec<u8> {
        unsafe { bytes(&value).unwrap() }.to_vec()
    }
    unsafe extern "C" fn folded(a: *const MDB_val, b: *const MDB_val) -> c_int {
        let a = unsafe { bytes(a).unwrap() };
        let b = unsafe { bytes(b).unwrap() };
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase)) as c_int
    }

    #[test]
    fn ordinary_snapshot_get_keeps_main_catalog_and_borrowed_values() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        let named = open(writer, c"named".as_ptr(), CREATE);
        put(writer, main, b"ordinary", b"main-value");
        put(writer, named, b"ordinary", b"named-value");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);

        let reader = fixture.begin(RDONLY);
        let borrowed = get(reader, main, b"ordinary").unwrap();
        assert_eq!(contents(borrowed), b"main-value");
        assert_eq!(
            contents(get(reader, named, b"ordinary").unwrap()),
            b"named-value"
        );
        assert_eq!(get(reader, main, b"absent").err(), Some(NOTFOUND));
        assert_eq!(get(reader, named, b"absent").err(), Some(NOTFOUND));
        let descriptor = get(reader, main, b"named").unwrap();
        assert_eq!(descriptor.mv_size, 8 + 5 * std::mem::size_of::<usize>());
        assert_eq!(contents(borrowed), b"main-value");
        unsafe { transaction::mdb_txn_abort(reader) };
    }

    #[test]
    fn comparator_changes_are_observed_between_snapshot_gets() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        let named = open(writer, c"named".as_ptr(), CREATE);
        put(writer, main, b"Alpha", b"main");
        put(writer, named, b"Alpha", b"named");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let reader = fixture.begin(RDONLY);
        for (dbi, expected) in [(main, b"main".as_slice()), (named, b"named".as_slice())] {
            assert_eq!(get(reader, dbi, b"alpha").err(), Some(NOTFOUND));
            assert_eq!(
                unsafe { mdb_set_compare(reader, dbi, folded as *mut c_void) },
                0
            );
            assert_eq!(contents(get(reader, dbi, b"alpha").unwrap()), expected);
            if dbi == main {
                assert_eq!(open(reader, ptr::null(), 0), main);
            } else {
                assert_eq!(unsafe { mdb_set_compare(reader, dbi, ptr::null_mut()) }, 0);
            }
            assert_eq!(get(reader, dbi, b"alpha").err(), Some(NOTFOUND));
            assert_eq!(contents(get(reader, dbi, b"Alpha").unwrap()), expected);
        }
        unsafe { transaction::mdb_txn_abort(reader) };
    }

    #[test]
    fn persisted_custom_main_order_keeps_default_reopen_fallback() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        assert_eq!(
            unsafe { mdb_set_compare(writer, main, folded as *mut c_void) },
            0
        );
        put(writer, main, b"Alpha", b"custom");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let reader = fixture.begin(RDONLY);
        assert_eq!(open(reader, ptr::null(), 0), main);
        assert_eq!(contents(get(reader, main, b"Alpha").unwrap()), b"custom");
        assert_eq!(get(reader, main, b"alpha").err(), Some(NOTFOUND));
        assert_eq!(
            unsafe { mdb_set_compare(reader, main, folded as *mut c_void) },
            0
        );
        assert_eq!(contents(get(reader, main, b"alpha").unwrap()), b"custom");
        unsafe { transaction::mdb_txn_abort(reader) };
    }

    #[test]
    fn reserve_pointers_and_get_error_order_survive_reset_and_renew() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        let mut reserved = MDB_val {
            mv_size: 4,
            mv_data: ptr::null_mut(),
        };
        assert_eq!(
            unsafe { mdb_put(writer, main, &mut val(b"reserve"), &mut reserved, 0x10000) },
            0
        );
        unsafe { std::ptr::copy_nonoverlapping(b"live".as_ptr(), reserved.mv_data.cast(), 4) };
        let borrowed = get(writer, main, b"reserve").unwrap();
        assert_eq!(borrowed.mv_data, reserved.mv_data);
        assert_eq!(contents(borrowed), b"live");
        unsafe { std::ptr::copy_nonoverlapping(b"edit".as_ptr(), reserved.mv_data.cast(), 4) };
        assert_eq!(contents(borrowed), b"edit");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);

        let reader = fixture.begin(RDONLY);
        assert_eq!(get(reader, main, &[]).err(), Some(BAD_VALSIZE));
        assert_eq!(get(reader, main, &[b'x'; 512]).err(), Some(BAD_VALSIZE));
        assert_eq!(get(reader, u32::MAX, &[]).err(), Some(BAD_DBI));
        assert_eq!(
            unsafe { mdb_get(reader, u32::MAX, &mut val(&[]), ptr::null_mut()) },
            libc::EINVAL
        );
        assert_eq!(contents(get(reader, main, b"reserve").unwrap()), b"edit");
        unsafe { transaction::mdb_txn_reset(reader) };
        assert_eq!(get(reader, main, b"reserve").err(), Some(BAD_TXN));
        assert_eq!(unsafe { transaction::mdb_txn_renew(reader) }, 0);
        assert_eq!(contents(get(reader, main, b"reserve").unwrap()), b"edit");
        unsafe { transaction::mdb_txn_abort(reader) };
    }

    #[test]
    fn ordinary_put_overwrites_reuse_map_quota_and_preserve_failed_write_abort() {
        let fixture = Fixture::with_mapsize(128 * 1024);
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        put(writer, main, b"a", &vec![1; 100_000]);
        put(writer, main, b"a", &vec![2; 120_000]);
        put(writer, main, b"a", &vec![3; 1_000]);
        put(writer, main, b"b", &vec![4; 60_000]);
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let reader = fixture.begin(RDONLY);
        assert_eq!(contents(get(reader, main, b"a").unwrap()), vec![3; 1_000]);
        assert_eq!(contents(get(reader, main, b"b").unwrap()), vec![4; 60_000]);
        unsafe { transaction::mdb_txn_abort(reader) };

        let writer = fixture.begin(0);
        assert_eq!(
            unsafe { mdb_put(writer, main, &mut val(b"c"), &mut val(&vec![5; 200_000]), 0) },
            -30792
        );
        unsafe { transaction::mdb_txn_abort(writer) };
        let reader = fixture.begin(RDONLY);
        assert_eq!(get(reader, main, b"c").err(), Some(NOTFOUND));
        assert_eq!(contents(get(reader, main, b"a").unwrap()), vec![3; 1_000]);
        unsafe { transaction::mdb_txn_abort(reader) };
    }

    #[test]
    fn ordinary_put_copies_reserve_inputs_and_keeps_nooverwrite_output() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        let mut reserved = MDB_val {
            mv_size: 8,
            mv_data: ptr::null_mut(),
        };
        assert_eq!(
            unsafe { mdb_put(writer, main, &mut val(b"source"), &mut reserved, 0x10000) },
            0
        );
        unsafe { std::ptr::copy_nonoverlapping(b"copy-key".as_ptr(), reserved.mv_data.cast(), 8) };
        // Both foreign inputs borrow the initialized reserve that put_one ends.
        let mut borrowed_key = reserved;
        let mut borrowed_data = reserved;
        assert_eq!(
            unsafe { mdb_put(writer, main, &mut borrowed_key, &mut borrowed_data, 0) },
            0
        );
        assert_eq!(
            contents(get(writer, main, b"copy-key").unwrap()),
            b"copy-key"
        );
        assert_eq!(contents(get(writer, main, b"source").unwrap()), b"copy-key");
        let mut replacement = val(b"replacement");
        assert_eq!(
            unsafe { mdb_put(writer, main, &mut val(b"copy-key"), &mut replacement, 0x10) },
            KEYEXIST
        );
        assert_eq!(contents(replacement), b"copy-key");
        assert_eq!(
            contents(get(writer, main, b"copy-key").unwrap()),
            b"copy-key"
        );
        unsafe { transaction::mdb_txn_abort(writer) };
    }

    #[test]
    fn ordinary_delete_keeps_missing_and_live_cursor_notifications() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        for (key, value) in [(b"a", b"A"), (b"b", b"B"), (b"c", b"C")] {
            put(writer, main, key, value);
        }
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { cursor::mdb_cursor_open(writer, main, &mut cursor) },
            0
        );
        let mut key = val(b"b");
        let mut value = val(&[]);
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 15) },
            0
        );
        assert_eq!(
            unsafe { mdb_del(writer, main, &mut val(b"absent"), ptr::null_mut()) },
            NOTFOUND
        );
        // Ordinary KV ignores the data argument, then refreshes the cursor
        // before an insertion can fill the vacated position.
        assert_eq!(
            unsafe { mdb_del(writer, main, &mut val(b"b"), &mut val(b"ignored")) },
            0
        );
        put(writer, main, b"bb", b"BB");
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 4) },
            0
        );
        assert_eq!(contents(key), b"c");
        assert_eq!(contents(value), b"C");
        assert_eq!(get(writer, main, b"b").err(), Some(NOTFOUND));
        unsafe { cursor::mdb_cursor_close(cursor) };
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    }

    #[test]
    fn custom_write_canonical_keys_and_savepoint_abort_keep_fallback_semantics() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        assert_eq!(
            unsafe { mdb_set_compare(writer, main, folded as *mut c_void) },
            0
        );
        put(writer, main, b"Alpha", b"first");
        put(writer, main, b"alpha", b"replacement");
        assert_eq!(
            contents(get(writer, main, b"ALPHA").unwrap()),
            b"replacement"
        );
        assert_eq!(
            unsafe { mdb_del(writer, main, &mut val(b"aLpHa"), ptr::null_mut()) },
            0
        );
        assert_eq!(get(writer, main, b"Alpha").err(), Some(NOTFOUND));
        put(writer, main, b"parent", b"root");
        let mut child = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(fixture.env, writer, 0, &mut child) },
            0
        );
        put(child, main, b"PARENT", b"child");
        assert_eq!(get(writer, main, b"parent").err(), Some(BAD_TXN));
        assert_eq!(contents(get(child, main, b"parent").unwrap()), b"child");
        unsafe { transaction::mdb_txn_abort(child) };
        assert_eq!(contents(get(writer, main, b"parent").unwrap()), b"root");
        unsafe { transaction::mdb_txn_abort(writer) };
    }

    #[test]
    fn fused_cursor_metadata_preserves_callback_and_input_error_order() {
        thread_local! { static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
        unsafe extern "C" fn counted(a: *const MDB_val, b: *const MDB_val) -> c_int {
            CALLS.with(|calls| calls.set(calls.get() + 1));
            unsafe { folded(a, b) }
        }
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        put(writer, main, b"Alpha", b"value");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let reader = fixture.begin(RDONLY);
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { cursor::mdb_cursor_open(reader, main, &mut cursor) },
            0
        );
        let mut key = val(&[]);
        let mut value = val(&[]);
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 0) },
            0
        );
        assert_eq!(
            unsafe { mdb_set_compare(reader, main, counted as *mut c_void) },
            0
        );
        CALLS.with(|calls| calls.set(0));
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut val(&[]), &mut value, 15) },
            BAD_VALSIZE
        );
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, ptr::null_mut(), ptr::null_mut(), 1) },
            INCOMPATIBLE
        );
        assert_eq!(CALLS.with(std::cell::Cell::get), 0);
        key = val(b"alpha");
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 16) },
            0
        );
        assert_eq!(contents(key), b"Alpha");
        assert_eq!(contents(value), b"value");
        assert!(CALLS.with(std::cell::Cell::get) > 0);
        assert_eq!(unsafe { mdb_set_compare(reader, main, ptr::null_mut()) }, 0);
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut val(b"alpha"), &mut value, 15) },
            NOTFOUND
        );
        unsafe { transaction::mdb_txn_reset(reader) };
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, ptr::null_mut(), ptr::null_mut(), 0) },
            BAD_TXN
        );
        assert_eq!(unsafe { transaction::mdb_txn_renew(reader) }, 0);
        assert_eq!(unsafe { cursor::mdb_cursor_renew(reader, cursor) }, 0);
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 0) },
            0
        );
        assert_eq!(contents(key), b"Alpha");
        unsafe {
            cursor::mdb_cursor_close(cursor);
            transaction::mdb_txn_abort(reader);
        }
    }

    #[test]
    fn fused_read_metadata_keeps_dormant_snapshot_and_rejects_reused_slots() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let named = open(writer, c"original".as_ptr(), CREATE);
        put(writer, named, b"k", b"original");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let reader = fixture.begin(RDONLY);
        assert_eq!(contents(get(reader, named, b"k").unwrap()), b"original");
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { cursor::mdb_cursor_open(reader, named, &mut cursor) },
            0
        );
        let mut key = val(&[]);
        let mut value = val(&[]);
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 0) },
            0
        );
        let writer = fixture.begin(0);
        assert_eq!(unsafe { mdb_drop(writer, named, 1) }, 0);
        assert_eq!(get(reader, named, b"k").err(), Some(BAD_DBI));
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 4) },
            BAD_DBI
        );
        unsafe { transaction::mdb_txn_abort(writer) };
        assert!(
            unsafe { &*reader }
                .shared
                .aliases
                .lock()
                .unwrap()
                .dormant
                .contains(&named)
        );
        assert_eq!(contents(get(reader, named, b"k").unwrap()), b"original");
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 4) },
            0
        );
        assert_eq!(contents(value), b"original");
        unsafe { mdb_dbi_close(fixture.env, named) };
        let writer = fixture.begin(0);
        let reused = open(writer, c"replacement".as_ptr(), CREATE);
        assert_eq!(reused, named);
        put(writer, reused, b"k", b"replacement");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        assert_eq!(get(reader, named, b"k").err(), Some(libc::EINVAL));
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 4) },
            libc::EINVAL
        );
        unsafe {
            cursor::mdb_cursor_close(cursor);
            transaction::mdb_txn_abort(reader);
        }
        let reader = fixture.begin(RDONLY);
        assert_eq!(contents(get(reader, reused, b"k").unwrap()), b"replacement");
        unsafe { transaction::mdb_txn_abort(reader) };
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fused_warm_read_handles_reject_a_raw_fork_before_mutexes() {
        let fixture = Fixture::new();
        let writer = fixture.begin(0);
        let main = open(writer, ptr::null(), 0);
        put(writer, main, b"k", b"value");
        assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
        let reader = fixture.begin(RDONLY);
        assert_eq!(contents(get(reader, main, b"k").unwrap()), b"value");
        let mut cursor = ptr::null_mut();
        assert_eq!(
            unsafe { cursor::mdb_cursor_open(reader, main, &mut cursor) },
            0
        );
        let mut key = val(&[]);
        let mut value = val(&[]);
        assert_eq!(
            unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 0) },
            0
        );
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            let get_result = unsafe { mdb_get(reader, main, &mut val(b"k"), &mut value) };
            let cursor_result = unsafe { cursor::mdb_cursor_get(cursor, &mut key, &mut value, 4) };
            unsafe {
                libc::_exit(if get_result == BAD_TXN && cursor_result == BAD_TXN {
                    0
                } else {
                    2
                })
            };
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut status = 0;
        loop {
            let result = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
            if result == child {
                break;
            }
            assert!(result >= 0);
            if std::time::Instant::now() >= deadline {
                unsafe {
                    libc::kill(child, libc::SIGKILL);
                    libc::waitpid(child, &mut status, 0);
                }
                panic!("inherited read operation blocked before fork rejection");
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
        assert_eq!(contents(get(reader, main, b"k").unwrap()), b"value");
        unsafe {
            cursor::mdb_cursor_close(cursor);
            transaction::mdb_txn_abort(reader);
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_set_relfunc(
    txn: *mut MDB_txn,
    dbi: u32,
    callback: *mut c_void,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        let h = t.peek_handle(dbi)?;
        t.shared
            .options
            .lock()
            .map_err(|_| BAD_TXN)?
            .entry(h.id())
            .or_default()
            .relfunc = callback as usize;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_set_relctx(
    txn: *mut MDB_txn,
    dbi: u32,
    context: *mut c_void,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        let h = t.peek_handle(dbi)?;
        t.shared
            .options
            .lock()
            .map_err(|_| BAD_TXN)?
            .entry(h.id())
            .or_default()
            .relctx = context as usize;
        Ok(())
    })
}
fn comparison(
    txn: *mut MDB_txn,
    dbi: u32,
    a: *const MDB_val,
    b: *const MDB_val,
    duplicate: bool,
) -> c_int {
    let result = (|| -> FfiResult<std::cmp::Ordering> {
        let t = unsafe { get_mut(txn)? };
        let a = unsafe { bytes(a)? };
        let b = unsafe { bytes(b)? };
        if let Some(order) = t.pending_order(dbi, a, b, duplicate)? {
            return Ok(order);
        }
        t.flush_reserves()?;
        let h = match t.peek_handle(dbi) {
            Ok(handle) => handle,
            Err(error) => return t.invalid_alias_order(dbi, a, b, duplicate)?.ok_or(error),
        };
        Ok(if duplicate {
            t.value_order(&h, a, b)
        } else {
            t.key_order(&h, a, b)
        })
    })();
    match result {
        Ok(std::cmp::Ordering::Less) => -1,
        Ok(std::cmp::Ordering::Equal) => 0,
        Ok(std::cmp::Ordering::Greater) => 1,
        Err(_) => 0,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cmp(
    txn: *mut MDB_txn,
    dbi: u32,
    a: *const MDB_val,
    b: *const MDB_val,
) -> c_int {
    comparison(txn, dbi, a, b, false)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_dcmp(
    txn: *mut MDB_txn,
    dbi: u32,
    a: *const MDB_val,
    b: *const MDB_val,
) -> c_int {
    comparison(txn, dbi, a, b, true)
}
fn address(text: &str) -> FfiResult<ulmdb::Address> {
    match text.parse::<std::net::IpAddr>().map_err(|_| libc::EINVAL)? {
        std::net::IpAddr::V4(a) => Ok(ulmdb::Address::V4(u32::from(a))),
        std::net::IpAddr::V6(a) => Ok(ulmdb::Address::V6(u128::from(a))),
    }
}
fn prefix(text: &str) -> FfiResult<ulmdb::Prefix> {
    let (ip, len) = text.split_once('/').ok_or(libc::EINVAL)?;
    ulmdb::Prefix::new(address(ip)?, len.parse::<u8>().map_err(|_| libc::EINVAL)?)
        .map_err(|_| libc::EINVAL)
}
fn prefix_text(p: ulmdb::Prefix) -> String {
    match p.network() {
        ulmdb::Address::V4(a) => format!("{}/{}", std::net::Ipv4Addr::from(a), p.length()),
        ulmdb::Address::V6(a) => format!("{}/{}", std::net::Ipv6Addr::from(a), p.length()),
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_prefix_put(
    txn: *mut MDB_txn,
    dbi: u32,
    cidr: *const c_char,
    value: *const MDB_val,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        let h = t.handle(dbi)?;
        let p = prefix(unsafe { string(cidr)? })?;
        let v = unsafe { bytes(value)? }.to_vec();
        t.finish_reserves()?;
        t.writer()?
            .insert_prefix_handle(h.actual(), p, &v)
            .map_err(code)?;
        t.mark_database_dirty(&h)?;
        t.changed();
        Ok(())
    })
}
fn prefix_lookup<'a>(
    t: &'a MDB_txn,
    h: &ResolvedAlias,
    p: ulmdb::Prefix,
) -> FfiResult<Option<&'a [u8]>> {
    t.check_native_actual(h.actual()).map_err(code)?;
    let found = match t.state.as_ref() {
        Some(State::Read(r)) => r.exact_prefix(h.actual().name(), p),
        Some(State::PrivateRead(r)) => r.exact_prefix(h.actual().name(), p),
        Some(State::Write(w)) => w.exact_prefix(h.actual().name(), p),
        Some(State::Child(w)) => w.exact_prefix(h.actual().name(), p),
        _ => return Err(BAD_TXN),
    }
    .map_err(code)?;
    Ok(found.map(|(_, v)| v))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_prefix_get(
    txn: *mut MDB_txn,
    dbi: u32,
    cidr: *const c_char,
    value: *mut MDB_val,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        let h = t.handle(dbi)?;
        let p = prefix(unsafe { string(cidr)? })?;
        let v = prefix_lookup(t, &h, p)?.ok_or(NOTFOUND)?;
        set_value(value, v)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_prefix_delete(
    txn: *mut MDB_txn,
    dbi: u32,
    cidr: *const c_char,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        let h = t.handle(dbi)?;
        let p = prefix(unsafe { string(cidr)? })?;
        t.finish_reserves()?;
        if t.readonly {
            return Err(t.shared.readonly_error());
        }
        if prefix_lookup(t, &h, p)?.is_none() {
            return Err(NOTFOUND);
        }
        t.writer()?
            .delete_prefix_handle(h.actual(), p)
            .map_err(code)?;
        t.mark_database_dirty(&h)?;
        t.changed();
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_longest_match(
    txn: *mut MDB_txn,
    dbi: u32,
    ip: *const c_char,
    cidr: *mut MDB_val,
    value: *mut MDB_val,
) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        let h = t.handle(dbi)?;
        let a = address(unsafe { string(ip)? })?;
        t.check_native_actual(h.actual()).map_err(code)?;
        let found = match t.state.as_ref() {
            Some(State::Read(r)) => r.longest_match(h.actual().name(), a),
            Some(State::PrivateRead(r)) => r.longest_match(h.actual().name(), a),
            Some(State::Write(w)) => w.longest_match(h.actual().name(), a),
            Some(State::Child(w)) => w.longest_match(h.actual().name(), a),
            _ => return Err(BAD_TXN),
        }
        .map_err(code)?
        .ok_or(NOTFOUND)?;
        let p = prefix_text(found.0).into_bytes();
        // Values stay in the transaction's pinned engine storage. Only the
        // formatted CIDR needs scratch space, bounded by owned_output.
        set_value(value, found.1)?;
        t.owned_output(cidr, p)
    })
}
