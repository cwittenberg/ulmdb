//! Fixed per-environment residency policy. Mapping-owned leases outlive readers.
mod controller;
pub(crate) mod os;
#[cfg(test)]
pub(crate) mod tests;
use crate::{SegmentTable, mapping::Mapping};
pub use controller::ResidencyController;
use std::num::NonZeroU64;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResidencyPolicy {
    #[default]
    Demand,
    Prefault {
        max_bytes: u64,
    },
    RequiredLocked {
        max_bytes: u64,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResidencyReport {
    pub policy: ResidencyPolicy,
    pub mapped_bytes: Option<u64>,
    pub page_rounded_bytes: Option<u64>,
    pub page_size: Option<u64>,
    pub locked_bytes: u64,
    pub prefaulted_bytes: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResidencyStatus {
    pub policy: ResidencyPolicy,
    pub page_size: Option<u64>,
    pub reserved_bytes: u64,
    pub live_locked_bytes: u64,
    pub retained_failed_bytes: u64,
}
/// Evidence only the controller can construct. It binds both controller and
/// immutable mapping-table revision; cloning does not extend any lock lifetime.
/// Demand stores no report allocation. Managed policies own a boxed report so
/// the opt-in details do not enlarge every default snapshot.
#[derive(Clone, Debug)]
pub struct ResidencyAdmission {
    controller: NonZeroU64,
    table: u64,
    report: Option<Box<ManagedAdmission>>,
}
impl ResidencyAdmission {
    pub fn report(&self) -> ResidencyReport {
        match &self.report {
            Some(evidence) => evidence.report,
            None => ResidencyReport {
                policy: ResidencyPolicy::Demand,
                mapped_bytes: None,
                page_rounded_bytes: None,
                page_size: None,
                locked_bytes: 0,
                prefaulted_bytes: 0,
            },
        }
    }
    pub fn is_for(&self, controller: &ResidencyController) -> bool {
        self.controller == controller.identity()
    }
    pub fn is_for_table(&self, table: &SegmentTable) -> bool {
        self.table == table.residency_identity()
    }
}
/// Epoch is private evidence, not a public report claim. None disables the
/// incremental shortcut permanently after counter exhaustion.
#[derive(Clone, Debug)]
struct ManagedAdmission {
    report: ResidencyReport,
    health_epoch: Option<NonZeroU64>,
}
pub(crate) struct State {
    creator: u32,
    id: NonZeroU64,
    policy: ResidencyPolicy,
    page_size: usize,
    budget: u64,
    ledger: std::sync::Mutex<Ledger>,
    serial: std::sync::Mutex<()>,
}
struct Ledger {
    reserved: u64,
    locked: u64,
    failed: u64,
    health_epoch: Option<NonZeroU64>,
}
impl Default for Ledger {
    fn default() -> Self {
        Self {
            reserved: 0,
            locked: 0,
            failed: 0,
            health_epoch: NonZeroU64::new(1),
        }
    }
}
/// Call BEFORE a rollback/cleanup can revoke any healthy-map assumption.
fn revoke_health(state: &State) {
    let mut ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
    ledger.health_epoch = ledger
        .health_epoch
        .and_then(|epoch| epoch.get().checked_add(1))
        .and_then(NonZeroU64::new);
}
#[derive(Clone, Copy)]
enum LeaseKind {
    Pending,
    Locked,
    Failed,
}
pub(crate) struct Lease {
    state: Arc<State>,
    bytes: u64,
    kind: LeaseKind,
}
impl Lease {
    fn transition(&mut self, kind: LeaseKind) {
        let mut ledger = self.state.ledger.lock().unwrap_or_else(|e| e.into_inner());
        match self.kind {
            LeaseKind::Locked => ledger.locked -= self.bytes,
            LeaseKind::Failed => ledger.failed -= self.bytes,
            LeaseKind::Pending => {}
        }
        match kind {
            LeaseKind::Locked => ledger.locked += self.bytes,
            LeaseKind::Failed => ledger.failed += self.bytes,
            LeaseKind::Pending => {}
        }
        self.kind = kind;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        // A vanished creator thread may hold this inherited mutex. The child
        // owns no parent accounting authority and must not inspect the ledger.
        if self.state.creator != std::process::id() {
            return;
        }
        // Mmap is the first Mapping field; its destructor unmaps and implicitly
        // unlocks before the later lock-state field releases this reservation.
        // Explicit rollback drops a lease only after successful full munlock.
        let mut ledger = self.state.ledger.lock().unwrap_or_else(|e| e.into_inner());
        ledger.reserved -= self.bytes;
        match self.kind {
            LeaseKind::Locked => ledger.locked -= self.bytes,
            LeaseKind::Failed => ledger.failed -= self.bytes,
            LeaseKind::Pending => {}
        }
    }
}
/// Owned by the physical fixed-slot record, never an individual mapping.
/// Successful reprotect/unmap precedes final release; poisoned registry records
/// retain this charge until the complete reservation is destroyed.
pub(crate) struct FixedCharge {
    lease: Lease,
    healthy: bool,
}
pub(crate) struct LockState {
    lease: Lease,
    healthy: bool,
}
trait LockOps {
    fn lock(&self, mapping: &Mapping) -> std::io::Result<()>;
    fn unlock(&self, mapping: &Mapping) -> std::io::Result<()>;
}
fn rounded(mapping: &Mapping, page: usize) -> crate::Result<u64> {
    let start = (mapping.bytes.as_ptr() as usize) % page;
    let span = start
        .checked_add(mapping.bytes.len())
        .ok_or(crate::Error::LimitExceeded)?;
    let pages = span
        .checked_add(page - 1)
        .ok_or(crate::Error::LimitExceeded)?
        / page;
    let bytes = pages.checked_mul(page).ok_or(crate::Error::LimitExceeded)?;
    u64::try_from(bytes).map_err(|_| crate::Error::LimitExceeded)
}
