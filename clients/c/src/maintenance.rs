//! Per-C-environment policy and process-local handoff between native writers.
use super::*;

#[derive(Default)]
pub(super) struct Owner {
    policy: ulmdb::MaintenancePolicy,
    progress: Option<ulmdb::MaintenanceTransfer>,
    fallback: bool,
    startup_scan: bool,
}
impl Owner {
    pub(super) fn request_scan(&mut self) {
        if self.policy != ulmdb::MaintenancePolicy::Disabled {
            self.startup_scan = true;
        }
    }
    pub(super) fn for_open(flags: u32) -> Self {
        if cfg!(any(target_os = "linux", target_os = "macos", windows)) && flags & (RDONLY | 0x400000) == 0 {
            Self {
                policy: ulmdb::MaintenancePolicy::AUTOMATIC,
                progress: None,
                fallback: true,
                startup_scan: true,
            }
        } else {
            Self::default()
        }
    }
}
fn configure(engine: &Environment, owner: &Owner) -> ulmdb::Result<()> {
    if owner.fallback {
        engine.set_maintenance_policy_with_fallback(owner.policy)
    } else {
        engine.set_maintenance_policy(owner.policy)
    }
}
fn request_startup_scan(engine: &Environment, owner: &mut Owner) {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if owner.startup_scan && engine.request_history_maintenance().is_ok() {
        owner.startup_scan = false;
    }
}

#[derive(Default)]
pub(super) struct PublicationProtection {
    // Share the owner's already-admitted D snapshot/pin, avoiding a duplicate
    // long-lived registry slot. P/view pins are descriptor-only authorities.
    _base: Option<ReadTxn>,
    _pins: Vec<ulmdb_storage::coordination::PhysicalPin>,
}

pub(super) fn resume(shared: &Shared, engine: &Environment) -> FfiResult<PublicationProtection> {
    shared.check_process()?;
    {
        let owner = shared.maintenance.lock().map_err(|_| BAD_TXN)?;
        if owner.policy == ulmdb::MaintenancePolicy::Disabled {
            return Ok(Default::default());
        }
        let (threshold, byte_threshold) = match owner.policy {
            ulmdb::MaintenancePolicy::LegacyResumable {
                segment_threshold,
                rewrite_budget_bytes,
                ..
            }
            | ulmdb::MaintenancePolicy::CrossFormatResumable {
                segment_threshold,
                rewrite_budget_bytes,
                ..
            } => (segment_threshold, rewrite_budget_bytes.saturating_mul(4)),
            _ => (0, 0),
        };
        let pending = owner
            .progress
            .as_ref()
            .is_some_and(|p| p.has_pending_work().unwrap_or(true));
        let history_configured = shared.directory.join("HISTORY_RETENTION").try_exists().unwrap_or(true);
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let fixed_pressure = engine.reusable_fixed_map_pressure().unwrap_or(true);
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let fixed_pressure = false;
        if owner.fallback && !owner.startup_scan && !pending && !history_configured && !fixed_pressure {
            let read = engine.begin_read();
            let manifest = read.file_manifest().map_err(code)?;
            if manifest.segments.len().saturating_add(1) < threshold
                && manifest
                    .segments
                    .iter()
                    .fold(0u64, |n, e| n.saturating_add(e.length))
                    < byte_threshold
            {
                return Ok(Default::default());
            }
        }
    }
    // Old C D/P/view metadata remains authoritative until publish_record,
    // which follows Native commit and its automatic cleanup. Admit protection
    // before enabling cleanup. Pin pressure, old missing history or a temporary
    // capture conflict defers optional maintenance, never the business write.
    let base = engine.begin_read();
    let pins = match previous::publication_roots::capture(shared, &base) {
        Ok(pins) => pins,
        Err(_) => return Ok(Default::default()),
    };
    let mut owner = shared.maintenance.lock().map_err(|_| BAD_TXN)?;
    if owner.policy == ulmdb::MaintenancePolicy::Disabled {
        return Ok(Default::default());
    }
    if let Some(progress) = owner.progress.take() {
        if engine.restore_maintenance(progress).is_ok() {
            if engine.maintenance_policy().map_err(code)? == owner.policy {
                request_startup_scan(engine, &mut owner);
                return Ok(PublicationProtection {
                    _base: Some(base),
                    _pins: pins,
                });
            }
            // Never leave an unrelated restored cleanup policy active if the
            // configured policy is now ineligible and protection is released.
            engine
                .set_maintenance_policy(ulmdb::MaintenancePolicy::Disabled)
                .map_err(code)?;
        }
    }
    // A backend/policy change may make the old optional policy ineligible.
    // Explicit configuration still reports such errors; ordinary writers defer.
    if configure(engine, &owner).is_err() {
        return Ok(Default::default());
    }
    request_startup_scan(engine, &mut owner);
    Ok(PublicationProtection {
        _base: Some(base),
        _pins: pins,
    })
}

