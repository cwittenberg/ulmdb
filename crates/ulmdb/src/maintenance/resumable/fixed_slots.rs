//! V2 placement pruning uses the same fresh complete root discovery as file
//! reclamation, even while incremental data marks are unfinished. Neither a
//! stale scheduler window nor the pending-deletion list grants this authority.
use super::*;
use ulmdb_storage::{
    coordination::MaintenanceLease,
    fixed_map::{FixedMapDeclaration, FixedSlotReclaimLimits},
};
pub(super) fn reusable(
    env: &Environment,
    current: &Snapshot,
) -> Result<Option<Arc<FixedMapDeclaration>>> {
    if let Some(declaration) = current.segments.fixed_declaration() {
        return Ok(declaration.supports_reuse()?.then_some(declaration));
    }
    if let Some(fixed) = current.segments.fixed_map() {
        if fixed.supports_reuse()? {
            return Ok(Some(FixedMapDeclaration::open(
                &env.directory,
                current.manifest.uuid,
                current.manifest.epoch,
            )?));
        }
    }
    Ok(None)
}
pub(super) fn step(
    declaration: &FixedMapDeclaration,
    gate: &MaintenanceLease,
    roots: &Discovery,
    budget: ResumableReclaimBudget,
    stats: &mut MaintenanceStatus,
) -> Result<()> {
    // Limits separately bound layout scanning, protected metadata and removals.
    // Exhaustion defers optional cleanup; the business selection already exists.
    let limits = FixedSlotReclaimLimits {
        max_layout_slots: budget.max_candidates.min(65_536),
        max_inventory_entries: budget.max_inventory_entries,
        max_reclaimed_slots: budget.unlink_files.clamp(1, 256),
    };
    let manifests: Vec<&dyn ulmdb_storage::fixed_map::FixedSlotRoot> = roots.roots.values().map(|root| root.as_ref() as &dyn ulmdb_storage::fixed_map::FixedSlotRoot).collect();
    let report = declaration.reclaim_slots_from_roots(gate, &manifests, limits)?;
    stats.fixed_slot_reclaim_passes = stats.fixed_slot_reclaim_passes.saturating_add(1);
    stats.fixed_slots_removed = stats
        .fixed_slots_removed
        .saturating_add(report.removed_slots as u64);
    stats.fixed_slot_bytes_released = stats
        .fixed_slot_bytes_released
        .saturating_add(report.released_bytes);
    stats.fixed_slots_busy = stats
        .fixed_slots_busy
        .saturating_add(report.busy_slots as u64);
    Ok(())
}

/// Admission-time placement-only cleanup has its own bounded slot allowance.
/// It does not advance lifecycle/graph marking or double the configured per-
/// commit data/history unlink allowance. Caller holds the native writer guard.
pub(super) fn before_write(
    scheduler: &mut Scheduler,
    env: &Environment,
    budget: ResumableReclaimBudget,
) -> Result<()> {
    scheduler.check()?;
    env.check_file_namespace()?;
    if env.poisoned.load(Ordering::Acquire) {
        return Err(Error::OutcomeUnknown);
    }
    let current = env.current.load_full();
    let Some(declaration) = reusable(env, &current)? else {
        return Ok(());
    };
    match env.cached_file_durability()? {
        Some(d) if !d.is_weak() && !d.has_data_debt() => (),
        _ => return Err(Error::Busy),
    }
    let mut gate = env
        .coordination
        .as_ref()
        .ok_or(Error::InvalidInput("missing coordination"))?
        .try_maintenance()?;
    gate.check()?;
    ulmdb_platform::sync_directory(&env.directory)?;
    if env.directory.join("retained").try_exists()? {
        ulmdb_platform::sync_directory(&env.directory.join("retained"))?;
    }
    let state = &mut *scheduler.state;
    // Discovery's pending source leases keep old data-mark inputs alive. They
    // are optional progress, and can otherwise make obsolete fixed slots look
    // live forever across successive short-lived writers. Release that proof
    // together with every mark derived from it, then rediscover all actual
    // readers/durable roots under this fresh Capture. Candidate/event queues
    // remain only work to consider, never authority to remove a slot or file.
    state.window = None;
    state.discovery = None;
    state.stats.last_resumable_mark_units = 0;
    if !discover_bounded(env, &mut gate, &current, budget, state)? { return Ok(()); }
    let roots = &state.discovery.as_ref().unwrap().roots;
    step(
        &declaration,
        &gate,
        &roots,
        budget,
        &mut state.stats,
    )?;
    verify(env, &gate, &roots.selected, &roots.previous)
}
