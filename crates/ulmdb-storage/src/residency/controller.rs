#[path = "append.rs"]
mod append;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[path = "fixed.rs"]
mod fixed;
use super::*;
use crate::{Error, Result};
#[derive(Clone)]
pub struct ResidencyController {
    inner: Inner,
}
#[derive(Clone)]
enum Inner {
    Demand(NonZeroU64),
    Managed(Arc<State>),
}
impl ResidencyController {
    pub fn new(policy: ResidencyPolicy) -> Result<Self> {
        static IDS: AtomicU64 = AtomicU64::new(1);
        let id = IDS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::LimitExceeded)?;
        // Checked before any controller resources or lock admission can exist.
        let id = NonZeroU64::new(id).ok_or(Error::LimitExceeded)?;
        let inner = match policy {
            ResidencyPolicy::Demand => Inner::Demand(id),
            ResidencyPolicy::Prefault { max_bytes }
            | ResidencyPolicy::RequiredLocked { max_bytes } => Inner::Managed(Arc::new(State {
                creator: std::process::id(),
                id,
                policy,
                page_size: os::page_size()?,
                budget: max_bytes,
                ledger: std::sync::Mutex::new(Ledger::default()),
                serial: std::sync::Mutex::new(()),
            })),
        };
        Ok(Self { inner })
    }
    pub(super) fn identity(&self) -> NonZeroU64 {
        match &self.inner {
            Inner::Demand(id) => *id,
            Inner::Managed(s) => s.id,
        }
    }
    pub fn policy(&self) -> ResidencyPolicy {
        match &self.inner {
            Inner::Demand(_) => ResidencyPolicy::Demand,
            Inner::Managed(s) => s.policy,
        }
    }
    /// One consistent ledger snapshot, including in-flight conservative charges.
    pub fn live_status(&self) -> ResidencyStatus {
        match &self.inner {
            Inner::Demand(_) => ResidencyStatus {
                policy: ResidencyPolicy::Demand,
                page_size: None,
                reserved_bytes: 0,
                live_locked_bytes: 0,
                retained_failed_bytes: 0,
            },
            Inner::Managed(s) => {
                let ledger = s.ledger.lock().unwrap_or_else(|e| e.into_inner());
                ResidencyStatus {
                    policy: s.policy,
                    page_size: Some(s.page_size as u64),
                    reserved_bytes: ledger.reserved,
                    live_locked_bytes: ledger.locked,
                    retained_failed_bytes: ledger.failed,
                }
            }
        }
    }
    pub fn admit(&self, table: &SegmentTable) -> Result<ResidencyAdmission> {
        self.admit_with(table, &os::OsLocks)
    }
    pub(super) fn admit_with<O: LockOps>(
        &self,
        table: &SegmentTable,
        ops: &O,
    ) -> Result<ResidencyAdmission> {
        let Inner::Managed(state) = &self.inner else {
            return Ok(ResidencyAdmission {
                controller: self.identity(),
                table: table.residency_identity(),
                report: None,
            });
        };
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if matches!(state.policy, ResidencyPolicy::RequiredLocked { .. })
            && table.fixed_map().is_some()
        {
            return self.admit_fixed_with(table, state, ops);
        }
        let _serial = state.serial.lock().map_err(|_| Error::Busy)?;
        let health_epoch = state.ledger.lock().map_err(|_| Error::Busy)?.health_epoch;
        let mut mapped = 0u64;
        let mut pages = 0u64;
        let mut additional = 0u64;
        for mapping in table.mappings() {
            let rounded = rounded(mapping, state.page_size)?;
            mapped = mapped
                .checked_add(mapping.bytes.len() as u64)
                .ok_or(Error::LimitExceeded)?;
            pages = pages.checked_add(rounded).ok_or(Error::LimitExceeded)?;
            if matches!(state.policy, ResidencyPolicy::RequiredLocked { .. }) {
                let guard = mapping.residency.lock().map_err(|_| Error::Busy)?;
                match guard.as_ref() {
                    Some(lock) if lock.lease.state.id != state.id => {
                        return Err(Error::InvalidInput(
                            "mapping belongs to another residency lock controller",
                        ));
                    }
                    Some(lock) if !lock.healthy => {
                        return Err(Error::InvalidInput("mapping lock cleanup unresolved"));
                    }
                    Some(_) => {}
                    None => {
                        additional = additional
                            .checked_add(rounded)
                            .ok_or(Error::LimitExceeded)?
                    }
                }
            }
        }
        if pages > state.budget
            || (matches!(state.policy, ResidencyPolicy::RequiredLocked { .. })
                && state
                    .ledger
                    .lock()
                    .map_err(|_| Error::Busy)?
                    .reserved
                    .checked_add(additional)
                    .is_none_or(|n| n > state.budget))
        {
            return Err(Error::LimitExceeded);
        }
        let mut locked = 0;
        let mut touched = 0;
        match state.policy {
            ResidencyPolicy::Prefault { .. } => {
                for mapping in table.mappings() {
                    let bytes = mapping.data_bytes()?;
                    let first = (bytes.as_ptr() as usize) % state.page_size;
                    std::hint::black_box(bytes[0]);
                    let mut offset = state.page_size - first;
                    while offset < bytes.len() {
                        std::hint::black_box(bytes[offset]);
                        offset = offset
                            .checked_add(state.page_size)
                            .ok_or(Error::LimitExceeded)?;
                    }
                    std::hint::black_box(bytes[bytes.len() - 1]);
                }
                touched = mapped;
            }
            ResidencyPolicy::RequiredLocked { .. } => {
                let mut acquired = Vec::new();
                for mapping in table.mappings() {
                    match lock_one(state, mapping, ops) {
                        Ok(true) => acquired.push(mapping),
                        Ok(false) => {}
                        Err(error) => {
                            rollback(&acquired, state.id, ops);
                            return Err(error);
                        }
                    }
                }
                locked = pages;
            }
            ResidencyPolicy::Demand => unreachable!(),
        }
        Ok(ResidencyAdmission {
            controller: state.id,
            table: table.residency_identity(),
            report: Some(Box::new(ManagedAdmission {
                health_epoch,
                report: ResidencyReport {
                    policy: state.policy,
                    mapped_bytes: Some(mapped),
                    page_rounded_bytes: Some(pages),
                    page_size: Some(state.page_size as u64),
                    locked_bytes: locked,
                    prefaulted_bytes: touched,
                },
            })),
        })
    }
}
fn lock_one<O: LockOps>(state: &Arc<State>, mapping: &Mapping, ops: &O) -> Result<bool> {
    let mut guard = mapping.residency.lock().map_err(|_| Error::Busy)?;
    if let Some(lock) = guard.as_ref() {
        if lock.lease.state.id != state.id {
            return Err(Error::InvalidInput(
                "mapping belongs to another residency lock controller",
            ));
        }
        if !lock.healthy {
            return Err(Error::InvalidInput("mapping lock cleanup unresolved"));
        }
        return Ok(false);
    }
    let bytes = rounded(mapping, state.page_size)?;
    {
        let mut ledger = state.ledger.lock().map_err(|_| Error::Busy)?;
        ledger.reserved = ledger
            .reserved
            .checked_add(bytes)
            .filter(|sum| *sum <= state.budget)
            .ok_or(Error::LimitExceeded)?;
    }
    let mut lease = Lease {
        state: Arc::clone(state),
        bytes,
        kind: LeaseKind::Pending,
    };
    match ops.lock(mapping) {
        Ok(()) => {
            lease.transition(LeaseKind::Locked);
            *guard = Some(LockState {
                lease,
                healthy: true,
            });
            Ok(true)
        }
        Err(error) => {
            revoke_health(state);
            // mlock may have partially locked pages. Release the reservation
            // only after a successful full-range unlock; otherwise quarantine.
            if ops.unlock(mapping).is_err() {
                lease.transition(LeaseKind::Failed);
                *guard = Some(LockState {
                    lease,
                    healthy: false,
                });
            }
            Err(error.into())
        }
    }
}
fn rollback<O: LockOps>(mappings: &[&Mapping], owner: NonZeroU64, ops: &O) {
    for mapping in mappings.iter().rev() {
        let mut guard = match mapping.residency.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if let Some(lock) = guard.as_mut()
            && lock.lease.state.id == owner
            && lock.healthy
        {
            revoke_health(&lock.lease.state);
            if ops.unlock(mapping).is_ok() {
                *guard = None;
            } else {
                lock.healthy = false;
                lock.lease.transition(LeaseKind::Failed);
            }
        }
    }
}
