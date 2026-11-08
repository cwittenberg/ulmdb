//! Resumable inventory marking, followed by bounded name discovery/unlink.
//! The plan retains writer + capture exclusion; scheduling those fences and
//! resuming full logical graph rewriting are separate, unfinished work.
use super::{Candidate, Kind, Marks, ReclaimCursor, ReclaimReport, checked_selector as selector};
use crate::{Environment, Error, Result, retention};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    mem::ManuallyDrop,
    sync::{MutexGuard, atomic::Ordering},
};
use ulmdb_storage::{Manifest, MetadataOriginCursor, coordination::MaintenanceLease};

#[derive(Clone, Copy, Debug)]
pub struct ReclaimPlanLimits {
    pub max_roots: usize,
    /// Aggregate entries in distinct root inventories. One descriptor may be
    /// decoded before the charge is checked; existing codec bounds still apply.
    pub max_inventory_entries: usize,
    /// Distinct physical marks plus generation identities, all fixed-size keys.
    pub max_marks: usize,
}
impl Default for ReclaimPlanLimits {
    fn default() -> Self {
        Self {
            max_roots: 16_384,
            max_inventory_entries: 4_194_304,
            max_marks: 2_097_152,
        }
    }
}
impl ReclaimPlanLimits {
    fn validate(self) -> Result<()> {
        if self.max_roots == 0
            || self.max_roots > 65_536
            || self.max_inventory_entries == 0
            || self.max_inventory_entries > 16_777_216
            || self.max_marks == 0
            || self.max_marks > 16_777_216
        {
            return Err(Error::InvalidInput("invalid reclaim plan limits"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ReclaimStepBudget {
    /// One root, inventory entry or metadata-index node per unit.
    pub mark_units: usize,
    pub directory_entries: usize,
    pub unlink_files: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReclaimProgress {
    pub last_mark_units: usize,
    pub marked_units: u64,
    pub remaining_roots: usize,
    pub marking_complete: bool,
    pub complete: bool,
    /// Historical descriptors/cohorts deliberately kept for incremental lineage.
    pub preserved_history_files: usize,
    /// Exact selected decoded-inventory sum, not live bytes or a graph proof.
    pub rewrite_source_bytes: u64,
    pub rewrite_source_segments: usize,
    pub rewrite_estimate_complete: bool,
    pub report: ReclaimReport,
}
#[derive(Default)]
struct RootSet {
    roots: Vec<Manifest>,
    seen: BTreeSet<u128>,
    entries: usize,
}
impl RootSet {
    fn add(&mut self, m: &Manifest, limits: ReclaimPlanLimits, uuid: [u8; 16]) -> Result<()> {
        if m.uuid != uuid {
            return Err(Error::Corrupt("foreign reclaim plan root"));
        }
        if self.seen.contains(&m.generation) {
            return Ok(());
        }
        let entries = self
            .entries
            .checked_add(m.segments.len())
            .ok_or(Error::LimitExceeded)?;
        if self.roots.len() >= limits.max_roots || entries > limits.max_inventory_entries {
            return Err(Error::LimitExceeded);
        }
        self.roots
            .try_reserve(1)
            .map_err(|_| Error::LimitExceeded)?;
        self.roots.push(m.clone());
        self.entries = entries;
        self.seen.insert(m.generation);
        Ok(())
    }
}
struct State<'a> {
    roots: Vec<Manifest>,
    root: usize,
    stage: u8,
    segment: usize,
    origins: Option<MetadataOriginCursor>,
    marks: Marks,
    cursor: ReclaimCursor,
    limits: ReclaimPlanLimits,
    selected: Vec<u8>,
    previous: Option<Vec<u8>>,
    progress: ReclaimProgress,
    failed: bool,
    // Release mutable state before kernel exclusion, then the native writer.
    maintenance: MaintenanceLease,
    _writer: MutexGuard<'a, ()>,
}
/// Opaque, process-local plan tied to one live Environment. Drop cancels it.
/// No asset deletion occurs before complete marks; budget exhaustion is normal progress.
/// Writer and capture fences remain held until drop, including between steps.
pub struct ReclaimPlan<'a> {
    env: &'a Environment,
    creator: u32,
    state: ManuallyDrop<Option<State<'a>>>,
}
impl Drop for ReclaimPlan<'_> {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let state = std::mem::replace(&mut self.state, ManuallyDrop::new(None));
            drop(ManuallyDrop::into_inner(state));
        }
    }
}
impl Environment {
    /// Capture bounded root metadata once, then let the caller budget marking
    /// and sweeping. Constructor descriptor/registry IO is separate from steps.
    /// Linux coordinated writable files only; weak/debt selectors are deferred.
    pub fn begin_reclaim_plan(&self, limits: ReclaimPlanLimits) -> Result<ReclaimPlan<'_>> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        self.check_file_namespace()?;
        limits.validate()?;
        let writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        ulmdb_platform::sync_path(&self.directory)?;
        if self.directory.join("retained").try_exists()? {
            ulmdb_platform::sync_path(self.directory.join("retained"))?;
        }
        let current = self.current.load_full();
        let mut maintenance = self
            .coordination
            .as_ref()
            .ok_or(Error::InvalidInput("missing coordination"))?
            .try_maintenance()?;
        maintenance.check()?;
        let selected = selector(&self.directory, "CURRENT", false)?.unwrap();
        let expected = current
            .physical_pin
            .as_ref()
            .ok_or(Error::InvalidInput("missing current pin"))?
            .root()
            .checkpoint();
        if selected.len() == 64 {
            ulmdb_storage::checkpoint_identity(&selected)?;
            if selected.as_slice() != expected {
                self.poisoned.store(true, Ordering::Release);
                return Err(Error::OutcomeUnknown);
            }
        }
        // Preflight authoritative legacy PREVIOUS before an ordinary metadata
        // decoder could open a static FIFO. This is identity comparison only;
        // the existing codecs below still authenticate both records.
        let external_previous = selected.len() == 64 && selected.starts_with(b"ULMDBCUR");
        let previous_bytes = if external_previous {
            selector(&self.directory, "PREVIOUS", true)?
        } else {
            None
        };
        let (durable, previous) = ulmdb_storage::read_roots_checked_with_transform(
            &self.directory,
            |d| {
                if d.uuid != current.manifest.uuid
                    || ulmdb_storage::checkpoint_bytes(d)? != *expected
                {
                    self.poisoned.store(true, Ordering::Release);
                    return Err(Error::OutcomeUnknown);
                }
                Ok(())
            },
            current.segments.transform().cloned(),
        )?;
        // Existing codecs, including head-drift priority, run before deferral.
        if selected.len() == 176 {
            return Err(Error::Busy);
        }
        if durable.format_minor < 3
            && durable.previous != 0
            && previous.as_ref().map(|m| m.generation) != Some(durable.previous)
            && (self
                .directory
                .join("commits")
                .join(format!("{:032x}.root", durable.previous))
                .try_exists()?
                || self
                    .directory
                    .join("cohorts")
                    .join(format!("{:032x}.cohort", durable.previous))
                    .try_exists()?)
        {
            return Err(Error::Busy);
        }
        if selector(&self.directory, "CURRENT", false)?.as_ref() != Some(&selected) {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        if external_previous && selector(&self.directory, "PREVIOUS", true)? != previous_bytes {
            return Err(Error::Corrupt("reclaim predecessor changed during capture"));
        }
        let mut roots = RootSet::default();
        roots.add(&durable, limits, durable.uuid)?;
        if let Some(p) = &previous {
            roots.add(p, limits, durable.uuid)?;
        }
        let mut local = Vec::new();
        {
            let mut retired = self.retired.lock().map_err(|_| Error::Busy)?;
            if retired.len() > limits.max_roots {
                return Err(Error::LimitExceeded);
            }
            local
                .try_reserve(retired.len())
                .map_err(|_| Error::LimitExceeded)?;
            retired.retain(|weak| {
                if let Some(r) = weak.upgrade() {
                    local.push(r);
                    true
                } else {
                    false
                }
            });
        }
        for r in &local {
            roots.add(&r.manifest, limits, durable.uuid)?;
        }
        let pins = maintenance.scan()?;
        for r in pins.live_roots() {
            roots.add(r.manifest(), limits, durable.uuid)?;
        }
        pins.cleanup_stale()?;
        let retained = retention::pins_bounded(
            self,
            4096,
            limits.max_inventory_entries,
            limits.max_roots.saturating_mul(16).saturating_add(64),
        )?;
        for (_, r) in retained {
            roots.add(&r, limits, durable.uuid)?;
        }
        let count = roots.roots.len();
        let plan = ReclaimPlan {
            env: self,
            creator: std::process::id(),
            state: ManuallyDrop::new(Some(State {
                roots: roots.roots,
                root: 0,
                stage: 0,
                segment: 0,
                origins: None,
                marks: Marks::default(),
                cursor: ReclaimCursor::default(),
                limits,
                selected,
                previous: previous_bytes,
                progress: ReclaimProgress {
                    remaining_roots: count,
                    ..Default::default()
                },
                failed: false,
                maintenance,
                _writer: writer,
            })),
        };
        plan.check()?;
        Ok(plan)
    }
}
impl ReclaimPlan<'_> {
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited reclaim plan"));
        }
        let state = self
            .state
            .as_ref()
            .ok_or(Error::InvalidInput("finished reclaim plan"))?;
        if state.failed {
            return Err(Error::InvalidInput("failed reclaim plan must drop"));
        }
        if self.env.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        verify_selection(self.env, state)
    }
    /// Placement reuse shares this plan's complete authenticated root discovery,
    /// writer fence and Capture EX; no extra Capture acquisition is attempted.
    pub fn reclaim_fixed_map_slots(
        &mut self,
        limits: ulmdb_storage::fixed_map::FixedSlotReclaimLimits,
    ) -> Result<ulmdb_storage::fixed_map::FixedSlotReclaimReport> {
        self.check()?;
        let state = self.state.as_mut().unwrap();
        let selected = &state.roots[0];
        let declaration = ulmdb_storage::fixed_map::FixedMapDeclaration::open(
            &self.env.directory,
            selected.uuid,
            selected.epoch,
        )?;
        let report = declaration.reclaim_slots(&state.maintenance, &state.roots, limits)?;
        if let Err(error) = verify_selection(self.env, state) {
            state.failed = true;
            return Err(error);
        }
        Ok(report)
    }
    pub fn progress(&self) -> Result<ReclaimProgress> {
        self.check()?;
        Ok(self.state.as_ref().unwrap().progress.clone())
    }
    pub fn step(&mut self, budget: ReclaimStepBudget) -> Result<ReclaimProgress> {
        self.check()?;
        if budget.mark_units > 65_536
            || budget.directory_entries > 65_536
            || budget.unlink_files > 4096
            || budget.mark_units | budget.directory_entries | budget.unlink_files == 0
        {
            return Err(Error::InvalidInput("invalid reclaim step budget"));
        }
        let env = self.env;
        let state = self.state.as_mut().unwrap();
        let result = state.advance(env, budget);
        if result.is_err() {
            state.failed = true;
        }
        result?;
        self.check()?;
        Ok(self.state.as_ref().unwrap().progress.clone())
    }
}
fn verify_selection(env: &Environment, state: &State<'_>) -> Result<()> {
    env.check_file_namespace()?;
    state.maintenance.check()?;
    if env.directory.canonicalize()? != env.directory {
        return Err(Error::Corrupt("reclaim namespace alias"));
    }
    if selector(&env.directory, "CURRENT", false)?.as_ref() != Some(&state.selected) {
        env.poisoned.store(true, Ordering::Release);
        return Err(Error::OutcomeUnknown);
    }
    if state.selected.len() == 64
        && state.roots[0].format_minor < 3
        && selector(&env.directory, "PREVIOUS", true)? != state.previous
    {
        return Err(Error::Corrupt("reclaim predecessor changed"));
    }
    Ok(())
}
impl State<'_> {
    fn capacity(&self) -> Result<()> {
        if self.marks.roots.len()
            + self.marks.segments.len()
            + self.marks.cohorts.len()
            + self.marks.generations.len()
            >= self.limits.max_marks
        {
            Err(Error::LimitExceeded)
        } else {
            Ok(())
        }
    }
    fn root_file(&mut self, generation: u128, cohort: bool) -> Result<()> {
        let exists = if cohort {
            self.marks.cohorts.contains(&generation)
        } else {
            self.marks.roots.contains(&generation)
        };
        if !exists {
            self.capacity()?;
            if cohort {
                self.marks.cohorts.insert(generation);
            } else {
                self.marks.roots.insert(generation);
            }
        }
        Ok(())
    }
    fn advance(&mut self, env: &Environment, budget: ReclaimStepBudget) -> Result<()> {
        self.progress.last_mark_units = 0;
        while self.root < self.roots.len() && self.progress.last_mark_units < budget.mark_units {
            match self.stage {
                0 => {
                    let generation = self.roots[self.root].generation;
                    self.root_file(generation, self.roots[self.root].format_minor >= 3)?;
                    self.capacity()?;
                    self.marks.generations.insert(generation);
                    self.stage = 1;
                }
                1 => {
                    if let Some(entry) = self.roots[self.root].segments.get(self.segment).cloned() {
                        if entry.cohort == 0 {
                            if !self.marks.segments.contains(&entry.hash) {
                                self.capacity()?;
                                self.marks.segments.insert(entry.hash);
                            }
                        } else {
                            self.root_file(entry.cohort, true)?;
                        }
                        if self.root == 0 {
                            self.progress.rewrite_source_bytes = self
                                .progress
                                .rewrite_source_bytes
                                .checked_add(entry.length)
                                .ok_or(Error::LimitExceeded)?;
                            self.progress.rewrite_source_segments += 1;
                        }
                        self.segment += 1;
                    } else {
                        if self.root == 0 {
                            self.progress.rewrite_estimate_complete = true;
                        }
                        self.origins = Some(self.roots[self.root].metadata_origin_cursor()?);
                        self.stage = 2;
                        continue;
                    }
                }
                2 => {
                    if let Some(origin) = self.origins.as_mut().unwrap().next_origin()? {
                        self.root_file(origin, true)?;
                    } else {
                        self.origins = None;
                        self.root += 1;
                        self.segment = 0;
                        self.stage = 0;
                        self.progress.remaining_roots = self.roots.len() - self.root;
                        continue;
                    }
                }
                _ => unreachable!(),
            }
            self.progress.last_mark_units += 1;
            self.progress.marked_units += 1;
        }
        self.progress.marking_complete = self.root == self.roots.len();
        self.progress.report.retained_roots = self.marks.generations.len();
        self.progress.report.retained_segment_files = self.marks.segments.len();
        self.progress.report.retained_cohort_files = self.marks.cohorts.len();
        if !self.progress.marking_complete
            || self.progress.complete
            || budget.directory_entries == 0
            || budget.unlink_files == 0
        {
            return Ok(());
        }
        let mut remove = Vec::<Candidate>::new();
        let mut scanned = 0;
        let complete = self.cursor.collect(
            &env.directory,
            &self.marks,
            &mut remove,
            budget.unlink_files,
            budget.directory_entries,
            &mut scanned,
        )?;
        // Existing incremental exports authenticate intermediate descriptors.
        // Keeping only their endpoint root is insufficient. Until independent
        // compact history exists, retain all descriptor/cohort containers.
        remove.retain(|c| {
            if matches!(c.kind, Kind::Descriptor | Kind::Cohort) {
                self.progress.preserved_history_files += 1;
                false
            } else {
                true
            }
        });
        let total = remove.iter().try_fold(0u64, |n, c| {
            n.checked_add(c.bytes).ok_or(Error::LimitExceeded)
        })?;
        self.progress
            .report
            .removed_bytes
            .checked_add(total)
            .ok_or(Error::LimitExceeded)?;
        verify_selection(env, self)?;
        let mut changed = BTreeSet::new();
        for c in remove {
            fs::remove_file(&c.path)?;
            changed.insert(
                c.path
                    .parent()
                    .ok_or(Error::Corrupt("reclaim candidate parent"))?
                    .to_path_buf(),
            );
            self.progress.report.removed_bytes += c.bytes;
            match c.kind {
                Kind::Segment => self.progress.report.removed_segments += 1,
                Kind::Descriptor => self.progress.report.removed_descriptors += 1,
                Kind::Cohort => self.progress.report.removed_cohorts += 1,
                Kind::Temporary => self.progress.report.removed_temporary_files += 1,
            }
        }
        for p in changed {
            ulmdb_platform::sync_path(p)?;
        }
        self.progress.report.scanned_directory_entries += scanned;
        self.progress.report.enumeration_complete = complete;
        self.progress.complete = complete;
        Ok(())
    }
}
