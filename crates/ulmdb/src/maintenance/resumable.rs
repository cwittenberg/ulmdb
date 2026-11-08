//! Opt-in resumable automatic inventory marking and exact cohort history. No writer/Capture lease survives a
//! step. Metadata is progress, never a serving snapshot or deletion certificate:
//! each unlink requires a fresh, fully covered root set under Capture EX.
use super::{MaintenanceStatus, ResumableReclaimBudget};
use crate::{Environment, Error, Result, environment::Snapshot, reclaim::checked_selector};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    mem::ManuallyDrop,
    sync::{Arc, atomic::Ordering},
};
use ulmdb_storage::{Inventory, Manifest, SegmentEntry};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::fs::File;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod fixed_slots;
mod lifecycle;
mod root;
mod discovery;
use root::ProtectedRoot;
type Checkpoint = [u8; 64];
type Hash = [u8; 32];
const MAX_EVENTS: usize = 16;
const MAX_MARKS: usize = 4_194_304;

struct RetiredInventory {
    before: Inventory,
    after: Inventory,
    at: usize,
}
struct RootWork {
    checkpoint: Checkpoint,
    root: Arc<ProtectedRoot>,
    at: usize,
    stage: u8,
    origins: Option<ulmdb_storage::MetadataOriginCursor>,
    origin_after: Option<u128>,
}
struct Window {
    cutoff: u64,
    // Optional physical baseline ownership, never the authority for unlink.
    // Released before the next writer admission so a custom two-pin owner can
    // still admit its next candidate. Immutable metadata progress remains.
    pin: Option<Arc<Snapshot>>,
    anchor: Checkpoint,
    forward: BTreeSet<Checkpoint>,
    covered: BTreeSet<Checkpoint>,
    queued: BTreeSet<Checkpoint>,
    roots: VecDeque<RootWork>,
    protected: BTreeSet<Hash>,
    protected_cohorts: BTreeSet<u128>,
    after: Option<Hash>,
}
#[derive(Default)]
struct State {
    events: VecDeque<RetiredInventory>,
    candidates: BTreeMap<Hash, SegmentEntry>,
    window: Option<Window>,
    stats: MaintenanceStatus,
    lifecycle: lifecycle::Lifecycle,
    lifecycle_turn: bool,
    discovery: Option<discovery::Progress>,
    last_previous: Option<(Checkpoint, Arc<ProtectedRoot>)>,
}
pub(super) struct Scheduler {
    creator: u32,
    state: ManuallyDrop<State>,
    cross_format: bool,
}
impl Drop for Scheduler {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let state = std::mem::replace(&mut self.state, ManuallyDrop::new(State::default()));
            drop(ManuallyDrop::into_inner(state));
        }
    }
}
impl Scheduler {
    pub(super) fn has_pending_work(&self) -> Result<bool> {
        self.check()?;
        Ok(!self.state.events.is_empty()
            || !self.state.candidates.is_empty()
            || self.state.window.is_some()
            || self.state.lifecycle.pending())
    }
    pub(super) fn new(status: MaintenanceStatus, cross_format: bool) -> Self {
        Self {
            creator: std::process::id(),
            cross_format,
            state: ManuallyDrop::new(State {
                stats: status,
                ..Default::default()
            }),
        }
    }
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited reclaim scheduler"));
        }
        Ok(())
    }
    pub(super) fn release_optional_pin(&mut self) {
        if self.check().is_err() {
            return;
        }
        let s = &mut *self.state;
        if let Some(w) = &mut s.window {
            if w.pin.take().is_some() {
                s.stats.resumable_released_snapshot_pins =
                    s.stats.resumable_released_snapshot_pins.saturating_add(1);
            }
        }
    }
    pub(super) fn reset_transfer_window(&mut self) {
        if self.check().is_ok() {
            self.state.window = None;
            self.state.discovery = None;
            self.state.lifecycle.reset();
            self.state.stats.resumable_nonforward_resets = self
                .state
                .stats
                .resumable_nonforward_resets
                .saturating_add(1);
        }
    }
    pub(super) fn retention_changed(&mut self) {
        if self.check().is_ok() {
            // Only retention mutations restart the retained-name cursor. Normal
            // forward publication preserves physical-registry discovery progress.
            if let Some(d) = &mut self.state.discovery { d.retention_changed(); }
            self.state.stats.resumable_retention_changes = self
                .state
                .stats
                .resumable_retention_changes
                .saturating_add(1);
        }
    }
    pub(super) fn activation(
        &mut self,
        before: &Snapshot,
        after: &Snapshot,
        forward: bool,
        retire: bool,
    ) {
        if self.check().is_err() {
            return;
        }
        let s = &mut *self.state;
        s.lifecycle.observe(before, after, forward);
        s.last_previous = before.physical_pin.as_ref().map(|p| (
            *p.root().checkpoint(), Arc::new(ProtectedRoot::Manifest(before.manifest.clone()))
        ));
        if let (Some(d), Some((p, m))) = (&mut s.discovery, &s.last_previous) {
            if let ProtectedRoot::Manifest(m) = m.as_ref() { d.previous_owned(*p, m); }
        }
        if retire {
            s.lifecycle.dirty();
            s.lifecycle.request_orphans();
        }
        let same = before.manifest.uuid == after.manifest.uuid
            && before.manifest.epoch == after.manifest.epoch
            && ((before.manifest.format_minor < 3 && after.manifest.format_minor < 3)
                || (self.cross_format
                    && before.manifest.format_minor <= 4
                    && before.manifest.format_minor == after.manifest.format_minor));
        if !forward || !same {
            s.window = None;
            s.discovery = None;
            s.stats.resumable_nonforward_resets =
                s.stats.resumable_nonforward_resets.saturating_add(1);
        } else if let Some(w) = &mut s.window {
            match (before.physical_pin.as_ref(), after.physical_pin.as_ref()) {
                (Some(b), Some(a)) if w.forward.contains(b.root().checkpoint()) => {
                    // Only ordinary WriteTxn constructs this private transition.
                    // New objects have monotonic segment IDs; existing roots
                    // derive from its exact base. No import/rollback/previous
                    // path can mint this no-resurrection coverage.
                    if w.forward.len() >= 32_768 {
                        s.window = None;
                        s.stats.resumable_nonforward_resets =
                            s.stats.resumable_nonforward_resets.saturating_add(1);
                    } else {
                        w.forward.insert(*a.root().checkpoint());
                    }
                }
                _ => {
                    s.window = None;
                    s.stats.resumable_nonforward_resets =
                        s.stats.resumable_nonforward_resets.saturating_add(1);
                }
            }
        }
        if retire && same {
            if s.events.len() == MAX_EVENTS {
                // Best effort backlog overflow leaks/defer candidates, never
                // grants deletion or fails an already published business write.
                s.stats.resumable_deferred_events =
                    s.stats.resumable_deferred_events.saturating_add(1);
            } else {
                s.events.push_back(RetiredInventory {
                    before: before.manifest.segments.clone(),
                    after: after.manifest.segments.clone(),
                    at: 0,
                });
            }
        }
    }
    pub(super) fn report(&self, status: &mut MaintenanceStatus) {
        if self.check().is_err() {
            return;
        }
        let s = &*self.state;
        macro_rules! copy { ($($f:ident),* $(,)?) => { $(status.$f = s.stats.$f;)* }; }
        copy!(
            resumable_marked_units,
            last_resumable_mark_units,
            resumable_windows_started,
            resumable_windows_completed,
            resumable_nonforward_resets,
            resumable_retention_changes,
            resumable_discovered_roots,
            resumable_discovered_inventory_entries,
            resumable_deferred_events,
            resumable_released_snapshot_pins,
            resumable_archived_cohorts,
            resumable_archived_metadata_bytes,
            fixed_slot_reclaim_passes,
            fixed_slots_removed,
            fixed_slot_bytes_released,
            fixed_slots_busy
        );
        status.resumable_pending_candidates = s.candidates.len();
        status.resumable_pending_roots = s.window.as_ref().map_or(0, |w| w.roots.len());
        // Only this policy owns these cumulative cleanup counters while enabled.
        status.scanned_directory_entries = s.stats.scanned_directory_entries;
        status.reclaimed_files = s.stats.reclaimed_files;
        status.reclaimed_bytes = s.stats.reclaimed_bytes;
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(super) fn prune_fixed_slots_before_write(
        &mut self,
        env: &Environment,
        budget: ResumableReclaimBudget,
    ) -> Result<()> {
        fixed_slots::before_write(self, env, budget)
    }
    pub(super) fn step(
        &mut self,
        env: &Environment,
        budget: ResumableReclaimBudget,
        mut history_budget: u64,
    ) -> Result<()> {
        self.check()?;
        env.check_file_namespace()?;
        if env.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let s = &mut *self.state;
        s.stats.last_resumable_mark_units = 0;
        let history_pending = s.lifecycle.refresh(env)?;
        let current = env.current.load_full();
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let fixed_slots = fixed_slots::reusable(env, &current)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let has_fixed_slots = fixed_slots.is_some();
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let has_fixed_slots = false;
        if s.events.is_empty() && s.candidates.is_empty() && !history_pending && !has_fixed_slots {
            s.window = None;
            s.discovery = None;
            return Ok(());
        }
        match env.cached_file_durability()? {
            Some(d) if !d.is_weak() && !d.has_data_debt() => (),
            _ => return Err(Error::Busy),
        }
        // One authenticated old-inventory entry per unit, bounded independent of
        // current inventory length. No hash-named file/header high-water guesses.
        let difference_budget = budget.mark_units / 2;
        for _ in 0..difference_budget {
            let Some(e) = s.events.front_mut() else {
                break;
            };
            let Some(old) = e.before.get(e.at) else {
                s.events.pop_front();
                continue;
            };
            if s.candidates.len() >= budget.max_candidates && !s.candidates.contains_key(&old.hash)
            {
                break;
            }
            if (old.cohort == 0 || self.cross_format) && !contains(&e.after, old) {
                s.candidates.entry(old.hash).or_insert_with(|| old.clone());
            }
            e.at += 1;
            s.stats.last_resumable_mark_units += 1;
            s.stats.resumable_marked_units = s.stats.resumable_marked_units.saturating_add(1);
        }
        if current.manifest.format_minor > 4
            || (!self.cross_format && current.manifest.format_minor >= 3)
        {
            return Err(Error::InvalidInput(
                "resumable Legacy selection changed format",
            ));
        }
        // Already authenticated by owner activation. Reusing its exact fixed
        // checkpoint does not re-encode/hash the full Legacy inventory.
        let checkpoint = *current
            .physical_pin
            .as_ref()
            .ok_or(Error::InvalidInput("unbound scheduler head"))?
            .root()
            .checkpoint();
        if s.window.is_none() {
            s.window = Some(Window {
                cutoff: current.manifest.next_segment,
                pin: Some(current.clone()),
                anchor: checkpoint,
                forward: BTreeSet::from([checkpoint]),
                covered: BTreeSet::new(),
                queued: BTreeSet::new(),
                roots: VecDeque::new(),
                protected: BTreeSet::new(),
                protected_cohorts: BTreeSet::new(),
                after: None,
            });
            s.stats.resumable_windows_started = s.stats.resumable_windows_started.saturating_add(1);
        }
        // Discovery retains only sealed-membership/pending-source leases. Each
        // call reacquires Writer/Capture authority and charges registry, descriptor
        // bytes and metadata work before marking or unlink can proceed.
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
        // Queued roots retain their authenticated source capabilities in
        // discovery. New registry frontiers must not consume every unit before
        // that independent graph work advances. A unit reserved here is part
        // of the same per-call budget, never additional work.
        let reserve_mark = usize::from(!s.window.as_ref().unwrap().roots.is_empty());
        let mut discovery_budget = budget;
        discovery_budget.mark_units = budget.mark_units.saturating_sub(reserve_mark);
        let discovery_ready = if s.stats.last_resumable_mark_units < discovery_budget.mark_units {
            discover_bounded(env, &mut gate, &current, discovery_budget, s)?
        } else {
            false
        };
        if !discovery_ready && s.window.as_ref().unwrap().roots.is_empty() {
            return Ok(());
        }
        let roots = &s.discovery.as_ref().unwrap().roots;
        if discovery_ready {
            s.stats.resumable_discovered_roots = s
                .stats
                .resumable_discovered_roots
                .saturating_add(roots.roots.len() as u64);
            s.stats.resumable_discovered_inventory_entries = s
                .stats
                .resumable_discovered_inventory_entries
                .saturating_add(roots.entries as u64);
        }
        let w = s.window.as_mut().unwrap();
        if discovery_ready {
            let actual: BTreeSet<_> = roots.roots.keys().copied().collect();
            w.forward.retain(|p| *p == w.anchor || actual.contains(p));
            w.covered.retain(|p| *p == w.anchor || actual.contains(p));
            for (&p, manifest) in &roots.roots {
                // Forward descendants need the anchor's completed marks. They do
                // not enqueue a newly growing inventory on each publishing commit.
                if w.covered.contains(&p)
                    || (p != w.anchor && w.forward.contains(&p))
                    || w.queued.contains(&p)
                {
                    continue;
                }
                if w.roots.len() + w.covered.len() >= budget.max_roots {
                    return Err(Error::LimitExceeded);
                }
                w.queued.insert(p);
                w.roots.push_back(RootWork {
                    checkpoint: p,
                    root: manifest.clone(),
                    at: 0,
                    stage: 0,
                    origins: None,
                    origin_after: None,
                });
            }
        }
        while s.stats.last_resumable_mark_units < budget.mark_units {
            let Some(r) = w.roots.front_mut() else {
                break;
            };
            if w.protected.len() + w.protected_cohorts.len() >= MAX_MARKS {
                return Err(Error::LimitExceeded);
            }
            match r.stage {
                0 => {
                    if r.root.minor() >= 3 {
                        w.protected_cohorts.insert(r.root.generation());
                    }
                    r.stage = 1;
                }
                1 => {
                    if let Some(e) = r.root.entry(r.at) {
                        w.protected.insert(e.hash);
                        if e.cohort != 0 {
                            w.protected_cohorts.insert(e.cohort);
                        }
                        r.at += 1;
                    } else {
                        if let ProtectedRoot::Manifest(m) = r.root.as_ref() {
                            r.origins = Some(m.metadata_origin_cursor()?);
                        }
                        r.stage = 2;
                    }
                }
                2 => {
                    let origin = match r.root.as_ref() {
                        ProtectedRoot::Manifest(_) => r.origins.as_mut().unwrap().next_origin()?,
                        ProtectedRoot::Flat(_) => None,
                        ProtectedRoot::Indexed(m) => m.metadata_origin_after(r.origin_after),
                    };
                    if let Some(origin) = origin {
                        r.origin_after = Some(origin);
                        w.protected_cohorts.insert(origin);
                    } else {
                        let r = w.roots.pop_front().unwrap();
                        w.queued.remove(&r.checkpoint);
                        w.covered.insert(r.checkpoint);
                    }
                }
                _ => unreachable!(),
            }
            s.stats.last_resumable_mark_units += 1;
            s.stats.resumable_marked_units = s.stats.resumable_marked_units.saturating_add(1);
        }
        if !discovery_ready || !w.roots.is_empty() {
            return Ok(());
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(declaration) = fixed_slots {
            fixed_slots::step(&declaration, &gate, &roots, budget, &mut s.stats)?;
            verify(env, &gate, &roots.selected, &roots.previous)?;
        }
        // Fresh roots all have either exact completed marks or private forward
        // coverage rooted at the marked anchor. Capture EX forbids new physical
        // roots. Native writer exclusion forbids retention/history changes.
        if !w.covered.contains(&w.anchor) {
            return Err(Error::Busy);
        }
        verify(env, &gate, &roots.selected, &roots.previous)?;
        // A pending history/orphan cursor receives one unlink slot on
        // alternating sweeps, so full retirement queues cannot starve it.
        let reserve = usize::from(s.lifecycle.pending() && s.lifecycle_turn);
        s.lifecycle_turn = !s.lifecycle_turn;
        let data_unlinks = budget.unlink_files.saturating_sub(reserve);
        let mut examined = 0usize;
        let mut removed = 0usize;
        let mut changed_directories = BTreeSet::new();
        let mut history_inventory = budget.max_inventory_entries;
        let mut exhausted = true;
        let scan_limit = budget.unlink_files.saturating_mul(8).saturating_add(16);
        let keys: Vec<Hash> = s
            .candidates
            .range((
                w.after
                    .map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded),
                std::ops::Bound::Unbounded,
            ))
            .take(scan_limit + 1)
            .map(|(h, _)| *h)
            .collect();
        for hash in keys {
            if examined >= scan_limit || removed >= data_unlinks {
                exhausted = false;
                break;
            }
            examined += 1;
            w.after = Some(hash);
            let entry = &s.candidates[&hash];
            if entry.id >= w.cutoff
                || w.protected.contains(&hash)
                || (entry.cohort != 0 && w.protected_cohorts.contains(&entry.cohort))
            {
                continue;
            }
            gate.check()?;
            let hex: String = hash.iter().map(|x| format!("{x:02x}")).collect();
            let path = if entry.cohort == 0 {
                env.directory.join("segments").join(format!("{hex}.seg"))
            } else {
                env.directory
                    .join("cohorts")
                    .join(format!("{:032x}.cohort", entry.cohort))
            };
            match fs::symlink_metadata(&path) {
                Ok(m) if m.is_file() && !m.file_type().is_symlink() => {
                    if entry.cohort != 0 {
                        let report = gate.archive_cohort_history(
                            entry.cohort,
                            history_budget,
                            history_inventory,
                        )?;
                        history_inventory = history_inventory
                            .checked_sub(report.inventory_entries)
                            .ok_or(Error::LimitExceeded)?;
                        history_budget = history_budget
                            .checked_sub(report.metadata_bytes)
                            .ok_or(Error::LimitExceeded)?;
                        if report.created {
                            s.stats.resumable_archived_cohorts =
                                s.stats.resumable_archived_cohorts.saturating_add(1);
                            s.stats.resumable_archived_metadata_bytes = s
                                .stats
                                .resumable_archived_metadata_bytes
                                .saturating_add(report.metadata_bytes);
                        }
                        verify(env, &gate, &roots.selected, &roots.previous)?;
                    }
                    #[cfg(windows)]
                    gate.remove_file(&path)?;
                    #[cfg(not(windows))]
                    fs::remove_file(&path)?;
                    changed_directories.insert(path.parent().unwrap().to_path_buf());
                    removed += 1;
                    s.stats.reclaimed_files = s.stats.reclaimed_files.saturating_add(1);
                    s.stats.reclaimed_bytes = s.stats.reclaimed_bytes.saturating_add(m.len());
                }
                Ok(_) => return Err(Error::Corrupt("nonregular retired segment")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
            s.candidates.remove(&hash);
        }
        for directory in changed_directories {
            ulmdb_platform::sync_directory(&directory)?;
        }
        verify(env, &gate, &roots.selected, &roots.previous)?;
        let history_result = s.lifecycle.step(
            env,
            &mut gate,
            &roots,
            w,
            budget,
            history_budget,
            history_inventory,
            budget.unlink_files.saturating_sub(removed),
            &mut s.stats,
        );
        // Lifecycle plans/cursors are independent state. Their pending work
        // must not retain this data cutoff after its candidate sweep finishes.
        if exhausted {
            s.window = None;
            s.discovery = None;
            s.stats.resumable_windows_completed =
                s.stats.resumable_windows_completed.saturating_add(1);
        }
        history_result?;
        Ok(())
    }
}
fn discover_bounded(env:&Environment, gate:&mut ulmdb_storage::coordination::MaintenanceLease, current:&Arc<Snapshot>, budget:ResumableReclaimBudget, state:&mut State) -> Result<bool> {
    if state.discovery.is_none() {
        let checkpoint=*current.physical_pin.as_ref().ok_or(Error::InvalidInput("unbound discovery head"))?.root().checkpoint();
        let mut progress=discovery::Progress::new(env,gate,state.window.as_ref().map_or(checkpoint,|w|w.anchor))?;
        if let Some((p,m))=&state.last_previous { if let ProtectedRoot::Manifest(m)=m.as_ref() { progress.previous_owned(*p,m); } }
        state.discovery=Some(progress);
    }
    if let Some(window)=&state.window {
        state.discovery.as_mut().unwrap().anchor(window.anchor, gate, env._file_lock.as_ref().ok_or(Error::InvalidInput("discovery requires writer fence"))?)?;
    }
    let before=state.stats.last_resumable_mark_units;
    // Refresh can seal another registry frontier after a bounded pass. Finish
    // it under this same Capture exclusion while budget remains. Otherwise
    // every short-lived writer can add births before discovery becomes ready,
    // leaving even small fixed windows and orphan sweeps permanently deferred.
    let result = loop {
        let used = state.stats.last_resumable_mark_units;
        match state.discovery.as_mut().unwrap().advance(
            env,
            gate,
            current,
            budget,
            &mut state.stats.last_resumable_mark_units,
        ) {
            Ok(true) => break Ok(true),
            Err(error) => break Err(error),
            Ok(false) => {
                if state.stats.last_resumable_mark_units >= budget.mark_units
                    || state.stats.last_resumable_mark_units == used
                {
                    break Ok(false);
                }
            }
        }
    };
    if result.is_err() { state.discovery=None; state.window=None; }
    state.stats.resumable_marked_units=state.stats.resumable_marked_units.saturating_add(state.stats.last_resumable_mark_units.saturating_sub(before) as u64);
    result
}

fn contains(inventory: &Inventory, old: &SegmentEntry) -> bool {
    let mut low = 0;
    let mut high = inventory.len();
    while low < high {
        let mid = low + (high - low) / 2;
        let e = inventory.get(mid).unwrap();
        match e.id.cmp(&old.id) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => {
                return e.hash == old.hash && e.cohort == old.cohort && e.length == old.length;
            }
        }
    }
    false
}
struct Discovery {
    roots: BTreeMap<Checkpoint, Arc<ProtectedRoot>>,
    entries: usize,
    selected: Vec<u8>,
    previous: Option<Vec<u8>>,
}

fn verify(
    env: &Environment,
    gate: &ulmdb_storage::coordination::MaintenanceLease,
    selected: &Vec<u8>,
    previous: &Option<Vec<u8>>,
) -> Result<()> {
    env.check_file_namespace()?;
    gate.check()?;
    if env.directory.canonicalize()? != env.directory {
        return Err(Error::Corrupt("resumable namespace alias"));
    }
    if checked_selector(&env.directory, "CURRENT", false)?.as_ref() != Some(selected) {
        env.poisoned.store(true, Ordering::Release);
        return Err(Error::OutcomeUnknown);
    }
    if selected.len() == 64 && &checked_selector(&env.directory, "PREVIOUS", true)? != previous {
        return Err(Error::Corrupt("resumable previous changed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DatabaseKind, MaintenancePolicy};
    #[test]
    fn queued_graph_work_advances_with_one_unit_during_new_physical_births() {
        let d = std::env::temp_dir().join(format!(
            "resumable-mark-fairness-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let e = Environment::create(&d).unwrap();
        let mut write = e.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write.put(b"kv", b"k", &[17; 12000]).unwrap();
        write.commit().unwrap();
        let reader = e.begin_read();
        let current = e.current.load_full();
        let checkpoint = *current.physical_pin.as_ref().unwrap().root().checkpoint();
        let coordination = e.coordination.as_ref().unwrap();
        let mut scheduler = Scheduler::new(MaintenanceStatus::default(), false);
        let mut gate = coordination.try_maintenance().unwrap();
        let progress = discovery::Progress::new(&e, &mut gate, checkpoint).unwrap();
        drop(gate);
        scheduler.state.discovery = Some(progress);
        scheduler.state.window = Some(Window {
            cutoff: current.manifest.next_segment,
            pin: Some(current.clone()),
            anchor: checkpoint,
            forward: BTreeSet::from([checkpoint]),
            covered: BTreeSet::new(),
            queued: BTreeSet::from([checkpoint]),
            roots: VecDeque::from([RootWork {
                checkpoint,
                root: Arc::new(ProtectedRoot::Manifest(current.manifest.clone())),
                at: 0,
                stage: 0,
                origins: None,
                origin_after: None,
            }]),
            protected: BTreeSet::new(),
            protected_cohorts: BTreeSet::new(),
            after: None,
        });
        let budget = ResumableReclaimBudget {
            mark_units: 1,
            unlink_files: 1,
            ..Default::default()
        };
        let mut births = Vec::new();
        let mut completed = false;
        for _ in 0..current.manifest.segments.len() + 8 {
            births.push(
                coordination
                    .try_capture()
                    .unwrap()
                    .pin_checkpoint(&checkpoint)
                    .unwrap(),
            );
            let before = scheduler.state.stats.resumable_marked_units;
            {
                let _writer = e.writer.lock().unwrap();
                scheduler.step(&e, budget, 0).unwrap();
            }
            assert_eq!(scheduler.state.stats.last_resumable_mark_units, 1);
            assert_eq!(scheduler.state.stats.resumable_marked_units, before + 1);
            assert_eq!(scheduler.state.stats.reclaimed_files, 0);
            assert_eq!(scheduler.state.stats.resumable_windows_completed, 0);
            assert_eq!(reader.get(b"kv", b"k").unwrap().unwrap()[0], 17);
            for entry in current.manifest.segments.iter() {
                assert!(ulmdb_storage::segment_path(&d, entry).is_file());
            }
            let window = scheduler.state.window.as_ref().unwrap();
            if window.roots.is_empty() {
                assert!(window.covered.contains(&checkpoint));
                completed = true;
                break;
            }
        }
        assert!(completed, "new frontiers starved authenticated graph marks");
        drop((scheduler, births, reader, current, e));
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn optional_progress_pin_drops_before_two_pin_writer_candidate_admission() {
        let d = std::env::temp_dir().join(format!(
            "resumable-two-pins-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let mut e = Environment::create(&d).unwrap();
        {
            let mut w = e.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"k", &[0; 12000]).unwrap();
            w.commit().unwrap();
        }
        for n in 1..24u8 {
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"k", &[n; 12000]).unwrap();
            w.commit().unwrap();
        }
        // Rebind a genuine admitted owner to a coordinator whose original pin
        // admission limit is two. No forged snapshot or fake physical pin.
        let before = e.current.load_full();
        let coord = ulmdb_storage::coordination::Coordination::open(
            &d,
            before.manifest.uuid,
            ulmdb_storage::coordination::Limits {
                max_pin_records: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let mut snapshot = crate::environment::load_snapshot_like(
            &d,
            before.manifest.clone(),
            before.owner,
            &before.segments,
        )
        .unwrap();
        e.admit_snapshot(&mut snapshot).unwrap();
        snapshot
            .pin_with_capture(&coord.try_capture().unwrap())
            .unwrap();
        e.current.store(Arc::new(snapshot));
        e.coordination = Some(coord.clone());
        drop(before);
        e.set_maintenance_policy(MaintenancePolicy::LegacyResumable {
            segment_threshold: usize::MAX,
            rewrite_budget_bytes: 16 << 20,
            reclaim: ResumableReclaimBudget {
                mark_units: 2,
                unlink_files: 1,
                ..Default::default()
            },
        })
        .unwrap();
        e.prune_inventory().unwrap();
        for n in 24..80u8 {
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"k", &[n; 12000]).unwrap();
            let ready = w.prepare().unwrap();
            // Current plus the prepared candidate fit the real two-pin cap.
            // A third ordinary admission must still fail; internal discovery
            // records neither consume nor silently raise that reader budget.
            let capture = coord.try_capture().unwrap();
            assert!(matches!(capture.pin_current(), Err(Error::LimitExceeded)));
            drop(capture);
            ready.commit().unwrap();
            assert!(e.maintenance_status().unwrap().last_resumable_mark_units <= 2);
        }
        assert!(
            e.maintenance_status()
                .unwrap()
                .resumable_released_snapshot_pins
                > 0
        );
        assert_eq!(e.begin_read().get(b"kv", b"k").unwrap().unwrap()[0], 79);
        drop(e);
        drop(coord);
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn newly_pinned_historical_root_is_queued_before_unlink() {
        let d = std::env::temp_dir().join(format!(
            "resumable-late-pin-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let e = Environment::create(&d).unwrap();
        let mut checkpoint = [0; 64];
        for n in 1..=20u8 {
            let mut w = e.begin_write().unwrap();
            if n == 1 {
                w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            }
            w.put(b"kv", b"k", &[n; 12000]).unwrap();
            w.commit().unwrap();
            if n == 10 {
                checkpoint = e.file_checkpoint().unwrap();
            }
        }
        e.set_maintenance_policy(MaintenancePolicy::LegacyResumable {
            segment_threshold: usize::MAX,
            rewrite_budget_bytes: 16 << 20,
            reclaim: ResumableReclaimBudget {
                mark_units: 2,
                unlink_files: 1,
                ..Default::default()
            },
        })
        .unwrap();
        e.prune_inventory().unwrap();
        {
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"k", &[21; 12000]).unwrap();
            w.commit().unwrap();
        }
        {
            let state = e.maintenance.lock().unwrap();
            let window = state
                .resumable
                .as_ref()
                .unwrap()
                .state
                .window
                .as_ref()
                .unwrap();
            assert!(!window.queued.contains(&checkpoint));
            assert!(!window.covered.contains(&checkpoint));
            assert!(!window.forward.contains(&checkpoint));
        }
        // Genuine descriptor pin created only AFTER marking has started. It
        // was absent from both the owner's weak list and initial registry walk.
        let coord = e.coordination.as_ref().unwrap();
        let pin = coord
            .try_capture()
            .unwrap()
            .pin_checkpoint(&checkpoint)
            .unwrap();
        let old = crate::ReadTxn::from_physical_pin_with_read_ahead(
            &d,
            crate::ResidencyPolicy::Demand,
            pin,
            crate::ReadAhead::Normal,
        )
        .unwrap();
        let completed_before = e.maintenance_status().unwrap().resumable_windows_completed;
        let check_old = || {
            assert_eq!(old.get(b"kv", b"k").unwrap().unwrap()[0], 10);
            // A mapped value could remain readable after unlink. Check the
            // actual immutable sources too, including while discovery has not
            // yet consumed this late pin within its two-unit allowance.
            for entry in old.snapshot.manifest.segments.iter() {
                assert!(ulmdb_storage::segment_path(&d, entry).is_file());
            }
            let state = e.maintenance.lock().unwrap();
            let scheduler = state.resumable.as_ref().unwrap();
            assert!(scheduler.state.stats.last_resumable_mark_units <= 2);
            if let Some(window) = &scheduler.state.window {
                assert!(!window.forward.contains(&checkpoint));
            }
        };
        for n in 22..=100u8 {
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"k", &[n; 12000]).unwrap();
            w.commit().unwrap();
            check_old();
        }
        // Stop introducing births, then require bounded discovery/marking to
        // complete a window while the late historical reader is still live.
        for _ in 0..4096 {
            if e.maintenance_status().unwrap().resumable_windows_completed > completed_before {
                break;
            }
            {
                let _writer = e.writer.lock().unwrap();
                e.automatic_reclaim();
            }
            check_old();
        }
        assert!(
            e.maintenance_status().unwrap().resumable_windows_completed > completed_before,
            "bounded maintenance did not complete after new births stopped"
        );
        drop(old);
        drop(e);
        fs::remove_dir_all(d).unwrap();
    }
}
