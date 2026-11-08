//! Bounded commit-time file maintenance. Rebuilding introduces no
//! additional publication: real previous, business sequence and commit survive.
use crate::{DatabaseKind, Environment, Error, Result, WriteTxn, catalog::Database};
use std::{cell::Cell, collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::{ReadStore, Reference, WriteStore};
use ulmdb_storage::{Manifest, SegmentBuilder, SegmentTable};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod resumable;
mod transfer;
pub use transfer::MaintenanceTransfer;
mod rewrite;
pub(crate) use rewrite::Progress as RewriteProgress;
mod fallback;

/// Per-owner policy. Ordinary coordinated Linux, macOS and Windows file owners start with AUTOMATIC.
/// Rebuild streams cursor paths, builder frontiers and one record at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaintenancePolicy {
    #[default]
    Disabled,
    Bounded {
        /// Trigger when base inventory plus a private segment reaches this count.
        segment_threshold: usize,
        /// Maximum source inventory + private-arena bytes eligible for rewriting.
        /// Traversal and output allocation cap at 128x and 4x this amount.
        /// Larger inventories defer; choose a budget covering the source inventory.
        rewrite_budget_bytes: u64,
        /// Maximum recognized obsolete files unlinked per publishing commit.
        reclaim_budget_files: usize,
    },
    /// Coordinated Legacy only. Carries authenticated inventory marking
    /// across commits, including bounded protected-root discovery.
    /// Preserves every descriptor/cohort for exact incremental lineage. Does not
    /// reclaim cohort payload containers. Recognized orphan names are discovered
    /// under fresh root coverage; unrecognized operator files are preserved.
    LegacyResumable {
        segment_threshold: usize,
        /// Encoded object read/copy allowance per publishing commit. Rewriting
        /// advances across inventories larger than this allowance, inspecting
        /// at most1024 uncached typed objects per step. Local codec validation,
        /// catalog fixup and final inventory projection have separate costs.
        /// Oversized opaque payloads keep their containing segment; an indivisible
        /// typed node larger than the allowance defers that frontier.
        rewrite_budget_bytes: u64,
        reclaim: ResumableReclaimBudget,
    },
    /// Legacy/Cohort/Indexed retirement, archiving exact historical metadata
    /// before deleting an unreferenced cohort's payload container.
    CrossFormatResumable {
        segment_threshold: usize,
        /// Same incremental object allowance as LegacyResumable, including
        /// temporary read scopes for bounded cached owners.
        rewrite_budget_bytes: u64,
        reclaim: ResumableReclaimBudget,
        history_budget_bytes: u64,
    },
}
impl MaintenancePolicy {
    /// Default for ordinary coordinated Linux, macOS and Windows file owners. Physical payload
    /// reuse is automatic; exact historical metadata remains KeepAll unless
    /// an explicit finite history policy is configured.
    pub const AUTOMATIC: Self = Self::CrossFormatResumable {
        segment_threshold: 128, rewrite_budget_bytes: 16 << 20,
        reclaim: ResumableReclaimBudget { mark_units: 256, unlink_files: 8,
            max_roots: 4096, max_inventory_entries: 1_048_575, max_candidates: 65_536 },
        history_budget_bytes: 16 << 20,
    };
}
/// Per-commit discovery/marking and unlink allowances. Each metadata work unit
/// admits at most8KiB of descriptor input; fixed header/identity queries are
/// charged by the corresponding registry/source work unit. Registry and aggregate
/// inventory limits also cap retained discovery state across commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumableReclaimBudget {
    pub mark_units: usize,
    pub unlink_files: usize,
    pub max_roots: usize,
    pub max_inventory_entries: usize,
    pub max_candidates: usize,
}
impl Default for ResumableReclaimBudget {
    fn default() -> Self {
        Self {
            mark_units: 256,
            unlink_files: 16,
            max_roots: 4096,
            max_inventory_entries: 1_048_575,
            max_candidates: 65_536,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceStatus {
    pub scanned_directory_entries: u64,
    pub last_scanned_directory_entries: usize,
    pub incomplete_directory_scans: u64,
    pub rebuilt_candidates: u64,
    pub deferred_rebuilds: u64,
    pub reclaimed_files: u64,
    pub reclaimed_bytes: u64,
    pub deferred_reclaims: u64,
    /// Cleanup after successful selection is best effort, retried on the next
    /// publishing commit. Its failure never disguises a successful commit.
    pub last_reclaim_error: Option<String>,
    pub resumable_marked_units: u64,
    pub last_resumable_mark_units: usize,
    pub resumable_windows_started: u64,
    pub resumable_windows_completed: u64,
    pub resumable_nonforward_resets: u64,
    pub resumable_retention_changes: u64,
    pub resumable_pending_candidates: usize,
    pub resumable_pending_roots: usize,
    /// Authenticated roots completed by incremental protected-root discovery.
    pub resumable_discovered_roots: u64,
    pub resumable_discovered_inventory_entries: u64,
    pub resumable_deferred_events: u64,
    pub resumable_released_snapshot_pins: u64,
    pub resumable_archived_cohorts: u64,
    pub resumable_archived_metadata_bytes: u64,
    pub incremental_rewrite_steps: u64,
    pub incremental_rewrite_objects: u64,
    pub incremental_rewrite_bytes: u64,
    pub fallback_rewrite_attempts: u64,
    pub fallback_rewrite_deferred: u64,
    pub last_fallback_rewrite_error: Option<String>,
    pub last_rewrite_kept_segments: usize,
    /// Placement metadata only; actual data-file cleanup has separate counters.
    pub fixed_slot_reclaim_passes: u64,
    pub fixed_slots_removed: u64,
    pub fixed_slot_bytes_released: u64,
    pub fixed_slots_busy: u64,
}
#[derive(Default)]
pub(crate) struct MaintenanceState {
    policy: MaintenancePolicy,
    status: MaintenanceStatus,
    reclaim_cursor: Option<crate::reclaim::ReclaimCursor>,
    rewrite: Option<RewriteProgress>,
    fallback: bool,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    resumable: Option<resumable::Scheduler>,
}
impl MaintenanceState {
    pub(crate) fn for_file_owner(enabled: bool) -> Self {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if enabled {
            return Self { policy: MaintenancePolicy::AUTOMATIC, fallback: true,
                resumable: Some(resumable::Scheduler::new(Default::default(), true)),
                ..Default::default() };
        }
        let _ = enabled;
        Self::default()
    }
    pub(crate) fn install_rewrite(&mut self, progress: Option<RewriteProgress>) {
        self.rewrite = progress;
    }
}
impl Environment {
    /// Coordinated writable file owners only. Range/replicas are excluded; pending
    /// PreviousSession intentionally exposes no maintenance-policy setter.
    pub fn set_maintenance_policy(&self, policy: MaintenancePolicy) -> Result<()> {
        self.set_maintenance_policy_inner(policy, false)
    }
    /// Prepare an ordinary business candidate first. Optional rewriting is a
    /// separately admitted alternative; any failure keeps the ordinary ready
    /// candidate available for the same logical transaction and previous root.
    pub fn set_maintenance_policy_with_fallback(&self, policy: MaintenancePolicy) -> Result<()> {
        if matches!(policy, MaintenancePolicy::Bounded { .. }) {
            return Err(Error::InvalidInput("fallback requires resumable maintenance"));
        }
        self.set_maintenance_policy_inner(policy, true)
    }
    fn set_maintenance_policy_inner(&self, policy: MaintenancePolicy, fallback: bool) -> Result<()> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(Error::InvalidInput(
                "automatic maintenance requires coordinated physical pins",
            ));
        }
        let (segment_threshold, rewrite_budget_bytes, reclaim_budget_files) = match policy {
            MaintenancePolicy::Disabled => (1, 4096, 1),
            MaintenancePolicy::Bounded {
                segment_threshold,
                rewrite_budget_bytes,
                reclaim_budget_files,
            } => (
                segment_threshold,
                rewrite_budget_bytes,
                reclaim_budget_files,
            ),
            MaintenancePolicy::LegacyResumable {
                segment_threshold,
                rewrite_budget_bytes,
                reclaim,
            }
            | MaintenancePolicy::CrossFormatResumable {
                segment_threshold, rewrite_budget_bytes, reclaim, ..
            } => {
                let base = self.current.load();
                let cross = matches!(policy, MaintenancePolicy::CrossFormatResumable { .. });
                if base.manifest.format_minor > 4
                    || (!cross && base.manifest.format_minor >= 3) {
                    return Err(Error::InvalidInput(
                        "resumable automatic reclaim requires an admitted file format",
                    ));
                }
                if let MaintenancePolicy::CrossFormatResumable { history_budget_bytes, .. } = policy {
                    if history_budget_bytes < 256 || history_budget_bytes >
                        ulmdb_storage::MAX_DESCRIPTOR_BYTES + ulmdb_storage::MAX_DESCRIPTOR_BYTES / 4 + 8320 {
                        return Err(Error::InvalidInput("invalid descriptor archive budget"));
                    }
                }
                if reclaim.mark_units < 2
                    || reclaim.mark_units > 65_536
                    || reclaim.max_roots == 0
                    || reclaim.max_roots > 16_384
                    || reclaim.max_inventory_entries == 0
                    || reclaim.max_inventory_entries > 4_194_304
                    || reclaim.max_candidates == 0
                    || reclaim.max_candidates > 1_048_576
                {
                    return Err(Error::InvalidInput("invalid resumable reclaim budget"));
                }
                (
                    segment_threshold,
                    rewrite_budget_bytes,
                    reclaim.unlink_files,
                )
            }
        };
        {
            if segment_threshold == 0
                || rewrite_budget_bytes < 4096
                || rewrite_budget_bytes > u64::MAX / 128
                || reclaim_budget_files == 0
                || reclaim_budget_files > 4096
            {
                return Err(Error::InvalidInput("invalid automatic maintenance budget"));
            }
        }
        if !self.poisoned.load(std::sync::atomic::Ordering::Acquire) {
            self.check_file_namespace()?;
        }
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.check_file_namespace()?;
        {
            let mut state = self.maintenance.lock().map_err(|_| Error::Busy)?;
            state.policy = policy;
            state.fallback = fallback;
            state.reclaim_cursor = None;
            state.rewrite = None;
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            {
                let status = state.status.clone();
                state.resumable = matches!(policy, MaintenancePolicy::LegacyResumable { .. }
                    | MaintenancePolicy::CrossFormatResumable { .. })
                    .then(|| resumable::Scheduler::new(status,
                        matches!(policy, MaintenancePolicy::CrossFormatResumable { .. })));
            }
        }
        self.maintenance_enabled.store(
            policy != MaintenancePolicy::Disabled,
            std::sync::atomic::Ordering::Release,
        );
        Ok(())
    }
    pub fn maintenance_policy(&self) -> Result<MaintenancePolicy> {
        if self
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.check_file_namespace()?;
        }
        Ok(self.maintenance.lock().map_err(|_| Error::Busy)?.policy)
    }
    pub fn maintenance_status(&self) -> Result<MaintenanceStatus> {
        if self
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.check_file_namespace()?;
        }
        Ok(self
            .maintenance
            .lock()
            .map_err(|_| Error::Busy)?
            .status
            .clone())
    }
    // Only the consuming PreparedWriteTxn with its original WriterGuard calls.
    pub(crate) fn automatic_reclaim(&self) {
        if !self
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        let Ok(mut state) = self.maintenance.lock() else {
            return;
        };
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let MaintenancePolicy::LegacyResumable { reclaim, .. }
            | MaintenancePolicy::CrossFormatResumable { reclaim, .. } = state.policy {
            let history_budget = match state.policy {
                MaintenancePolicy::CrossFormatResumable { history_budget_bytes, .. } => history_budget_bytes,
                _ => 0,
            };
            let Some(mut scheduler) = state.resumable.take() else {
                return;
            };
            drop(state);
            let result = scheduler.step(self, reclaim, history_budget);
            if let Ok(mut state) = self.maintenance.lock() {
                scheduler.report(&mut state.status);
                if let Err(error) = result {
                    state.status.deferred_reclaims =
                        state.status.deferred_reclaims.saturating_add(1);
                    state.status.last_reclaim_error = Some(error.to_string());
                } else {
                    state.status.last_reclaim_error = None;
                }
                state.resumable = Some(scheduler);
            }
            return;
        }
        let MaintenancePolicy::Bounded {
            reclaim_budget_files,
            ..
        } = state.policy
        else {
            return;
        };
        let mut cursor = state.reclaim_cursor.take().unwrap_or_default();
        drop(state);
        // Bound discovery separately from deletions: a directory containing
        // only protected/unknown names must not be rescanned in full each time.
        let scan_budget = reclaim_budget_files.saturating_mul(16).saturating_add(64);
        // Never remove recovery assets under weak selection or data debt.
        let result = match self.cached_file_durability() {
            Ok(Some(state)) if !state.is_weak() && !state.has_data_debt() => {
                self.reclaim_bounded_under_writer(reclaim_budget_files, &mut cursor, scan_budget)
            }
            Ok(_) => Err(Error::Busy),
            Err(error) => Err(error),
        };
        if let Ok(mut state) = self.maintenance.lock() {
            state.reclaim_cursor = Some(cursor);
            match result {
                Ok(report) => {
                    state.status.last_scanned_directory_entries = report.scanned_directory_entries;
                    state.status.scanned_directory_entries = state
                        .status
                        .scanned_directory_entries
                        .saturating_add(report.scanned_directory_entries as u64);
                    if !report.enumeration_complete {
                        state.status.incomplete_directory_scans =
                            state.status.incomplete_directory_scans.saturating_add(1);
                    }
                    let files = report.removed_segments
                        + report.removed_descriptors
                        + report.removed_cohorts
                        + report.removed_temporary_files;
                    state.status.reclaimed_files =
                        state.status.reclaimed_files.saturating_add(files as u64);
                    state.status.reclaimed_bytes = state
                        .status
                        .reclaimed_bytes
                        .saturating_add(report.removed_bytes);
                    state.status.last_reclaim_error = None;
                }
                Err(error) => {
                    state.status.deferred_reclaims =
                        state.status.deferred_reclaims.saturating_add(1);
                    state.status.last_reclaim_error = Some(error.to_string());
                }
            }
        }
    }
}
impl Environment {
    pub(crate) fn maintenance_reset_rewrite(&self) {
        if self.maintenance_enabled.load(std::sync::atomic::Ordering::Acquire) {
            if let Ok(mut state) = self.maintenance.lock() { state.rewrite = None; }
        }
    }
    pub(crate) fn maintenance_before_write(&self) {
        if !self
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        if let Ok(mut state) = self.maintenance.lock() {
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            if let Some(s) = state.resumable.as_mut() {
                s.release_optional_pin();
            }
        }
        // Prepared/aborted candidates can leave unselected placement rows even
        // without a successful activation. The caller already holds the writer;
        // pressure-only cleanup rediscovers complete roots under fresh Capture.
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if self.reusable_fixed_map_pressure().unwrap_or(false) {
            self.automatic_fixed_slots_before_write();
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn automatic_fixed_slots_before_write(&self) {
        let Ok(mut state) = self.maintenance.lock() else { return; };
        let reclaim = match state.policy {
            MaintenancePolicy::LegacyResumable { reclaim, .. }
            | MaintenancePolicy::CrossFormatResumable { reclaim, .. } => reclaim,
            _ => return,
        };
        let Some(mut scheduler) = state.resumable.take() else { return; };
        drop(state);
        let result = scheduler.prune_fixed_slots_before_write(self, reclaim);
        if let Ok(mut state) = self.maintenance.lock() {
            scheduler.report(&mut state.status);
            if let Err(error) = result {
                state.status.deferred_reclaims = state.status.deferred_reclaims.saturating_add(1);
                state.status.last_reclaim_error = Some(error.to_string());
            } else {
                state.status.last_reclaim_error = None;
            }
            state.resumable = Some(scheduler);
        }
    }
    pub(crate) fn maintenance_activation(
        &self,
        before: &crate::environment::Snapshot,
        after: &crate::environment::Snapshot,
        forward: bool,
        retired_inventory: bool,
    ) {
        if !self
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        if let Ok(mut state) = self.maintenance.lock() {
            if !forward { state.rewrite = None; }
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            if let Some(s) = state.resumable.as_mut() {
                s.activation(before, after, forward, retired_inventory);
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let _ = (before, after, forward, retired_inventory);
    }
    pub(crate) fn maintenance_retention_changed(&self) {
        if !self
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Ok(mut state) = self.maintenance.lock() {
            if let Some(s) = state.resumable.as_mut() {
                s.retention_changed();
            }
        }
    }
}

struct Source<'a> {
    inner: &'a ulmdb_storage::Arena,
    remaining: Cell<u64>,
}
impl ReadStore for Source<'_> {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.remaining.set(
            self.remaining
                .get()
                .checked_sub(length as u64)
                .ok_or(Error::LimitExceeded)?,
        );
        self.inner.read(at, length)
    }
}
struct Target {
    inner: SegmentBuilder,
    remaining: u64,
}
impl ReadStore for Target {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.inner.read(at, length)
    }
}
impl WriteStore for Target {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.charge(bytes.len(), alignment)?;
        self.inner.allocate(bytes, alignment)
    }
    fn rewrite(&mut self, at: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.charge(bytes.len(), alignment)?;
        self.inner.rewrite(at, bytes, alignment)
    }
}
impl Target {
    fn charge(&mut self, bytes: usize, alignment: usize) -> Result<()> {
        let amount = bytes.checked_add(alignment).ok_or(Error::LimitExceeded)?;
        self.remaining = self
            .remaining
            .checked_sub(amount as u64)
            .ok_or(Error::LimitExceeded)?;
        Ok(())
    }
}
type Rebuilt = (SegmentTable, BTreeMap<Vec<u8>, Database>);
impl WriteTxn<'_> {
    pub(crate) fn rebuild_for_maintenance(
        &self,
        manifest: &mut Manifest,
        data_policy: crate::Durability,
    ) -> Result<Option<Rebuilt>> {
        if !self
            .env
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(None);
        }
        let mut candidate = manifest.clone_metadata();
        match self.try_rebuild_for_maintenance(&mut candidate, data_policy) {
            Ok(Some(rebuilt)) => {
                *manifest = candidate;
                Ok(Some(rebuilt))
            }
            Ok(None) => Ok(None),
            Err(Error::LimitExceeded) => {
                // Maintenance must not turn an otherwise valid write into a
                // quota failure. Discard the unpublished rebuild; the original
                // arena and its candidate metadata remain available for sealing.
                let mut state = self.env.maintenance.lock().map_err(|_| Error::Busy)?;
                state.status.deferred_rebuilds = state.status.deferred_rebuilds.saturating_add(1);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    fn try_rebuild_for_maintenance(
        &self,
        manifest: &mut Manifest,
        data_policy: crate::Durability,
    ) -> Result<Option<Rebuilt>> {
        if !self
            .env
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(None);
        }
        let (segment_threshold, rewrite_budget_bytes) = match self.env.maintenance_policy()? {
            MaintenancePolicy::Bounded {
                segment_threshold,
                rewrite_budget_bytes,
                ..
            } => (segment_threshold, rewrite_budget_bytes),
            MaintenancePolicy::Disabled | MaintenancePolicy::LegacyResumable { .. }
                | MaintenancePolicy::CrossFormatResumable { .. } => return Ok(None),
        };
        if self
            .base
            .manifest
            .segments
            .len()
            .saturating_add(usize::from(self.arena.dirty_bytes() != 0))
            < segment_threshold
        {
            return Ok(None);
        }
        let mut input_bytes = self.arena.dirty_bytes() as u64 + 4096;
        // Do not traverse the remaining inventory after the eligible byte
        // budget is exhausted. Every admitted segment contains at least4096
        // bytes, so even planning work is capped by this explicit allowance.
        if input_bytes <= rewrite_budget_bytes {
            for entry in &self.base.manifest.segments {
                input_bytes = input_bytes
                    .checked_add(entry.length)
                    .ok_or(Error::LimitExceeded)?;
                if input_bytes > rewrite_budget_bytes {
                    break;
                }
            }
        }
        if input_bytes > rewrite_budget_bytes {
            let mut state = self.env.maintenance.lock().map_err(|_| Error::Busy)?;
            state.status.deferred_rebuilds = state.status.deferred_rebuilds.saturating_add(1);
            return Ok(None);
        }
        #[cfg(feature = "fault-injection")]
        ulmdb_storage::injected_fault("maintenance.before_rebuild")?;
        let source = Source {
            inner: &self.arena,
            remaining: Cell::new(rewrite_budget_bytes * 128),
        };
        let mut target = Target {
            inner: SegmentBuilder::new_with_geometry_and_transform(
                &self.env.directory,
                self.base.manifest.next_segment,
                self.base.segments.read_ahead(),
                manifest.page_size,
                self.base.segments.transform().cloned(),
            )?
            .with_value_limit(manifest.value_limit)
            .with_durability(data_policy),
            remaining: rewrite_budget_bytes * 4,
        };
        let mut catalog = BTreeMap::new();
        let mut encoded = ulmdb_kv::SortedBuilder::new();
        if let Some(bytes) = (self.catalog_capacity).marker() {
            encoded.push(&mut target, b"", &bytes)?;
        }
        for (name, old) in &self.catalog {
            let root = match old.kind {
                DatabaseKind::Kv => {
                    ulmdb_kv::rebuild_inorder(
                        &mut target,
                        ulmdb_kv::Cursor::new(
                            &source,
                            old.root,
                            Bound::Unbounded,
                            Bound::Unbounded,
                            false,
                        )?,
                    )?
                    .root
                }
                DatabaseKind::Duplicates { width } => {
                    crate::duplicates::copy_buckets(&source, old.root, &mut target, width)?.0
                }
                DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                    let address = if old.kind == DatabaseKind::Ipv4 {
                        crate::Address::V4(0)
                    } else {
                        crate::Address::V6(0)
                    };
                    let all = crate::Prefix::new(address, 0)
                        .map_err(|_| Error::Corrupt("maintenance prefix family"))?;
                    ulmdb_prefix::build_sorted(
                        &mut target,
                        ulmdb_prefix::subnets(&source, old.root, all)?,
                    )?
                    .root
                }
            };
            let next = Database {
                root,
                ..old.clone()
            };
            encoded.push(
                &mut target,
                name,
                &next.encode_for_profile(manifest.page_size, manifest.value_limit),
            )?;
            catalog.insert(name.clone(), next);
        }
        manifest.catalog = encoded.finish(&mut target)?.root;
        let (segments, next) = target.inner.finish()?;
        manifest.next_segment = next;
        manifest.segments = segments.entries().into();
        if manifest.format_minor == 4 {
            manifest.prepare_indexed(None)?;
        }
        crate::backup::validate_for_admission(&segments, manifest)?;
        #[cfg(feature = "fault-injection")]
        ulmdb_storage::injected_fault("maintenance.after_rebuild")?;
        let mut state = self.env.maintenance.lock().map_err(|_| Error::Busy)?;
        state.status.rebuilt_candidates = state.status.rebuilt_candidates.saturating_add(1);
        Ok(Some((segments, catalog)))
    }
}