pub(super) fn suspend(shared: &Shared, engine: &Environment) {
    if shared.creator != std::process::id() || shared.external.is_some() {
        return;
    }
    if !engine
        .maintenance_policy()
        .is_ok_and(|p| p != ulmdb::MaintenancePolicy::Disabled)
    {
        return;
    }
    let Ok(mut owner) = shared.maintenance.lock() else {
        return;
    };
    if owner.policy != ulmdb::MaintenancePolicy::Disabled {
        // A poisoned owner cannot export authority. Progress is best effort;
        // a definite business commit never becomes an error during cleanup.
        owner.progress = engine.take_maintenance().ok();
    }
}

/// Configure automatic maintenance for subsequent native writer lifetimes.
/// Zero threshold disables it. NOLOCK has no hidden coordination fallback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_env_set_maintenance(
    env: *mut MDB_env,
    segment_threshold: usize,
    rewrite_budget_bytes: u64,
    reclaim_budget_files: usize,
) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        let shared = env.shared.as_ref().ok_or(libc::EINVAL)?.clone();
        shared.check_process()?;
        if shared.poisoned.load(std::sync::atomic::Ordering::Acquire) {
            return Err(-30795);
        }
        if env.flags() & RDONLY != 0 {
            return Err(shared.readonly_error());
        }
        if shared.external.is_some() || !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(libc::ENOTSUP);
        }
        if shared.previous.pending() {
            return Err(libc::EBUSY);
        }
        let _opening = rollback::OpeningTransaction::if_idle(shared.clone())?;
        if shared.writer_thread.lock().map_err(|_| BAD_TXN)?.is_some() {
            return Err(libc::EBUSY);
        }
        let policy = if segment_threshold == 0 {
            ulmdb::MaintenancePolicy::Disabled
        } else {
            ulmdb::MaintenancePolicy::CrossFormatResumable {
                segment_threshold,
                rewrite_budget_bytes,
                reclaim: ulmdb::ResumableReclaimBudget {
                    unlink_files: reclaim_budget_files,
                    ..Default::default()
                },
                history_budget_bytes: rewrite_budget_bytes.clamp(
                    256,
                    ulmdb_storage::MAX_DESCRIPTOR_BYTES
                        + ulmdb_storage::MAX_DESCRIPTOR_BYTES / 4
                        + 8320,
                ),
            }
        };
        let _writer = process::WriterLease::try_acquire(&shared.directory)?;
        let engine = rollback::WriterEnvironment::open_for_configuration(shared.clone())?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let fallback = if let Some(fixed) = &shared.fixed_map {
            fixed.supports_reuse().map_err(code)?
        } else if let Some(declaration) = &shared.fixed_declaration {
            declaration.supports_reuse().map_err(code)?
        } else {
            true
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let fallback = true;
        engine.set_maintenance_policy(policy, fallback)?;
        let mut owner = shared.maintenance.lock().map_err(|_| BAD_TXN)?;
        owner.policy = policy;
        owner.fallback = fallback;
        owner.progress = None;
        owner.startup_scan = policy != ulmdb::MaintenancePolicy::Disabled;
        drop(owner);
        drop(engine);
        Ok(())
    })
}
