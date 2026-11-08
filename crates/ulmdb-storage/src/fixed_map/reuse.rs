//! Opt-in v2 slots. Native callers retain writer and Capture EX while deriving
//! the complete immutable root set. Kernel range leases protect actual VMAs,
//! even after their snapshot/physical-pin owners have otherwise disappeared.
use super::*;
use ulmdb_platform::{LockMode, RangeLock};
pub(super) const MAGIC_V2: &[u8; 8] = b"ULMDBFM2";
const LOCK_BASE: u64 = 1 << 40;

/// Inventory access without materializing a decoded descriptor into a flat Vec.
/// The caller still supplies complete, freshly protected root coverage.
pub trait FixedSlotRoot {
    fn uuid(&self) -> [u8; 16];
    fn epoch(&self) -> u64;
    fn inventory_len(&self) -> usize;
    fn inventory_entry(&self, index: usize) -> Option<&crate::SegmentEntry>;
}
impl FixedSlotRoot for crate::Manifest {
    fn uuid(&self) -> [u8; 16] { self.uuid }
    fn epoch(&self) -> u64 { self.epoch }
    fn inventory_len(&self) -> usize { self.segments.len() }
    fn inventory_entry(&self, index: usize) -> Option<&crate::SegmentEntry> { self.segments.get(index) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LeaseKind {
    NativeLock,
    Dedicated,
}
impl LeaseKind {
    fn name(self) -> &'static str {
        match self {
            Self::NativeLock => "LOCK",
            Self::Dedicated => "FIXEDMAP-LEASES",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct LeaseIdentity {
    pub volume: u64,
    pub file_id: u128,
    pub kind: LeaseKind,
}
pub(super) fn fresh_version() -> u16 {
    if cfg!(target_os = "linux") { 2 } else { 3 }
}
pub(super) fn target_version(source: u16) -> u16 {
    if source == 2 && !cfg!(target_os = "linux") {
        3
    } else {
        source
    }
}
fn identity_for(directory: &Path, kind: LeaseKind) -> Result<LeaseIdentity> {
    let file = lock_file(directory, kind, false)?;
    let (volume, file_id) = host::identity(&file)?;
    Ok(LeaseIdentity {
        volume,
        file_id,
        kind,
    })
}
#[cfg(test)]
pub(super) fn lock_identity(directory: &Path) -> Result<LeaseIdentity> {
    identity_for(directory, LeaseKind::NativeLock)
}
pub(super) fn create_identity(directory: &Path, version: u16) -> Result<LeaseIdentity> {
    let kind = match version {
        2 => LeaseKind::NativeLock,
        3 => LeaseKind::Dedicated,
        _ => return Err(Error::Corrupt("fixed reuse version")),
    };
    if kind == LeaseKind::Dedicated {
        let path = directory.join(kind.name());
        match crate::access::options_for_entry(&path)?
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                check_file(&path, &file)?;
                ulmdb_platform::sync_file(&file)?;
                ulmdb_platform::sync_directory(directory)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Stable inode, no truncate/replace. A previous failed creation
                // may have left this empty private coordination object behind.
                let file = lock_file(directory, kind, true)?;
                ulmdb_platform::sync_file(&file)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    identity_for(directory, kind)
}
pub(super) fn check_identity(directory: &Path, identity: Option<LeaseIdentity>) -> Result<()> {
    if let Some(expected) = identity {
        if cfg!(target_os = "macos") && expected.kind == LeaseKind::NativeLock {
            return Err(Error::InvalidInput(
                "v2 fixed LOCK protocol is not compatible with macOS writer flock",
            ));
        }
        if identity_for(directory, expected.kind)? != expected {
            return Err(Error::Corrupt("fixed slot lease namespace changed"));
        }
    }
    Ok(())
}
fn lock_file(directory: &Path, kind: LeaseKind, write: bool) -> Result<File> {
    let path = directory.join(kind.name());
    let file = host::open_regular(&path, write)?;
    check_file(&path, &file)?;
    Ok(file)
}
fn check_file(path: &Path, file: &File) -> Result<()> {
    host::check_regular(path, file)?;
    let named = host::named(path)?;
    let held = ulmdb_platform::file_identity(file)?;
    if held.links != 1 || named.links != 1 || held.length != 0 || named.length != 0 {
        return Err(Error::Corrupt("fixed slot lease inode changed"));
    }
    Ok(())
}
pub(super) fn lease(
    directory: &Path,
    identity: LeaseIdentity,
    offset: usize,
    span: usize,
    exclusive: bool,
) -> Result<RangeLock> {
    let p = page()?;
    if span == 0 || !offset.is_multiple_of(p) || !span.is_multiple_of(p) {
        return Err(Error::InvalidInput("unaligned fixed slot lease"));
    }
    check_identity(directory, Some(identity))?;
    let file = lock_file(directory, identity.kind, exclusive)?;
    if host::identity(&file)? != (identity.volume, identity.file_id) {
        return Err(Error::Corrupt("fixed slot lease namespace changed"));
    }
    let at = LOCK_BASE
        .checked_add((offset / p) as u64)
        .ok_or(Error::LimitExceeded)?;
    // Independent file descriptions prevent one owner from unlocking another
    // VMA's overlapping range. M3 dedicated files avoid native writer locks.
    let lock = RangeLock::acquire(
        file,
        if exclusive {
            LockMode::Exclusive
        } else {
            LockMode::Shared
        },
        at,
        (span / p) as u64,
        false,
    )
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            e.into()
        }
    })?;
    check_file(&directory.join(identity.kind.name()), lock.file())?;
    Ok(lock)
}
fn gaps(state: &State, capacity: usize) -> Result<Vec<Slot>> {
    let mut ranges: Vec<_> = state.slots.values().copied().collect();
    ranges.sort_unstable_by_key(|s| s.offset);
    let mut out = Vec::new();
    out.try_reserve(ranges.len() + 1)
        .map_err(|_| Error::LimitExceeded)?;
    let mut at = 0;
    for slot in ranges {
        if slot.offset > at {
            out.push(Slot {
                offset: at,
                span: slot.offset - at,
            });
        }
        at = slot
            .offset
            .checked_add(slot.span)
            .filter(|n| *n <= capacity)
            .ok_or(Error::Corrupt("fixed slot extent"))?;
    }
    if at < capacity {
        out.push(Slot {
            offset: at,
            span: capacity - at,
        });
    }
    Ok(out)
}
pub(super) fn range_free(state: &State, offset: usize, size: usize, capacity: usize) -> bool {
    let Some(end) = offset.checked_add(size).filter(|n| *n <= capacity) else {
        return false;
    };
    size != 0
        && state
            .slots
            .values()
            .all(|s| end <= s.offset || offset >= s.offset + s.span)
}
pub(super) fn allocation(state: &State, capacity: usize, size: usize) -> Result<Slot> {
    if !state.reusable {
        state
            .next
            .checked_add(size)
            .filter(|n| *n <= capacity)
            .ok_or(Error::LimitExceeded)?;
        return Ok(Slot {
            offset: state.next,
            span: size,
        });
    }
    let slot = gaps(state, capacity)?
        .into_iter()
        .find(|s| s.span >= size)
        .ok_or(Error::LimitExceeded)?;
    Ok(Slot {
        offset: slot.offset,
        span: size,
    })
}
pub(super) fn writer_extent(
    state: &State,
    capacity: usize,
    requested: usize,
) -> Result<(usize, usize)> {
    let p = page()?;
    let requested = requested / p * p;
    if requested == 0 {
        return Err(Error::LimitExceeded);
    }
    if !state.reusable {
        let size = requested.min(capacity.saturating_sub(state.next));
        return if size == 0 {
            Err(Error::LimitExceeded)
        } else {
            Ok((state.next, size))
        };
    }
    let gaps = gaps(state, capacity)?;
    let slot = gaps
        .iter()
        .find(|s| s.span >= requested)
        .or_else(|| gaps.iter().max_by_key(|s| s.span))
        .ok_or(Error::LimitExceeded)?;
    Ok((slot.offset, slot.span.min(requested)))
}
/// Authenticated reusable-layout capacity, not a process address high-water mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedSlotPressure {
    pub capacity: usize,
    pub occupied_bytes: usize,
    pub largest_free_span: usize,
}
impl FixedSlotPressure {
    /// Leave ordinary ready B and the true predecessor headroom. Optional
    /// rewriting can still fail; it never consumes or invalidates ready B.
    pub fn needs_rewrite(self) -> bool {
        self.occupied_bytes >= self.capacity / 2 || self.largest_free_span < self.capacity / 4
    }
}
pub(super) fn pressure(state: &State, capacity: usize) -> Result<Option<FixedSlotPressure>> {
    if !state.reusable {
        return Ok(None);
    }
    let occupied_bytes = state
        .slots
        .values()
        .try_fold(0usize, |n, s| n.checked_add(s.span))
        .ok_or(Error::LimitExceeded)?;
    let largest_free_span = gaps(state, capacity)?
        .iter()
        .map(|s| s.span)
        .max()
        .unwrap_or(0);
    Ok(Some(FixedSlotPressure {
        capacity,
        occupied_bytes,
        largest_free_span,
    }))
}
#[derive(Clone, Copy, Debug)]
pub struct FixedSlotReclaimLimits {
    pub max_layout_slots: usize,
    pub max_inventory_entries: usize,
    pub max_reclaimed_slots: usize,
}
impl Default for FixedSlotReclaimLimits {
    fn default() -> Self {
        Self {
            max_layout_slots: 65_536,
            max_inventory_entries: 4_194_304,
            max_reclaimed_slots: 4096,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FixedSlotReclaimReport {
    pub scanned_slots: usize,
    pub removed_slots: usize,
    /// Virtual placement capacity released; this method deletes no data files.
    pub released_bytes: u64,
    pub busy_slots: usize,
    pub assigned_before: usize,
    pub assigned_after: usize,
}
impl FixedMapDeclaration {
    /// Reclaim only with caller-held native writer exclusion, Capture EX and a
    /// complete authenticated durable/named/live/export root set. This method
    /// does not invent a graph certificate from a list of physical identities;
    /// Native ReclaimPlan supplies the checked root discovery and writer guard.
    pub fn reclaim_slots(
        &self,
        maintenance: &crate::coordination::MaintenanceLease,
        protected: &[crate::Manifest],
        limits: FixedSlotReclaimLimits,
    ) -> Result<FixedSlotReclaimReport> {
        let roots: Vec<&dyn FixedSlotRoot> = protected.iter().map(|m| m as &dyn FixedSlotRoot).collect();
        self.reclaim_slots_from_roots(maintenance, &roots, limits)
    }
    pub fn reclaim_slots_from_roots(
        &self,
        maintenance: &crate::coordination::MaintenanceLease,
        protected: &[&dyn FixedSlotRoot],
        limits: FixedSlotReclaimLimits,
    ) -> Result<FixedSlotReclaimReport> {
        if limits.max_layout_slots == 0
            || limits.max_layout_slots > MAX_SLOTS
            || limits.max_inventory_entries == 0
            || limits.max_inventory_entries > 16_777_216
            || limits.max_reclaimed_slots == 0
            || limits.max_reclaimed_slots > MAX_SLOTS
        {
            return Err(Error::InvalidInput("invalid fixed slot reclaim limits"));
        }
        self.check_for(&self.directory, self.uuid, self.epoch)?;
        maintenance.fixed_reuse_check(&self.directory, self.uuid)?;
        let (address, capacity, state) = FixedMap::decode(&self.directory, self.uuid, self.epoch)?;
        if !state.reusable {
            return Err(Error::InvalidInput(
                "fixed slot reuse requires fresh v2 layout",
            ));
        }
        if state.slots.len() > limits.max_layout_slots {
            return Err(Error::LimitExceeded);
        }
        if protected.is_empty() {
            return Err(Error::InvalidInput("empty fixed slot root set"));
        }
        let mut keys = std::collections::BTreeSet::new();
        let mut entries = 0usize;
        for root in protected {
            if root.uuid() != self.uuid || root.epoch() != self.epoch {
                return Err(Error::Corrupt("foreign fixed slot root"));
            }
            entries = entries
                .checked_add(root.inventory_len())
                .filter(|n| *n <= limits.max_inventory_entries)
                .ok_or(Error::LimitExceeded)?;
            for index in 0..root.inventory_len() {
                let entry = root.inventory_entry(index).ok_or(Error::Corrupt("incomplete fixed slot root"))?;
                let key = key(entry);
                if !state.slots.contains_key(&key) {
                    return Err(Error::Corrupt("protected fixed root missing slot"));
                }
                keys.insert(key);
            }
        }
        let mut candidate = state.clone();
        let mut leases = Vec::new();
        leases
            .try_reserve(limits.max_reclaimed_slots.min(state.slots.len()))
            .map_err(|_| Error::LimitExceeded)?;
        let mut report = FixedSlotReclaimReport {
            assigned_before: state.next,
            ..Default::default()
        };
        for (key, slot) in &state.slots {
            report.scanned_slots += 1;
            if keys.contains(key) || report.removed_slots >= limits.max_reclaimed_slots {
                continue;
            }
            match lease(
                &self.directory,
                state.lock_identity.unwrap(),
                slot.offset,
                slot.span,
                true,
            ) {
                Ok(guard) => {
                    leases.push(guard);
                    candidate.slots.remove(key);
                    report.removed_slots += 1;
                    report.released_bytes += slot.span as u64;
                }
                Err(Error::Busy) => report.busy_slots += 1,
                Err(e) => return Err(e),
            }
        }
        candidate.next = candidate
            .slots
            .values()
            .map(|s| s.offset + s.span)
            .max()
            .unwrap_or(0);
        report.assigned_after = candidate.next;
        if report.removed_slots != 0 {
            let bytes =
                FixedMap::encode_parts(self.uuid, self.epoch, address, capacity, &candidate)?;
            publish::replace_under_maintenance(
                &self.directory,
                self.uuid,
                self.epoch,
                &bytes,
                state.source.unwrap(),
                maintenance,
                || {
                    self.check_for(&self.directory, self.uuid, self.epoch)?;
                    check_identity(&self.directory, state.lock_identity)
                },
            )?;
        }
        maintenance.fixed_reuse_check(&self.directory, self.uuid)?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(unsafe_code)]
    fn actual_vma_clones_retain_kernel_exclusion_until_last_unmap() {
        let dir =
            std::env::temp_dir().join(format!("fixed-vma-{:032x}", crate::new_identity().unwrap()));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("LOCK"), []).unwrap();
        let page = page().unwrap();
        fs::write(dir.join("source"), vec![17; page]).unwrap();
        let identity = lock_identity(&dir).unwrap();
        let region = Reservation::reserve(None, page * 2).unwrap();
        let source = Arc::new(File::open(dir.join("source")).unwrap());
        // The test owns this private source file and never mutates/truncates it.
        let bytes = unsafe { region.map_file(source, 0, 0, page) }.unwrap();
        bytes
            .retain_layout_lock(lease(&dir, identity, 0, page, false).unwrap())
            .unwrap();
        let clone = bytes.clone();
        let pointer = bytes.as_ptr();
        drop(bytes);
        assert!(matches!(
            lease(&dir, identity, 0, page, true),
            Err(Error::Busy)
        ));
        assert_eq!(clone.bytes().unwrap()[0], 17);
        assert_eq!(clone.as_ptr(), pointer);
        drop(clone);
        let exclusive = lease(&dir, identity, 0, page, true).unwrap();
        drop(exclusive);
        drop(region);
        fs::remove_dir_all(dir).unwrap();
    }
}

impl FixedMap {
    /// Private entry used only by the opaque freshly-created BootstrapPairTarget.
    /// Import authenticates the original image before rebinding its process-local
    /// lease inode. No existing source namespace can obtain this target authority.
    pub(crate) fn rebind_imported_layout(
        directory: &Path,
        roots: &[&crate::Manifest],
        writer: Option<&File>,
        check: impl Fn() -> Result<()>,
    ) -> Result<()> {
        check()?;
        match fs::symlink_metadata(directory.join("FIXEDMAP")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => (),
        }
        let first = roots
            .first()
            .ok_or(Error::InvalidInput("empty imported fixed roots"))?;
        // Header/hash/row/overlap/realm validation is unchanged. Only an imported
        // source LOCK identity is not required to exist in the new target yet.
        let (address, capacity, mut state) =
            Self::decode_image(directory, first.uuid, first.epoch, false)?;
        for root in roots {
            if root.uuid != first.uuid
                || root.epoch != first.epoch
                || root
                    .segments
                    .iter()
                    .any(|e| !state.slots.contains_key(&key(e)))
            {
                return Err(Error::Corrupt("imported fixed graph layout mismatch"));
            }
        }
        if !state.reusable {
            return Ok(());
        }
        let writer = writer.ok_or(Error::InvalidInput(
            "reusable fixed import requires owned writer",
        ))?;
        let target_lock = identity_for(directory, LeaseKind::NativeLock)?;
        if host::identity(writer)? != (target_lock.volume, target_lock.file_id) {
            return Err(Error::Corrupt("fixed import writer namespace changed"));
        }
        state.version = target_version(state.version);
        state.lock_identity = Some(create_identity(directory, state.version)?);
        let bytes = Self::encode_parts(first.uuid, first.epoch, address, capacity, &state)?;
        publish::replace(
            directory,
            first.uuid,
            first.epoch,
            &bytes,
            state.source.unwrap(),
            &publish::Options::default(),
            || {
                check()?;
                check_file(&directory.join("LOCK"), writer)?;
                check_identity(directory, state.lock_identity)
            },
        )?;
        Self::validate_layout(directory, roots)?;
        check()
    }
}
