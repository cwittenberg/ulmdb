//! Rollback does not consume reader handles. Immutable snapshots retain their
//! native pins; process aliases remain exported exactly as before selection.
use super::*;
use std::sync::atomic::Ordering;

/// Captures every allocation, including an aborted writer's allocations,
/// before its short-lived native owner releases LOCK. Drop also covers unwind.
pub(super) struct WriterEnvironment {
    engine: WriterEngine,
    shared: Arc<Shared>,
    // Keep the old C compound D/P/view roots live across Native cleanup and
    // the subsequent visibility-record update. These own no Capture gate.
    _publication_roots: maintenance::PublicationProtection,
}
enum WriterEngine {
    Current(Environment),
    Previous(Arc<ulmdb::PreviousSession>),
}
impl WriterEnvironment {
    pub(super) fn open(shared: Arc<Shared>) -> FfiResult<Self> {
        Self::open_inner(shared, true)
    }
    /// Configuration must be able to disable/replace an obsolete prior policy.
    pub(super) fn open_for_configuration(shared: Arc<Shared>) -> FfiResult<Self> {
        Self::open_inner(shared, false)
    }
    fn open_inner(shared: Arc<Shared>, resume: bool) -> FfiResult<Self> {
        let engine = if let Some(external) = &shared.external {
            external.open_environment_with_cache_options(
                &shared.directory,
                ulmdb::ResidencyPolicy::Demand,
                shared.read_ahead(),
                shared.transform.clone(),
                shared.cache.clone(),
            )
        } else {
            Environment::open_with_cache_options(
                &shared.directory,
                ulmdb::ResidencyPolicy::Demand,
                shared.read_ahead(),
                shared.transform.clone(),
                shared.cache.clone(),
            )
        }
        .map_err(code)?;
        // C publication has its own D/P/view protection and transferable policy.
        // An ordinary Native default must not bypass an explicit C disable.
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if shared.external.is_none() && !engine.is_replica() {
            engine.set_maintenance_policy(ulmdb::MaintenancePolicy::Disabled).map_err(code)?;
        }
        fixed_map::owner(&shared, &engine)?;
        engine
            .raise_database_id_floor(shared.database_id_floor.load(Ordering::Acquire))
            .map_err(code)?;
        let publication_roots = if resume {
            maintenance::resume(&shared, &engine)?
        } else {
            Default::default()
        };
        Ok(Self {
            engine: WriterEngine::Current(engine),
            shared,
            _publication_roots: publication_roots,
        })
    }
    pub(super) fn previous(
        shared: Arc<Shared>,
        session: Arc<ulmdb::PreviousSession>,
    ) -> FfiResult<Self> {
        fixed_map::previous_owner(&shared, &session)?;
        session
            .raise_database_id_floor(shared.database_id_floor.load(Ordering::Acquire))
            .map_err(code)?;
        Ok(Self {
            engine: WriterEngine::Previous(session),
            shared,
            // Pending/offset PreviousSession never enables this facade's
            // automatic policy; its existing ReadTxn pins retain the A view.
            _publication_roots: Default::default(),
        })
    }
    pub(super) fn begin_write(&self) -> ulmdb::Result<WriteTxn<'_>> {
        match &self.engine {
            WriterEngine::Current(e) => e.begin_write(),
            WriterEngine::Previous(e) => e.begin_write(),
        }
    }
    pub(super) fn begin_read(&self) -> ReadTxn {
        match &self.engine {
            WriterEngine::Current(e) => e.begin_read(),
            WriterEngine::Previous(e) => e.begin_read(),
        }
    }
    pub(super) fn stats(&self) -> ulmdb::Stats {
        self.begin_read().stats()
    }
    pub(super) fn selected_checkpoint(&self) -> ulmdb::Result<[u8; 64]> {
        match &self.engine {
            WriterEngine::Current(e) => e.file_checkpoint(),
            WriterEngine::Previous(e) => e.selected_checkpoint(),
        }
    }
    pub(super) fn database_id_floor(&self) -> u64 {
        match &self.engine {
            WriterEngine::Current(e) => e.database_id_floor(),
            WriterEngine::Previous(e) => e.database_id_floor(),
        }
    }
    pub(super) fn set_maintenance_policy(&self, policy: ulmdb::MaintenancePolicy, fallback: bool) -> FfiResult<()> {
        match &self.engine {
            WriterEngine::Current(e) => if fallback {
                e.set_maintenance_policy_with_fallback(policy).map_err(code)
            } else { e.set_maintenance_policy(policy).map_err(code) },
            WriterEngine::Previous(_) => Err(libc::EBUSY),
        }
    }
    pub(super) fn load_incremental_checked(
        &self,
        input: &mut impl std::io::Read,
        check: impl FnOnce(&ReadTxn) -> ulmdb::Result<()>,
    ) -> ulmdb::Result<ulmdb::backup::IncrementalReport> {
        match &self.engine {
            WriterEngine::Current(e) => {
                e.load_incremental_checked(input, Default::default(), check)
            }
            WriterEngine::Previous(_) => Err(Error::Busy),
        }
    }
    pub(super) fn rollback_last_commit_with_metadata_policy(
        &self,
        sequence: u64,
        policy: ulmdb::Durability,
    ) -> Result<ulmdb::RollbackReport, ulmdb::RollbackError> {
        match &self.engine {
            WriterEngine::Current(e) => {
                e.rollback_last_commit_with_metadata_policy(sequence, policy)
            }
            WriterEngine::Previous(_) => Err(ulmdb::RollbackError::Engine(Error::Busy)),
        }
    }
}
impl Drop for WriterEnvironment {
    fn drop(&mut self) {
        if self.shared.creator != std::process::id() {
            return;
        }
        self.shared
            .database_id_floor
            .fetch_max(self.database_id_floor(), Ordering::AcqRel);
        if let WriterEngine::Current(engine) = &self.engine {
            maintenance::suspend(&self.shared, engine);
        }
    }
}

