use super::*;
#[cfg(test)]
thread_local! {
    static PANIC_DURING_PREPARE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_begin(
    env: *mut MDB_env,
    parent: *mut MDB_txn,
    flags: u32,
    out: *mut *mut MDB_txn,
) -> c_int {
    boundary(|| {
        let out = unsafe { get_mut(out)? };
        *out = ptr::null_mut();
        let e = unsafe { get_ref(env)? };
        let shared = e.shared.as_ref().ok_or(libc::EINVAL)?.clone();
        if shared.poisoned.load(std::sync::atomic::Ordering::Acquire) {
            return Err(-30795);
        }
        // Count setup across snapshot/reader-slot IO without holding the
        // lifecycle mutex: independent readers may enter concurrently.
        let _opening_transaction = rollback::OpeningTransaction::new(shared.clone())?;
        let thread = std::thread::current().id();
        let readonly = flags & RDONLY != 0;
        if !readonly && e.flags() & RDONLY != 0 {
            return Err(shared.readonly_error());
        }
        let mut writer_environment = None;
        let mut writer_lease = None;
        let mut reader_lease = None;
        let mut reader_setup = None;
        let mut write_role = previous::WriteRole::canonical();
        let mut initial_quota = None;
        let state = if !parent.is_null() {
            let p = unsafe { get_mut(parent)? };
            if p.readonly {
                return Err(libc::EINVAL);
            }
            if e.flags() & 0x80000 != 0 && !readonly {
                return Err(if shared.catalog_terminator {
                    libc::EINVAL
                } else {
                    BAD_TXN
                });
            }
            if !Arc::ptr_eq(&p.shared, &shared) {
                return Err(libc::EINVAL);
            }
            if readonly {
                if !shared.catalog_terminator {
                    return Err(BAD_TXN);
                }
                if !p.correct_thread()
                    || p.state.is_none()
                    || matches!(p.state, Some(State::Prepared(_)))
                    || p.capacity_failed
                    || !p.child.is_null()
                {
                    return Err(libc::EINVAL);
                }
                if !p
                    .private_readers
                    .as_ref()
                    .is_some_and(|readers| readers.active())
                {
                    // Only the first child may finish reserve staging. Once a
                    // view exists, siblings must never borrow the writer mutably.
                    p.finish_reserves()?;
                }
                let readers = p
                    .private_readers
                    .get_or_insert_with(Default::default)
                    .clone();
                let source: &WriteTxn<'static> = match p.state.as_ref() {
                    Some(State::Write(writer)) => writer,
                    Some(State::Child(writer)) => writer,
                    _ => return Err(BAD_TXN),
                };
                // Check the native failed state through an immutable catalog
                // lookup before issuing a new private-read capability.
                source.open_database(MAIN).map_err(|_| libc::EINVAL)?;
                // SAFETY: the separately allocated source stays fixed; live()
                // blocks native mutation, and consuming operations preflight
                // every ancestor before dropping any owner in the child chain.
                State::PrivateRead(unsafe { private_read::PrivateReadView::new(source, readers)? })
            } else {
                p.live()?;
                p.finish_reserves()?;
                let mut scope = p.writer()?.savepoint().map_err(code)?;
                let local = durability::local_policy(flags, shared.catalog_terminator);
                if local != ulmdb::Durability::Full {
                    scope.weaken_durability(local).map_err(code)?;
                }
                // SAFETY: parent is heap-stable, inaccessible until child ends,
                // and retained by the caller. Consume preflight protects any
                // private readonly descendants before recursive child teardown.
                State::Child(Owned::new(unsafe {
                    std::mem::transmute::<Savepoint<'_, '_>, Savepoint<'static, 'static>>(scope)
                }))
            }
        } else if readonly {
            let read = fixed_map::read(
                &shared,
                previous::transaction_snapshot(&shared, e.flags(), true)?,
            )?;
            reader_setup = Some(reader_slots::setup(
                &shared,
                e.flags() & 0x200000 != 0,
                e.flags() & 0x400000 != 0,
                read.sequence(),
                &mut reader_lease,
            )?);
            map_size::check(&shared, &read, e.mapsize)?;
            State::Read(read)
        } else {
            if shared.writer_thread.lock().map_err(|_| BAD_TXN)?.as_ref() == Some(&thread) {
                return Err(BAD_TXN);
            }
            let lease = if shared.external.is_some() {
                None
            } else {
                Some(process::WriterLease::acquire(&shared.directory)?)
            };
            let (engine, role) = previous::writer_owner(shared.clone())?;
            write_role = role;
            initial_quota = map_size::check_with_used(&shared, &engine.begin_read(), e.mapsize)?;
            let mut writer = engine.begin_write().map_err(code)?;
            // Configure physical allocation while the writer is pristine.
            // Explicit catalog metadata changes are transactional mutations.
            fixed_map::writer(&shared, &mut writer)?;
            write_map::begin(&mut writer, e)?;
            let profile = dbi_state::catalog_profile(&shared);
            if profile == ulmdb::CatalogCapacity::EXTENDED && writer.catalog_capacity() < profile {
                writer.set_catalog_capacity(profile).map_err(code)?;
            }
            let local = durability::local_policy(flags, shared.catalog_terminator);
            if local != ulmdb::Durability::Full {
                writer.weaken_durability(local).map_err(code)?;
            }
            if writer.open_database(MAIN).is_err() {
                writer
                    .create_database(MAIN, DatabaseKind::Kv)
                    .map_err(code)?;
                // Admission measured the immutable base before virtual MAIN
                // became a persisted native row. Charge its catalog overhead
                // without another complete value-tree walk on the first put.
                initial_quota = initial_quota.and_then(|used| used.checked_add(MAIN.len() + 48));
            }
            *shared.writer_thread.lock().map_err(|_| BAD_TXN)? = Some(thread);
            // Keep fallible setup in these local bindings: an error drops the
            // writer, then engine, then coordinator lease in that order. Move
            // ownership into the transaction only after setup has succeeded.
            writer_environment = Some(engine.clone());
            writer_lease = lease;
            // SAFETY: writer_environment retains the Arc<Environment> until
            // after the separately allocated transaction state is consumed.
            State::Write(Owned::new(unsafe {
                std::mem::transmute::<WriteTxn<'_>, WriteTxn<'static>>(writer)
            }))
        };
        let txnid = match &state {
            State::Read(read) => read.sequence() as usize,
            State::PrivateRead(_) => unsafe { (*parent).txnid },
            State::Child(_) => unsafe { (*parent).txnid },
            State::Prepared(_) => unreachable!("new transactions are never prepared"),
            State::Write(_) => writer_environment
                .as_ref()
                .unwrap()
                .stats()
                .sequence
                .saturating_add(1) as usize,
        };
        let dbis = if parent.is_null() {
            dbi_state::TxnDbis::from_shared(&shared)?
        } else {
            // This ledger is disjoint from the separately allocated native
            // writer borrowed by a child or its immutable private readers.
            unsafe { (*parent).dbis.lock().map_err(|_| BAD_TXN)?.clone() }
        };
        // Keep setup rollback armed through every fallible ledger step.
        reader_setup.map(|setup| setup.finish());
        let txn = Box::into_raw(Box::new(MDB_txn {
            state: Some(state),
            shared: shared.clone(),
            env,
            parent,
            child: ptr::null_mut(),
            private_readers: None,
            thread,
            readonly,
            txnid,
            notls: e.flags() & 0x200000 != 0,
            output: Vec::new(),
            cursors: Vec::new(),
            revision: 0,
            retired_rows: Vec::new(),
            catalog_index: Mutex::new(None),
            created_ids: Default::default(),
            dropped_dbis: Vec::new(),
            closed_dbis: Vec::new(),
            activated_dbis: Default::default(),
            dbis: Mutex::new(dbis),
            pending_reserves: Vec::new(),
            writer_environment,
            writer_lease,
            reader_lease,
            write_role,
            quota_used: if parent.is_null() {
                initial_quota
            } else {
                unsafe { (*parent).quota_used }
            },
            capacity_failed: false,
        }));
        if !parent.is_null() && !readonly {
            unsafe {
                (*parent).child = txn;
            }
        }
        *out = txn;
        Ok(())
    })
}
/// Re-arm only the next ordinary exact-root sweep. A short-lived C writer
/// may have skipped Native maintenance admission below its normal thresholds.
/// Its Shared policy outlives that owner, so Native's abort notification alone
/// cannot preserve this request. This never acquires deletion authority.
struct AbandonedPreparation {
    shared: Option<Arc<Shared>>,
}
impl AbandonedPreparation {
    fn new(shared: &Arc<Shared>, armed: bool) -> Self {
        Self {
            shared: (armed
                && cfg!(any(target_os = "linux", target_os = "macos", windows))
                && shared.creator == std::process::id()
                && shared.external.is_none())
            .then(|| shared.clone()),
        }
    }
    fn disarm(&mut self) {
        self.shared = None;
    }
}
impl Drop for AbandonedPreparation {
    fn drop(&mut self) {
        let Some(shared) = &self.shared else {
            return;
        };
        // Inherited/uncertain/NOLOCK owners must not lock or schedule cleanup.
        if shared.creator != std::process::id()
            || shared.external.is_some()
            || shared.poisoned.load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        if let Ok(mut owner) = shared.maintenance.lock() {
            owner.request_scan();
        }
    }
}