pub(super) struct OpeningTransaction(Arc<Shared>);
impl OpeningTransaction {
    /// Begin setup only while idle, then release lifecycle before any IO,
    /// callbacks or writer admission. Ordinary subsequent admissions remain
    /// counted, so rollback/configuration observes this in-flight operation.
    pub(super) fn if_idle(shared: Arc<Shared>) -> FfiResult<Self> {
        shared.check_process()?;
        {
            let mut count = shared.lifecycle.try_lock().map_err(|_| libc::EBUSY)?;
            if *count != 0 {
                return Err(libc::EBUSY);
            }
            *count = 1;
        }
        Ok(Self(shared))
    }
    pub(super) fn new(shared: Arc<Shared>) -> FfiResult<Self> {
        shared.check_process()?;
        {
            let mut count = shared.lifecycle.lock().map_err(|_| BAD_TXN)?;
            *count = count.checked_add(1).ok_or(-30792)?;
        }
        Ok(Self(shared))
    }
}
impl Drop for OpeningTransaction {
    fn drop(&mut self) {
        if self.0.creator != std::process::id() {
            return;
        }
        if let Ok(mut count) = self.0.lifecycle.lock() {
            *count -= 1;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_rollback(env: *mut MDB_env, txnid: usize) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        let shared = env.shared.as_ref().ok_or(libc::EINVAL)?.clone();
        shared.check_process()?;
        if shared.poisoned.load(Ordering::Acquire) {
            return Err(-30795);
        }
        if env.flags() & RDONLY != 0 {
            return Err(shared.readonly_error());
        }
        // Rollback's temporary lease assumes a normal SH environment. Pending
        // PREV owns retained ENV EX plus a native session; attempting a second
        // native owner would fail and must not downgrade that original lease.
        if shared.previous.pending() {
            return Err(libc::EBUSY);
        }
        // Count snapshot/slot admission and writers waiting for their lease.
        // Rollback must not overlap either setup or wait behind its own caller.
        let lifecycle = shared.lifecycle.try_lock().map_err(|_| libc::EBUSY)?;
        if *lifecycle != 0 {
            return Err(libc::EBUSY);
        }
        if shared.writer_thread.lock().map_err(|_| BAD_TXN)?.is_some() {
            return Err(libc::EBUSY);
        }
        let _exclusive = if shared.external.is_some() {
            None
        } else {
            Some(process::RollbackLease::acquire(
                &shared.directory,
                shared._environment_lease.as_ref(),
            )?)
        };
        let _writer = if shared.external.is_some() {
            None
        } else {
            Some(process::WriterLease::acquire(&shared.directory)?)
        };
        let owner = WriterEnvironment::open(shared.clone())?;
        // An unexpected unwind after entering the native publication boundary
        // cannot establish a trustworthy result. Preserve the poison fence.
        let before = owner.begin_read();
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), ulmdb::RollbackError> {
            owner.rollback_last_commit_with_metadata_policy(
                txnid as u64,
                shared.current_durability(),
            )?;
            // Native rollback prepares/admit/pins internally, so it must run
            // outside Visibility. R immediately invalidates the old exact D.
            // Only canonical bookkeeping takes this short lease, under LOCK.
            let visibility = shared
                .visibility
                .as_ref()
                .map(|v| v.lease(true))
                .transpose()
                .map_err(|_| ulmdb::RollbackError::Engine(Error::OutcomeUnknown))?;
            previous::publish_record(
                &shared,
                &owner,
                &previous::WriteRole::canonical(),
                visibility.as_ref(),
            )
            .map_err(|_| ulmdb::RollbackError::Engine(Error::OutcomeUnknown))?;
            drop(visibility);
            let restored = owner.begin_read();
            let handles = shared
                .handles
                .lock()
                .map_err(|_| ulmdb::RollbackError::Engine(Error::OutcomeUnknown))?;
            let mut invalid = shared
                .rollback_invalid
                .lock()
                .map_err(|_| ulmdb::RollbackError::Engine(Error::OutcomeUnknown))?;
            // Exported aliases survive rollback, but lazy data loading of a
            // catalog record created only by the undone head returns BAD_DBI.
            // Rebuild from current slots so history never grows without bound.
            *invalid = handles
                .iter()
                .flatten()
                .filter(|handle| {
                    handle.name() != MAIN
                        && restored.open_database(handle.native().name()).is_err()
                        && (invalid.contains(&handle.native_id())
                            || before
                                .open_database(handle.native().name())
                                .is_ok_and(|prior| NativeId::of(&prior) == handle.native_id()))
                })
                .map(AliasHandle::native_id)
                .collect();
            Ok(())
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(ulmdb::RollbackError::CannotRollback)) => Err(CANT_ROLLBACK),
            Ok(Err(ulmdb::RollbackError::Engine(error))) => {
                let error = code(error);
                if error == -30795 {
                    shared.poisoned.store(true, Ordering::Release);
                }
                Err(error)
            }
            Err(_) => {
                shared.poisoned.store(true, Ordering::Release);
                Err(-30795)
            }
        }
    })
}