fn invalidate_cursors(txn: &mut MDB_txn) {
    for cursor in txn.cursors.drain(..) {
        if txn.readonly {
            if let Some(cursor) = unsafe { cursor.as_mut() } {
                cursor.txn = ptr::null_mut();
            }
        } else {
            // LMDB owns write cursors and closes them automatically with the writer.
            unsafe {
                drop(Box::from_raw(cursor));
            }
        }
    }
}
fn detach(txn: &mut MDB_txn) {
    if !txn.parent.is_null() {
        if txn.readonly {
            // Private-read leases detach without accessing the parent, which
            // may be resumed concurrently after a NOTLS child's last release.
            return;
        }
        // SAFETY: the parent outlives its child; accesses resume after state drop.
        unsafe {
            (*txn.parent).child = ptr::null_mut();
            (*txn.parent).changed();
        }
    } else if !txn.readonly
        && let Ok(mut thread) = txn.shared.writer_thread.lock()
    {
        // A waiter may already own the engine mutex by the time commit
        // returns. Never clear a newer writer's recursion marker.
        if thread.as_ref() == Some(&txn.thread) {
            *thread = None;
        }
    }
}
pub(super) fn clear_handles(shared: &Shared, identities: &[(u32, AliasId)]) {
    if let (Ok(mut handles), Ok(mut aliases), Ok(mut options)) = (
        shared.handles.lock(),
        shared.aliases.lock(),
        shared.options.lock(),
    ) {
        for &(slot, id) in identities {
            if let Some(bound) = handles.get_mut(slot as usize)
                && bound.as_ref().is_some_and(|handle| handle.id() == id)
            {
                aliases.replace_handle(slot, bound, None);
                aliases.set_dormant(slot, None, false);
            }
            if aliases.reserved.get(&slot).is_some_and(|r| r.alias == id) {
                aliases.set_reserved(
                    slot,
                    handles.get(slot as usize).and_then(Option::as_ref),
                    None,
                );
            }
            if !handles.iter().flatten().any(|handle| handle.id() == id) {
                options.remove(&id);
            }
        }
    }
}
fn clear_created_handles(shared: &Shared, identities: &std::collections::BTreeSet<NativeId>) {
    if let (Ok(mut handles), Ok(mut aliases), Ok(mut options)) = (
        shared.handles.lock(),
        shared.aliases.lock(),
        shared.options.lock(),
    ) {
        for (slot, bound) in handles.iter_mut().enumerate() {
            if let Some(handle) = bound.as_ref() {
                if identities.contains(&handle.native_id()) {
                    options.remove(&handle.id());
                    aliases.replace_handle(slot as u32, bound, None);
                    aliases.set_dormant(slot as u32, None, false);
                }
            }
        }
        let release: Vec<_> = aliases
            .reserved
            .iter()
            .filter_map(|(&slot, reservation)| {
                identities.contains(&reservation.native).then_some(slot)
            })
            .collect();
        for slot in release {
            aliases.set_reserved(
                slot,
                handles.get(slot as usize).and_then(Option::as_ref),
                None,
            );
        }
    }
}
/// Reserve a deleted alias before mutating its native database. Only the owning
/// transaction's outcome may release this slot or restore its former identity.
pub(super) fn reserve_closed_handle(
    txn: &mut MDB_txn,
    slot: u32,
    handle: &ResolvedAlias,
) -> FfiResult<()> {
    let mut handles = txn.shared.handles.lock().map_err(|_| BAD_TXN)?;
    let mut aliases = txn.shared.aliases.lock().map_err(|_| BAD_TXN)?;
    let mut options = txn.shared.options.lock().map_err(|_| BAD_TXN)?;
    if aliases.reserved.contains_key(&slot)
        || !handles
            .get(slot as usize)
            .and_then(Option::as_ref)
            .is_some_and(|bound| bound.id() == handle.id())
    {
        return Err(BAD_DBI);
    }
    if !txn
        .closed_dbis
        .iter()
        .any(|closed| closed.dbi == slot && closed.handle.id() == handle.id())
    {
        txn.closed_dbis.push(dbi_state::ClosedDbi {
            dbi: slot,
            handle: handle.alias.clone(),
            options: options.get(&handle.id()).cloned(),
        });
    }
    dbi_state::push_identity(&mut txn.dropped_dbis, slot, handle.id());
    aliases.set_reserved(
        slot,
        handles[slot as usize].as_ref(),
        Some(dbi_state::AliasReservation {
            alias: handle.id(),
            native: handle.alias.native_id(),
        }),
    );
    aliases.set_dormant(slot, handles[slot as usize].as_ref(), false);
    aliases.replace_handle(slot, &mut handles[slot as usize], None);
    if !handles
        .iter()
        .flatten()
        .any(|bound| bound.id() == handle.id())
    {
        options.remove(&handle.id());
    }
    txn.dbis.lock().map_err(|_| BAD_TXN)?.valid.remove(&slot);
    Ok(())
}
fn finish_handles(txn: &mut MDB_txn, committed: bool) {
    if !committed {
        if let (Ok(handles), Ok(mut aliases)) =
            (txn.shared.handles.lock(), txn.shared.aliases.lock())
        {
            for (&slot, &id) in &txn.activated_dbis {
                if !aliases.reserved.contains_key(&slot)
                    && handles
                        .get(slot as usize)
                        .and_then(Option::as_ref)
                        .is_some_and(|handle| handle.id() == id)
                {
                    aliases.set_dormant(
                        slot,
                        handles.get(slot as usize).and_then(Option::as_ref),
                        true,
                    );
                }
            }
        }
    }
    if txn.readonly && !txn.parent.is_null() {
        return;
    }
    if !committed {
        clear_created_handles(&txn.shared, &txn.created_ids);
        if let (Ok(mut handles), Ok(mut aliases), Ok(mut options)) = (
            txn.shared.handles.lock(),
            txn.shared.aliases.lock(),
            txn.shared.options.lock(),
        ) {
            for closed in txn.closed_dbis.iter().rev() {
                if !aliases
                    .reserved
                    .get(&closed.dbi)
                    .is_some_and(|r| r.alias == closed.handle.id())
                {
                    continue;
                }
                aliases.set_reserved(
                    closed.dbi,
                    handles.get(closed.dbi as usize).and_then(Option::as_ref),
                    None,
                );
                if txn.created_ids.contains(&closed.handle.native_id()) {
                    continue;
                }
                if let Some(slot) = handles.get_mut(closed.dbi as usize) {
                    if slot.is_none() {
                        aliases.replace_handle(closed.dbi, slot, Some(closed.handle.clone()));
                        aliases.set_dormant(closed.dbi, slot.as_ref(), true);
                        // Another active alias owns current callbacks for this
                        // identity. Recovery must not overwrite those options.
                        if !dbi_state::active_identity(&handles, &aliases, closed.handle.id()) {
                            options.insert(
                                closed.handle.id(),
                                closed.options.clone().unwrap_or_default(),
                            );
                        }
                    } else {
                        // A reserved slot can never legally be rebound.
                        txn.shared
                            .poisoned
                            .store(true, std::sync::atomic::Ordering::Release);
                    }
                }
            }
        }
    } else if !txn.parent.is_null() {
        let parent = unsafe { &mut *txn.parent };
        parent.created_ids.append(&mut txn.created_ids);
        for &(slot, id) in &txn.dropped_dbis {
            dbi_state::push_identity(&mut parent.dropped_dbis, slot, id);
        }
        for (&slot, &id) in &txn.activated_dbis {
            parent.activated_dbis.insert(slot, id);
        }
        for closed in txn.closed_dbis.drain(..) {
            if !parent
                .closed_dbis
                .iter()
                .any(|old| old.dbi == closed.dbi && old.handle.id() == closed.handle.id())
            {
                parent.closed_dbis.push(closed);
            }
        }
        parent.quota_used = txn.quota_used;
        if let (Ok(child), Ok(mut state)) = (txn.dbis.lock(), parent.dbis.lock()) {
            *state = child.clone();
        }
    } else {
        clear_handles(&txn.shared, &txn.dropped_dbis);
    }
}
fn drain_private_slots(txn: &mut MDB_txn) -> FfiResult<()> {
    let Some(readers) = txn.private_readers.clone() else {
        return Ok(());
    };
    let slots = std::mem::take(&mut *readers.opened_slots()?);
    for (dbi, id) in slots {
        let native = txn
            .shared
            .handles
            .lock()
            .map_err(|_| BAD_TXN)?
            .get(dbi as usize)
            .and_then(Option::as_ref)
            .filter(|h| h.id() == id)
            .map(AliasHandle::native_id);
        if let Some(native) = native {
            operations::record_created_slot(txn, dbi, id, native, false);
        }
    }
    Ok(())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_commit(txn: *mut MDB_txn) -> c_int {
    boundary(|| {
        let pointer = unsafe { get_ref(txn)? };
        if !pointer.correct_thread() || pointer.state.is_none() {
            return Err(BAD_TXN);
        }
        pointer.preflight_consume()?;
        if pointer.capacity_failed {
            unsafe { mdb_txn_abort(txn) };
            return Err(BAD_TXN);
        }
        let child = pointer.child;
        drain_private_slots(unsafe { get_mut(txn)? })?;
        if !child.is_null() {
            let result = unsafe { mdb_txn_commit(child) };
            if result != 0 {
                unsafe { mdb_txn_abort(txn) };
                return Err(result);
            }
        }
        let dirty = unsafe { get_ref(txn)? }.publication_dirty()?;
        let flush = if matches!(unsafe { get_ref(txn)? }.state, Some(State::Prepared(_))) {
            Ok(())
        } else {
            let current = unsafe { get_mut(txn)? };
            current.finish_reserves().and_then(|_| {
                if current.parent.is_null() && !current.readonly && dirty {
                    current.finalize_alias_roots()?;
                    current.admit_map_capacity()
                } else {
                    Ok(())
                }
            })
        };
        if let Err(error) = flush {
            unsafe { mdb_txn_abort(txn) };
            return Err(error);
        }
        let mut txn = unsafe { Box::from_raw(txn) };
        invalidate_cursors(&mut txn);
        if matches!(txn.state, Some(State::Read(_))) {
            reader_slots::release(&mut txn, true);
        }
        let abandoned_candidate = matches!(&txn.state, Some(State::Prepared(_)))
            || (dirty && matches!(&txn.state, Some(State::Write(_))));
        let mut abandoned = AbandonedPreparation::new(&txn.shared, abandoned_candidate);
        let mut prepared = None;
        let mut result = match txn.state.take() {
            Some(State::Write(w)) if dirty => match write_map::sample(w.take(), &txn.shared)
                .and_then(|writer| {
                    writer.prepare_for_commit_with_data_policy(
                        CommitIntent::ForcePublication,
                        txn.shared.current_durability(),
                    )
                }) {
                Ok(owner) => {
                    prepared = Some(owner);
                    Ok(())
                }
                Err(error) => Err(code(error)),
            },
            Some(State::Write(w)) => {
                // Automatic native MAIN creation is adapter setup, not an
                // LMDB write. A clean facade writer must not publish it.
                w.take().abort();
                Ok(())
            }
            Some(State::Prepared(owner)) => {
                prepared = Some(owner.transaction);
                Ok(())
            }
            Some(State::Child(w)) => w.take().release().map_err(code),
            Some(State::Read(_)) => Ok(()),
            Some(State::PrivateRead(view)) => {
                drop(view);
                Ok(())
            }
            None => Err(BAD_TXN),
        };
        let publishing = prepared.is_some();
        if result.is_ok()
            && let Some(prepared) = prepared
        {
            result = (|| -> FfiResult<()> {
                let owner = txn.writer_environment.as_ref().ok_or(BAD_TXN)?;
                // All sealing/admission above retained no Visibility lease.
                // Native final selection takes History internally and never
                // acquires Capture/registry; LOCK survives the record update.
                let lease = txn
                    .shared
                    .visibility
                    .as_ref()
                    .map(|v| v.lease(true))
                    .transpose()?;
                // Validate the bounded prior record before selecting native D.
                // Known damage must abort preparation without publishing; only
                // damage/failure after selection makes the outcome uncertain.
                if let (Some(view), Some(lease)) = (&txn.shared.visibility, &lease) {
                    view.read(lease)?;
                }
                prepared
                    .commit_with_metadata_policy(txn.shared.current_durability())
                    .map_err(code)?;
                abandoned.disarm();
                previous::publish_record(&txn.shared, owner, &txn.write_role, lease.as_ref())
                    .map_err(|_| -30795)?;
                if let Some(used) = txn.quota_used {
                    // owner retains native LOCK plus our common writer lease;
                    // this clones its just-published current, not a peer head.
                    map_size::published(&txn.shared, &owner.begin_read(), used);
                }
                Ok(())
            })();
        }
        if result.is_ok()
            && !txn.readonly
            && txn.parent.is_null()
            && map_size::publish(&txn.shared.directory, unsafe { (*txn.env).mapsize }).is_err()
        {
            // The engine commit is already published. A capacity-metadata
            // failure makes the compound outcome uncertain until reopen.
            result = Err(-30795);
        }
        if result.is_ok() && publishing {
            // Visibility/History are released before ENV-GATE downgrade.
            if previous::finish_first(&txn.shared, &txn.write_role).is_err() {
                result = Err(-30795);
            }
        }
        if result == Err(-30795) {
            txn.shared
                .poisoned
                .store(true, std::sync::atomic::Ordering::Release);
        }
        // Retire provisional capabilities before a new engine owner can reuse
        // an aborted catalog id. Keep the writer lease through this bookkeeping.
        finish_handles(&mut txn, result.is_ok());
        detach(&mut txn);
        // Notify before either writer owner/fence can admit the next writer.
        drop(abandoned);
        // Release engine owner before the cross-process writer lease.
        txn.writer_environment.take();
        txn.writer_lease.take();
        txn.reader_lease.take();
        result
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_prepare(txn: *mut MDB_txn) -> c_int {
    boundary(|| {
        let current = unsafe { get_ref(txn)? };
        if !current.correct_thread() || current.state.is_none() {
            return Err(BAD_TXN);
        }
        if matches!(current.state, Some(State::Prepared(_))) {
            return Err(TXN_PENDING);
        }
        // Keep G02's transitive immutable-reader preflight before consuming or
        // flushing any exclusive descendant, including implicit child commits.
        current.preflight_consume()?;
        if current.capacity_failed {
            unsafe { mdb_txn_abort(txn) };
            return Err(BAD_TXN);
        }
        let child = current.child;
        drain_private_slots(unsafe { get_mut(txn)? })?;
        if !child.is_null() {
            let result = unsafe { mdb_txn_commit(child) };
            if result != 0 {
                unsafe { mdb_txn_abort(txn) };
                return Err(result);
            }
        }
        let current = unsafe { get_ref(txn)? };
        if current.readonly || !current.parent.is_null() || !current.publication_dirty()? {
            // Upstream consumes readonly/clean writers and merges a nested
            // writer before reaching its top-level prepare boundary.
            return match unsafe { mdb_txn_commit(txn) } {
                0 => Ok(()),
                error => Err(error),
            };
        }
        if let Err(error) = unsafe { get_mut(txn)? }.finish_reserves() {
            unsafe { mdb_txn_abort(txn) };
            return Err(error);
        }
        let final_capacity = unsafe { get_mut(txn)? };
        if let Err(error) = final_capacity
            .finalize_alias_roots()
            .and_then(|_| final_capacity.admit_map_capacity())
        {
            unsafe { mdb_txn_abort(txn) };
            return Err(error);
        }
        let metadata = match unsafe { get_ref(txn)? }.capture_prepared_metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                unsafe { mdb_txn_abort(txn) };
                return Err(error);
            }
        };
        // Own the C allocation while the native writer is consumed. This also
        // releases its outer lease if native preparation unwinds unexpectedly.
        let mut current = unsafe { Box::from_raw(txn) };
        invalidate_cursors(&mut current);
        let state = current.state.take().ok_or(BAD_TXN)?;
        let mut abandoned = AbandonedPreparation::new(&current.shared, true);
        let result = match catch_unwind(AssertUnwindSafe(|| match state {
            State::Write(writer) => {
                let writer = write_map::sample(writer.take(), &current.shared).map_err(code)?;
                #[cfg(test)]
                PANIC_DURING_PREPARE.with(|control| {
                    assert!(!control.replace(false), "injected native prepare unwind");
                });
                writer
                    .prepare_with_data_policy(
                        CommitIntent::ForcePublication,
                        current.shared.current_durability(),
                    )
                    .map_err(code)
            }
            _ => unreachable!("only a dirty outer writer reaches native preparation"),
        })) {
            Ok(result) => result,
            Err(_) => Err(-30795),
        };
        match result {
            Ok(prepared) => {
                abandoned.disarm();
                current.state = Some(State::Prepared(prepared_metadata::PreparedOwner {
                    transaction: prepared,
                    metadata,
                }));
                // Box conversion preserves the caller's original handle address.
                let _ = Box::into_raw(current);
                Ok(())
            }
            Err(error) => {
                // Native preparation consumed the writer without selecting its
                // candidate. Consume the C handle and retire provisional DBIs.
                if error == -30795 {
                    current
                        .shared
                        .poisoned
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                finish_handles(&mut current, false);
                detach(&mut current);
                drop(abandoned);
                current.writer_environment.take();
                current.writer_lease.take();
                current.reader_lease.take();
                Err(error)
            }
        }
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_abort(txn: *mut MDB_txn) {
    if txn.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !unsafe { (*txn).correct_thread() } {
            return;
        }
        if unsafe { (*txn).preflight_consume() }.is_err()
            || drain_private_slots(unsafe { &mut *txn }).is_err()
        {
            return;
        }
        if !unsafe { (*txn).child }.is_null() {
            unsafe { mdb_txn_abort((*txn).child) };
            if !unsafe { (*txn).child }.is_null() {
                // A failed cleanup preflight must preserve every ancestor.
                return;
            }
        }
        let mut txn = unsafe { Box::from_raw(txn) };
        invalidate_cursors(&mut txn);
        if matches!(txn.state, Some(State::Read(_))) {
            reader_slots::release(&mut txn, true);
        }
        let abandoned =
            AbandonedPreparation::new(&txn.shared, matches!(&txn.state, Some(State::Prepared(_))));
        txn.state.take();
        finish_handles(&mut txn, false);
        detach(&mut txn);
        drop(abandoned);
        txn.writer_environment.take();
        txn.writer_lease.take();
        txn.reader_lease.take();
    }));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_reset(txn: *mut MDB_txn) {
    if let Some(txn) = unsafe { txn.as_mut() }
        && txn.live().is_ok()
        && txn.readonly
        && txn.parent.is_null()
    {
        txn.state.take();
        reader_slots::release(txn, false);
        finish_handles(txn, false);
        txn.created_ids.clear();
        txn.dropped_dbis.clear();
        txn.closed_dbis.clear();
        txn.activated_dbis.clear();
        txn.output.clear();
        txn.retired_rows.clear();
        if let Ok(index) = txn.catalog_index.get_mut() {
            *index = None;
        }
        if let Ok(mut dbis) = txn.dbis.lock() {
            *dbis = Default::default();
        }
        txn.changed();
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_renew(txn: *mut MDB_txn) -> c_int {
    boundary(|| {
        let t = unsafe { get_mut(txn)? };
        if !t.readonly || t.state.is_some() || !t.correct_thread() {
            return Err(libc::EINVAL);
        }
        if t.shared.poisoned.load(std::sync::atomic::Ordering::Acquire) {
            return Err(-30795);
        }
        let _opening_transaction = rollback::OpeningTransaction::new(t.shared.clone())?;
        let read =
            previous::transaction_snapshot(&t.shared, unsafe { get_ref(t.env)? }.flags(), true)?;
        let setup = reader_slots::setup(
            &t.shared,
            t.notls,
            unsafe { get_ref(t.env)? }.flags() & 0x400000 != 0,
            read.sequence(),
            &mut t.reader_lease,
        )?;
        map_size::check(&t.shared, &read, unsafe { get_ref(t.env)? }.mapsize)?;
        *t.dbis.lock().map_err(|_| BAD_TXN)? = dbi_state::TxnDbis::from_shared(&t.shared)?;
        setup.finish();
        t.txnid = read.sequence() as usize;
        t.state = Some(State::Read(read));
        t.changed();
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_env(txn: *mut MDB_txn) -> *mut MDB_env {
    unsafe { txn.as_ref() }.map_or(ptr::null_mut(), |t| t.env)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_id(txn: *mut MDB_txn) -> usize {
    unsafe { txn.as_ref() }.map_or(0, |txn| txn.txnid)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_txn_flags(txn: *mut MDB_txn, flags: *mut u32) -> c_int {
    boundary(|| {
        let txn = unsafe { get_ref(txn)? };
        let flags = unsafe { get_mut(flags)? };
        // LMDB exposes only the public read-only bit, including on reset
        // readers and parents suspended by an active child transaction.
        *flags = if txn.readonly { RDONLY } else { 0 };
        Ok(())
    })
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use std::ffi::CString;

    struct Fixture {
        env: *mut MDB_env,
        path: std::path::PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            Self::with_flags(0)
        }
        fn with_flags(flags: u32) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("ulmdb-c-registry-{}-{nonce}", std::process::id()));
            let name = CString::new(path.to_str().unwrap()).unwrap();
            let mut env = ptr::null_mut();
            unsafe {
                assert_eq!(environment::mdb_env_create(&mut env), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(env, 4), 0);
                assert_eq!(
                    environment::mdb_env_open(env, name.as_ptr(), flags, 0o600),
                    0
                );
            }
            Self { env, path }
        }
        fn begin(&self) -> *mut MDB_txn {
            let mut txn = ptr::null_mut();
            assert_eq!(
                unsafe { mdb_txn_begin(self.env, ptr::null_mut(), 0, &mut txn) },
                0
            );
            txn
        }
        fn registry_len(&self) -> usize {
            unsafe { &*self.env }
                .shared
                .as_ref()
                .unwrap()
                .handles
                .lock()
                .unwrap()
                .len()
        }
        fn active_handles(&self) -> usize {
            unsafe { &*self.env }
                .shared
                .as_ref()
                .unwrap()
                .handles
                .lock()
                .unwrap()
                .iter()
                .flatten()
                .filter(|handle| handle.name() != MAIN)
                .count()
        }
        fn option_count(&self) -> usize {
            unsafe { &*self.env }
                .shared
                .as_ref()
                .unwrap()
                .options
                .lock()
                .unwrap()
                .len()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe { environment::mdb_env_close(self.env) };
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
    fn open(txn: *mut MDB_txn, name: &std::ffi::CStr) -> u32 {
        let mut dbi = u32::MAX;
        assert_eq!(
            unsafe { operations::mdb_dbi_open(txn, name.as_ptr(), CREATE, &mut dbi) },
            0
        );
        dbi
    }
    fn modern_fixture() -> Fixture {
        modern_fixture_with_flags(0)
    }
    fn modern_fixture_with_flags(flags: u32) -> Fixture {
        let fixture = Fixture::with_flags(flags);
        let env = unsafe { &mut *fixture.env };
        Arc::get_mut(env.shared.as_mut().unwrap())
            .unwrap()
            .catalog_terminator = true;
        fixture
    }
    fn readonly_child(env: *mut MDB_env, parent: *mut MDB_txn) -> *mut MDB_txn {
        let mut child = ptr::null_mut();
        assert_eq!(unsafe { mdb_txn_begin(env, parent, RDONLY, &mut child) }, 0);
        child
    }
    fn put_value(txn: *mut MDB_txn, dbi: u32, contents: &[u8]) {
        let mut key = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: contents.len(),
            mv_data: contents.as_ptr().cast_mut().cast(),
        };
        assert_eq!(
            unsafe { operations::mdb_put(txn, dbi, &mut key, &mut value, 0) },
            0
        );
    }
    fn value(txn: *mut MDB_txn, dbi: u32) -> MDB_val {
        let mut key = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: 0,
            mv_data: ptr::null_mut(),
        };
        assert_eq!(
            unsafe { operations::mdb_get(txn, dbi, &mut key, &mut value) },
            0
        );
        value
    }
    #[cfg(target_os = "linux")]
    fn check_abandoned_preparations_rearm_orphan_scan(count: u8) {
        fn assets(root: &std::path::Path) -> std::collections::BTreeSet<std::path::PathBuf> {
            ["segments", "cohorts"]
                .into_iter()
                .flat_map(|name| {
                    std::fs::read_dir(root.join(name))
                        .into_iter()
                        .flatten()
                        .map(|e| e.unwrap().path())
                        .filter(|p| p.is_file())
                })
                .collect()
        }
        let fixture = Fixture::with_flags(0);
        assert_eq!(
            unsafe { environment::mdb_env_set_mapsize(fixture.env, 64 << 20) },
            0
        );
        let txn = fixture.begin();
        let dbi = open(txn, c"orphan-preparation");
        put_value(txn, dbi, b"seed");
        assert_eq!(unsafe { mdb_txn_commit(txn) }, 0);
        // Drain first-open discovery, then stay below the128-segment/64MiB
        // normal admission thresholds. Every later dbi use is already exported.
        for _ in 0..24 {
            let txn = fixture.begin();
            put_value(txn, dbi, b"seed");
            assert_eq!(unsafe { mdb_txn_commit(txn) }, 0);
        }
        let mut old = ptr::null_mut();
        assert_eq!(
            unsafe { mdb_txn_begin(fixture.env, ptr::null_mut(), RDONLY, &mut old) },
            0
        );
        let borrowed = value(old, dbi);
        let mut large = vec![0x7bu8; (2 << 20) + 17];
        let mut abandoned = std::collections::BTreeSet::new();
        for serial in 0..count {
            // Distinct payloads prevent content-addressed sealing from reusing
            // an already abandoned immutable asset from the previous round.
            large[0] = serial;
            let before = assets(&fixture.path);
            let txn = fixture.begin();
            put_value(txn, dbi, &large);
            assert_eq!(unsafe { mdb_txn_prepare(txn) }, 0);
            let added: Vec<_> = assets(&fixture.path).difference(&before).cloned().collect();
            assert!(!added.is_empty(), "preparation must really seal assets");
            assert!(
                added
                    .iter()
                    .map(|p| std::fs::metadata(p).unwrap().len())
                    .sum::<u64>()
                    > (2 << 20)
            );
            abandoned.extend(added);
            unsafe { mdb_txn_abort(txn) };
        }
        assert!(
            abandoned.iter().all(|p| p.exists()),
            "abort only requests later cleanup"
        );
        for _ in 0..64 {
            let txn = fixture.begin();
            put_value(txn, dbi, b"small");
            assert_eq!(unsafe { mdb_txn_commit(txn) }, 0);
        }
        let remaining: Vec<_> = abandoned.iter().filter(|p| p.exists()).collect();
        assert!(
            remaining.is_empty(),
            "abandoned sealed assets remain: {remaining:?}"
        );
        assert_eq!(
            unsafe { std::slice::from_raw_parts(borrowed.mv_data.cast::<u8>(), borrowed.mv_size) },
            b"seed"
        );
        unsafe { mdb_txn_abort(old) };
        let txn = fixture.begin();
        let current = value(txn, dbi);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(current.mv_data.cast::<u8>(), current.mv_size) },
            b"small"
        );
        unsafe { mdb_txn_abort(txn) };
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn abandoned_preparations_rearm_orphan_scan_below_rewrite_threshold() {
        check_abandoned_preparations_rearm_orphan_scan(3);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn single_abandoned_preparation_rearms_a_restored_idle_scheduler() {
        check_abandoned_preparations_rearm_orphan_scan(1);
    }

    #[test]
    fn prepare_panic_cleanup_worker() {
        if std::env::var_os("ULMDB_C_PREPARE_PANIC_WORKER").is_none() {
            return;
        }
        let fixture = modern_fixture();
        let txn = fixture.begin();
        let provisional = open(txn, c"provisional");
        put_value(txn, provisional, b"private");
        PANIC_DURING_PREPARE.with(|control| control.set(true));
        assert_eq!(unsafe { mdb_txn_prepare(txn) }, -30795);
        // The failed handle was consumed. Do not reuse or abort its pointer.
        assert_eq!(fixture.active_handles(), 0);
        let mut blocked = ptr::null_mut();
        assert_eq!(
            unsafe { mdb_txn_begin(fixture.env, ptr::null_mut(), 0, &mut blocked) },
            -30795
        );
        assert!(blocked.is_null());
        // A fresh owner proves that the native guard and outer process lease
        // were released even though the original Shared remains poisoned.
        let path = CString::new(fixture.path.to_str().unwrap()).unwrap();
        let mut fresh = ptr::null_mut();
        assert_eq!(unsafe { environment::mdb_env_create(&mut fresh) }, 0);
        assert_eq!(unsafe { environment::mdb_env_set_maxdbs(fresh, 8) }, 0);
        assert_eq!(
            unsafe { environment::mdb_env_open(fresh, path.as_ptr(), 0, 0o600) },
            0
        );
        let mut retry = ptr::null_mut();
        assert_eq!(
            unsafe { mdb_txn_begin(fresh, ptr::null_mut(), 0, &mut retry) },
            0
        );
        let recovered = open(retry, c"provisional");
        put_value(retry, recovered, b"retry");
        assert_eq!(unsafe { mdb_txn_commit(retry) }, 0);
        unsafe { environment::mdb_env_close(fresh) };
    }
    #[test]
    fn prepare_panic_consumes_handle_and_releases_provisional_ownership() {
        let mut process = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "transaction::registry_tests::prepare_panic_cleanup_worker",
                "--nocapture",
            ])
            .env("ULMDB_C_PREPARE_PANIC_WORKER", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while process.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                process.kill().unwrap();
                let output = process.wait_with_output().unwrap();
                panic!("prepare unwind leaked writer ownership: {output:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let output = process.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    #[test]
    fn readonly_descendants_preserve_every_ancestor_on_invalid_consumption() {
        // Parent commit/abort while readonly children exist is forbidden by
        // LMDB. Test the adapter's ownership guard without using that sequence
        // as a differential upstream oracle.
        let fixture = modern_fixture();
        let parent = fixture.begin();
        let dbi = open(parent, c"values");
        put_value(parent, dbi, b"outer");
        let mut nested = ptr::null_mut();
        assert_eq!(
            unsafe { mdb_txn_begin(fixture.env, parent, 0, &mut nested) },
            0
        );
        put_value(nested, dbi, b"inner");
        let first = readonly_child(fixture.env, nested);
        let second = readonly_child(fixture.env, nested);
        let pinned = value(first, dbi);
        assert_eq!(unsafe { mdb_txn_prepare(parent) }, BAD_TXN);
        assert_eq!(unsafe { mdb_txn_prepare(nested) }, BAD_TXN);
        assert_eq!(unsafe { mdb_txn_commit(parent) }, BAD_TXN);
        assert_eq!(unsafe { mdb_txn_commit(nested) }, BAD_TXN);
        unsafe {
            mdb_txn_abort(parent);
            mdb_txn_abort(nested);
        }
        assert_eq!(
            unsafe { std::slice::from_raw_parts(pinned.mv_data.cast::<u8>(), pinned.mv_size) },
            b"inner"
        );
        assert_eq!(value(second, dbi).mv_data, pinned.mv_data);
        unsafe { mdb_txn_abort(first) };
        assert_eq!(unsafe { mdb_txn_prepare(parent) }, BAD_TXN);
        assert_eq!(unsafe { mdb_txn_commit(parent) }, BAD_TXN);
        unsafe {
            mdb_txn_abort(second);
            mdb_txn_abort(nested);
        }
        let outer = value(parent, dbi);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(outer.mv_data.cast::<u8>(), outer.mv_size) },
            b"outer"
        );
        assert_eq!(unsafe { mdb_txn_commit(parent) }, 0);
    }
    #[test]
    fn readonly_child_dbi_reopens_keep_provisional_cleanup_on_the_writer_thread() {
        // Opening DBIs from overlapping transactions exceeds LMDB's documented
        // dbi_open contract. The adapter must nevertheless keep its ownership
        // bookkeeping safe and clear aliases on the original writer's abort.
        let fixture = modern_fixture_with_flags(0x200000);
        let parent = fixture.begin();
        let first = open(parent, c"first");
        unsafe { operations::mdb_dbi_close(fixture.env, first) };
        let second = open(parent, c"second");
        let child = readonly_child(fixture.env, parent);
        let sibling = readonly_child(fixture.env, parent);
        let child_address = child as usize;
        let alias = std::thread::spawn(move || {
            let child = child_address as *mut MDB_txn;
            let mut alias = u32::MAX;
            assert_eq!(
                unsafe { operations::mdb_dbi_open(child, c"first".as_ptr(), 0, &mut alias) },
                0
            );
            unsafe { mdb_txn_abort(child) };
            alias
        })
        .join()
        .unwrap();
        assert_ne!(alias, first);
        let sibling_address = sibling as usize;
        let reopened_second = std::thread::spawn(move || {
            let sibling = sibling_address as *mut MDB_txn;
            let mut handle = u32::MAX;
            assert_eq!(
                unsafe { operations::mdb_dbi_open(sibling, c"second".as_ptr(), 0, &mut handle) },
                0
            );
            unsafe { mdb_txn_abort(sibling) };
            handle
        })
        .join()
        .unwrap();
        assert_eq!(reopened_second, second);
        assert_eq!(fixture.active_handles(), 2);
        unsafe { mdb_txn_abort(parent) };
        assert_eq!(fixture.active_handles(), 0);
    }
    #[test]
    fn readonly_views_prevent_capacity_failure_from_consuming_the_writer() {
        let fixture = modern_fixture();
        let parent = fixture.begin();
        let dbi = open(parent, c"values");
        put_value(parent, dbi, b"private");
        let child = readonly_child(fixture.env, parent);
        // Only unrelated adapter metadata changes; no native writer borrow.
        unsafe { (*parent).capacity_failed = true };
        assert_eq!(unsafe { mdb_txn_prepare(parent) }, BAD_TXN);
        assert_eq!(unsafe { mdb_txn_commit(parent) }, BAD_TXN);
        let pinned = value(child, dbi);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(pinned.mv_data.cast::<u8>(), pinned.mv_size) },
            b"private"
        );
        unsafe { mdb_txn_abort(child) };
        assert_eq!(unsafe { mdb_txn_commit(parent) }, BAD_TXN);
        assert_eq!(fixture.active_handles(), 0);
    }
    #[test]
    fn abort_and_committed_drop_reuse_registry_slots() {
        let fixture = Fixture::new();
        for _ in 0..64 {
            let txn = fixture.begin();
            open(txn, c"temporary");
            unsafe { mdb_txn_abort(txn) };
            assert_eq!(fixture.active_handles(), 0);
            assert_eq!(fixture.registry_len(), 3);
        }
        for _ in 0..32 {
            let txn = fixture.begin();
            let dbi = open(txn, c"temporary");
            assert_eq!(unsafe { mdb_txn_commit(txn) }, 0);
            let txn = fixture.begin();
            assert_eq!(unsafe { operations::mdb_drop(txn, dbi, 1) }, 0);
            assert_eq!(unsafe { mdb_txn_commit(txn) }, 0);
            assert_eq!(fixture.active_handles(), 0);
            assert_eq!(fixture.registry_len(), 3);
        }
    }
    #[test]
    fn close_and_reopen_provisional_database_remains_owned_by_creator() {
        let fixture = Fixture::new();
        for _ in 0..32 {
            let txn = fixture.begin();
            let first = open(txn, c"first");
            unsafe { operations::mdb_dbi_close(fixture.env, first) };
            open(txn, c"second");
            // A new slot for the same provisional catalog identity must also
            // be cleared on abort, even though this open did not create it.
            open(txn, c"first");
            unsafe { mdb_txn_abort(txn) };
            assert_eq!(fixture.active_handles(), 0);
            assert_eq!(fixture.registry_len(), 4);
        }
    }
    unsafe extern "C" fn compare_bytes(a: *const MDB_val, b: *const MDB_val) -> c_int {
        match (unsafe { bytes(a) }, unsafe { bytes(b) }) {
            (Ok(a), Ok(b)) => a.cmp(b) as c_int,
            _ => 0,
        }
    }
    #[test]
    fn aborted_and_dropped_database_callbacks_release_their_options() {
        let fixture = Fixture::new();
        for commit_first in [false, true] {
            let txn = fixture.begin();
            let dbi = open(txn, c"callbacks");
            assert_eq!(
                unsafe { operations::mdb_set_compare(txn, dbi, compare_bytes as *mut c_void) },
                0
            );
            assert_eq!(fixture.option_count(), 1);
            if commit_first {
                assert_eq!(unsafe { mdb_txn_commit(txn) }, 0);
                let txn = fixture.begin();
                assert_eq!(unsafe { operations::mdb_drop(txn, dbi, 1) }, 0);
                assert_eq!(unsafe { mdb_txn_commit(txn) }, 0);
            } else {
                unsafe { mdb_txn_abort(txn) };
            }
            assert_eq!(fixture.option_count(), 0);
        }
    }
    #[test]
    fn concurrent_aborted_creations_never_reuse_live_capabilities() {
        let fixture = Fixture::new();
        let address = fixture.env as usize;
        std::thread::scope(|scope| {
            for worker in 0..2 {
                scope.spawn(move || {
                    let name = CString::new(format!("aborted-{worker}")).unwrap();
                    for _ in 0..64 {
                        let env = address as *mut MDB_env;
                        let mut txn = ptr::null_mut();
                        assert_eq!(
                            unsafe { mdb_txn_begin(env, ptr::null_mut(), 0, &mut txn) },
                            0
                        );
                        let dbi = open(txn, &name);
                        let key = MDB_val {
                            mv_size: 1,
                            mv_data: c"k".as_ptr().cast_mut().cast(),
                        };
                        let value = MDB_val {
                            mv_size: 1,
                            mv_data: c"v".as_ptr().cast_mut().cast(),
                        };
                        let mut key = key;
                        let mut value = value;
                        assert_eq!(
                            unsafe { operations::mdb_put(txn, dbi, &mut key, &mut value, 0) },
                            0
                        );
                        unsafe { mdb_txn_abort(txn) };
                    }
                });
            }
        });
        assert_eq!(fixture.active_handles(), 0);
    }
}
