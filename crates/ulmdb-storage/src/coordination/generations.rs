//! Opt-in immutable-membership physical-pin generations. This module supplies
//! bounded record discovery and source leases, never graph or unlink authority.
use super::*;
use std::collections::{BTreeSet, VecDeque};
use ulmdb_platform::FileExt;

const HEADER_BYTES: usize = 128;
const SCAN: &str = "_scan";
const MAX_PENDING_RECORDS: u64 = 4096;

/// Birth namespace identities, observed under Registry before creating the pin.
/// Existing walkers hold their directory/scan handles, preventing identity reuse
/// while membership must remain immutable. No extra FD is retained per reader.
#[derive(Clone, Copy)]
pub(super) struct Birth {
    id: u64,
    directory: Identity,
    scan: Identity,
}

pub(super) fn birth(inner: &Inner, directory: &Path) -> Result<Option<Birth>> {
    if directory == inner.pins {
        return Ok(None);
    }
    if directory.parent() != Some(inner.pins.as_path()) {
        return Err(Error::Corrupt("pin birth outside registry"));
    }
    let id = number(
        directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(Error::Corrupt("invalid pin birth generation"))?,
    )?;
    let held = opened(directory, false, true)?;
    let scan = opened(&directory.join(SCAN), false, false)?;
    if scan.metadata()?.len() != 0 {
        return Err(Error::Corrupt("nonempty generation scan gate"));
    }
    inner.check()?;
    Ok(Some(Birth {
        id,
        directory: Identity::of(&held.metadata()?),
        scan: Identity::of(&scan.metadata()?),
    }))
}

/// Best effort owner release. Callers still own the locked record OFD.
/// Failure keeps a conservative stale member for a later exclusive cleanup.
pub(super) fn release(
    inner: &Inner,
    path: &Path,
    record: &File,
    birth: Option<Birth>,
) -> Result<()> {
    let registry = inner.try_registry()?;
    let current = profile(inner, &registry.file)?;
    let Some(birth) = birth else {
        if path.parent() != Some(inner.pins.as_path()) {
            return Err(Error::Corrupt("flat pin release outside registry"));
        }
        // Migration counts a flat record only when it moves it into gen-0.
        // Its old flat name therefore never decrements the profiled counter.
        if current.is_none_or(|profile| profile.migrating) {
            remove_owned(path, record)?;
        }
        return Ok(());
    };
    let profile = current.ok_or(Error::Corrupt("pin generation profile lost"))?;
    let directory = generation(inner, birth.id);
    if birth.id > profile.active || path.parent() != Some(directory.as_path()) {
        return Err(Error::Corrupt("pin release generation changed"));
    }
    let held = opened(&directory, false, true)?;
    if Identity::of(&held.metadata()?) != birth.directory {
        return Err(Error::Corrupt("pin birth generation replaced"));
    }
    let scan_path = directory.join(SCAN);
    let scan = opened(&scan_path, false, false)?;
    if scan.metadata()?.len() != 0 || Identity::of(&scan.metadata()?) != birth.scan {
        return Err(Error::Corrupt("pin birth scan gate replaced"));
    }
    // Registry keeps the active generation stable; EX also protects a sealed
    // generation from mutation while any resumable ReadDir holds membership SH.
    let exclusion = Lease::acquire(scan, false)?;
    inner.check()?;
    let named_directory = fs::symlink_metadata(&directory)?;
    let named_scan = fs::symlink_metadata(&scan_path)?;
    if !named_directory.is_dir()
        || named_directory.file_type().is_symlink()
        || Identity::of(&named_directory) != birth.directory
        || !named_scan.is_file()
        || named_scan.file_type().is_symlink()
        || named_scan.len() != 0
        || Identity::of(&named_scan) != birth.scan
        || Identity::of(&exclusion.file.metadata()?) != birth.scan
    {
        return Err(Error::Corrupt("pin release namespace replaced"));
    }
    if remove_owned(path, record)? {
        removed(inner, &registry.file, record)?;
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
#[path = "pin_release_tests.rs"]
mod release_tests;

#[derive(Clone, Copy)]
pub(super) struct Profile {
    active: u64,
    migrating: bool,
    // Conservative reservations: a failed creation may consume capacity until
    // an explicit owner rebuild. It can never undercount a published record.
    records: u64,
    pending_records: u64,
}
fn generation(inner: &Inner, number: u64) -> PathBuf {
    inner.pins.join(format!("gen-{number:016x}"))
}
fn number(name: &str) -> Result<u64> {
    let hex = name
        .strip_prefix("gen-")
        .ok_or(Error::Corrupt("invalid pin generation"))?;
    if hex.len() != 16
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Corrupt("invalid pin generation"));
    }
    u64::from_str_radix(hex, 16).map_err(|_| Error::Corrupt("invalid pin generation"))
}
pub(super) fn profile(inner: &Inner, file: &File) -> Result<Option<Profile>> {
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(None);
    }
    if length != HEADER_BYTES as u64 {
        return Err(Error::Corrupt("pin registry profile length"));
    }
    let mut b = [0; HEADER_BYTES];
    file.read_exact_at(&mut b, 0)?;
    if &b[..8] != b"ULMDBRG2"
        || b[8..10] != 2u16.to_le_bytes()
        || b[10..12] != (HEADER_BYTES as u16).to_le_bytes()
        || b[12..16] != [0; 4]
        || b[16..32] != inner.uuid
        || b[32..40] != inner.identity.device.to_le_bytes()
        || b[40..48] != inner.identity.inode.to_le_bytes()
        || b[56] > 1
        || b[57..64] != [0; 7]
        || b[80..120] != [0; 40]
        || b[120..124] != crate::publication::crc(&b[..120]).to_le_bytes()
        || b[124..] != [0; 4]
    {
        return Err(Error::Corrupt("pin registry profile binding"));
    }
    let active = u64::from_le_bytes(b[48..56].try_into().unwrap());
    let records = u64::from_le_bytes(b[64..72].try_into().unwrap());
    let pending_records = u64::from_le_bytes(b[72..80].try_into().unwrap());
    if active == 0
        || records > MAX_LIMIT as u64
        || pending_records > MAX_PENDING_RECORDS
        || pending_records > records
    {
        return Err(Error::Corrupt("pin registry profile counters"));
    }
    Ok(Some(Profile {
        active,
        migrating: b[56] != 0,
        records,
        pending_records,
    }))
}
fn store(inner: &Inner, file: &File, p: Profile) -> Result<()> {
    inner.check()?;
    let mut b = [0; HEADER_BYTES];
    b[..8].copy_from_slice(b"ULMDBRG2");
    b[8..10].copy_from_slice(&2u16.to_le_bytes());
    b[10..12].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    b[16..32].copy_from_slice(&inner.uuid);
    b[32..40].copy_from_slice(&inner.identity.device.to_le_bytes());
    b[40..48].copy_from_slice(&inner.identity.inode.to_le_bytes());
    b[48..56].copy_from_slice(&p.active.to_le_bytes());
    b[56] = u8::from(p.migrating);
    b[64..72].copy_from_slice(&p.records.to_le_bytes());
    b[72..80].copy_from_slice(&p.pending_records.to_le_bytes());
    let crc = crate::publication::crc(&b[..120]);
    b[120..124].copy_from_slice(&crc.to_le_bytes());
    // Partial metadata writes fail closed at the next CRC/length check. These
    // are host-lifetime leases; no business publication depends on persistence.
    file.write_all_at(&b, 0)?;
    file.set_len(HEADER_BYTES as u64)?;
    inner.check()
}
fn create_generation(inner: &Inner, id: u64) -> Result<()> {
    let directory = generation(inner, id);
    let _directory = ensure_directory(&directory)?;
    let _gate = gate(&directory.join(SCAN), true)?;
    inner.check()
}
fn is_empty(inner: &Inner, id: u64) -> Result<bool> {
    let path = generation(inner, id);
    // Registry exclusion prevents births in active while this is observed.
    // There is exactly one control entry, so finding a member is constant work.
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or(Error::Corrupt("invalid generation entry"))?;
        if name == SCAN {
            continue;
        }
        parse_name(name)?;
        return Ok(false);
    }
    Ok(true)
}
pub(super) fn paths(inner: &Inner) -> Result<Option<Vec<(PathBuf, u128)>>> {
    let file = opened(&inner.area.join("registry"), false, false)?;
    let Some(p) = profile(inner, &file)? else {
        return Ok(None);
    };
    // Compatibility full scan, used only by existing full admission/scanning.
    // It is deliberately not the incremental discovery API.
    let mut out = Vec::new();
    for entry in fs::read_dir(&inner.pins)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or(Error::Corrupt("invalid registry entry"))?;
        if name.starts_with("pin-") && p.migrating {
            push_path(inner, &mut out, entry.path(), parse_name(name)?)?;
            continue;
        }
        let id = number(name)?;
        if id > p.active || !opened(&entry.path(), false, true)?.metadata()?.is_dir() {
            return Err(Error::Corrupt("invalid registry generation"));
        }
        for member in fs::read_dir(entry.path())? {
            let member = member?;
            let name = member.file_name();
            let name = name
                .to_str()
                .ok_or(Error::Corrupt("invalid generation entry"))?;
            if name == SCAN {
                continue;
            }
            push_path(inner, &mut out, member.path(), parse_name(name)?)?;
        }
    }
    Ok(Some(out))
}
fn push_path(
    _inner: &Inner,
    out: &mut Vec<(PathBuf, u128)>,
    path: PathBuf,
    nonce: u128,
) -> Result<()> {
    if out.len() >= MAX_LIMIT {
        return Err(Error::LimitExceeded);
    }
    let m = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::Corrupt("nonregular physical pin"));
    }
    out.push((path, nonce));
    Ok(())
}
pub(super) fn reserve_birth(inner: &Inner, registry: &File) -> Result<Option<PathBuf>> {
    reserve_record(inner, registry, false)
}
fn reserve_record(inner: &Inner, registry: &File, pending: bool) -> Result<Option<PathBuf>> {
    let Some(mut p) = profile(inner, registry)? else {
        return Ok(None);
    };
    if p.migrating {
        // Header publication precedes initial directory creation. Resume an
        // interrupted negotiation under the same registry exclusion before a
        // producer needs either initial namespace. No membership is inferred.
        create_generation(inner, 0)?;
        create_generation(inner, p.active)?;
    }
    if p.records >= MAX_LIMIT as u64 {
        return Err(Error::LimitExceeded);
    }
    if pending {
        // Internal source protection has a separate bounded allowance. It does
        // not consume a custom two-pin reader/candidate admission budget.
        if p.pending_records >= MAX_PENDING_RECORDS {
            return Err(Error::LimitExceeded);
        }
        p.pending_records += 1;
    } else {
        // Preserve the existing ordinary admission walk; only live ordinary
        // records charge that cap. Stale immutable generation members do not.
        let mut live = 0usize;
        for (path, _) in paths(inner)?.unwrap() {
            let file = match opened(&path, false, false) {
                Ok(file) => file,
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            let mut version = [0; 10];
            if file.metadata()?.len() < 10 {
                continue;
            }
            file.read_exact_at(&mut version, 0)?;
            if version[8..10] != 1u16.to_le_bytes() {
                continue;
            }
            match file.try_lock() {
                Ok(()) => file.unlock()?,
                Err(TryLockError::WouldBlock) => {
                    live += 1;
                    if live >= inner.limits.max_pin_records {
                        return Err(Error::LimitExceeded);
                    }
                }
                Err(TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }
    p.records = p.records.checked_add(1).ok_or(Error::LimitExceeded)?;
    store(inner, registry, p)?;
    Ok(Some(generation(inner, p.active)))
}
pub(super) fn removed(inner: &Inner, registry: &File, record: &File) -> Result<()> {
    if let Some(mut p) = profile(inner, registry)? {
        p.records = p
            .records
            .checked_sub(1)
            .ok_or(Error::Corrupt("pin registry count"))?;
        let mut b = [0; 10];
        if record.metadata()?.len() >= 10 {
            record.read_exact_at(&mut b, 0)?;
            if b[8..10] != 1u16.to_le_bytes() {
                p.pending_records = p
                    .pending_records
                    .checked_sub(1)
                    .ok_or(Error::Corrupt("pending registry count"))?;
            }
        }
        store(inner, registry, p)?;
    }
    Ok(())
}
pub(super) fn cleanup_leases(inner: &Inner, registry: &File) -> Result<Vec<Lease>> {
    let Some(p) = profile(inner, registry)? else {
        return Ok(Vec::new());
    };
    if p.migrating {
        return Err(Error::Busy);
    }
    let mut leases = Vec::new();
    for id in 0..p.active {
        // A scanner holding generation SH makes this return Busy. No sealed
        // member can then be removed while a resumable ReadDir is alive.
        leases.push(gate(&generation(inner, id).join(SCAN), false)?);
    }
    Ok(leases)
}

/// Source protection for one boundedly validated physical record. Its sequence
/// and commit are record claims until an authenticated descriptor agrees.
/// There is intentionally no Manifest, graph or currentness accessor.
pub struct PendingPhysicalPin {
    inner: Arc<Inner>,
    path: PathBuf,
    lease: Lease,
    birth: Option<Birth>,
    checkpoint: Option<[u8; 64]>,
    generation: u128,
    sequence: Option<u64>,
    commit: Option<u128>,
}
impl PendingPhysicalPin {
    pub fn checkpoint(&self) -> Option<&[u8; 64]> {
        self.checkpoint.as_ref()
    }
    pub fn generation(&self) -> u128 {
        self.generation
    }
    pub fn claimed_sequence(&self) -> Option<u64> {
        self.sequence
    }
    pub fn claimed_commit(&self) -> Option<u128> {
        self.commit
    }
    pub fn check(&self) -> Result<()> {
        self.inner.check()?;
        let named = fs::symlink_metadata(&self.path)?;
        let held = self.lease.file.metadata()?;
        if !named.is_file()
            || named.file_type().is_symlink()
            || Identity::of(&named) != Identity::of(&held)
            || named.len() != RECORD_BYTES as u64
        {
            return Err(Error::Corrupt("pending pin path replaced"));
        }
        Ok(())
    }
    pub fn directory(&self) -> Result<&Path> {
        self.check()?;
        Ok(&self.inner.directory)
    }
    pub fn transform_context(&self) -> Result<Option<Arc<crate::transform::TransformContext>>> {
        self.check()?;
        Ok(self.inner.transform.clone())
    }
}
impl Drop for PendingPhysicalPin {
    fn drop(&mut self) {
        if self.lease.creator == std::process::id() {
            // Active copies of already covered roots must not create another
            // discovery frontier. Registry and membership EX are nonblocking;
            // a live sealed walker instead leaves a conservative stale member.
            let _ = release(&self.inner, &self.path, &self.lease.file, self.birth);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PinDiscoveryBudget {
    pub records: usize,
}
pub struct PinDiscoveryProgress {
    /// At most `budget.records` source records, each fixed RECORD_BYTES bytes.
    pub pins: Vec<PendingPhysicalPin>,
    /// Includes migration attempts, directory-control entries and stale rows.
    pub work_units: usize,
    pub complete: bool,
}
struct Walk {
    id: u64,
    directory: File,
    identity: Identity,
    entries: fs::ReadDir,
    _membership: Lease,
    stale: VecDeque<PathBuf>,
}
struct Cleanup {
    id: u64,
    directory: File,
    identity: Identity,
    records: VecDeque<PathBuf>,
    exclusion: Option<Lease>,
}
/// Continuation over sealed pin membership. The capability retains no Capture,
/// Registry or writer guard between calls. Each call requires a fresh exclusive
/// Capture owner. Completion covers physical records only, not other root roles.
pub struct PinDiscovery {
    inner: Arc<Inner>,
    next: u64,
    end: u64,
    walk: Option<Walk>,
    cleanup: Option<Cleanup>,
    own_records: BTreeSet<PathBuf>,
    failed: bool,
}
impl MaintenanceLease {
    /// Explicit one-way profile negotiation. A genuine namespace writer fence
    /// excludes old writable producers; nonempty registry bytes make old fresh
    /// clients reject the profile. Existing pin wire bytes are unchanged.
    pub fn begin_pin_discovery(&mut self, writer: &crate::WriterFence) -> Result<PinDiscovery> {
        self.check()?;
        writer.check_directory(&self.inner.directory)?;
        let registry = self.inner.registry()?;
        writer.check_directory(&self.inner.directory)?;
        let p = match profile(&self.inner, &registry.file)? {
            Some(p) => {
                if p.migrating {
                    create_generation(&self.inner, 0)?;
                    create_generation(&self.inner, p.active)?;
                }
                p
            }
            None => {
                let p = Profile {
                    active: 1,
                    migrating: true,
                    records: 0,
                    pending_records: 0,
                };
                // Publish the rejecting header before adding directory entries.
                // A failure is fail-closed; no physical files are reclaimed.
                store(&self.inner, &registry.file, p)?;
                create_generation(&self.inner, 0)?;
                create_generation(&self.inner, 1)?;
                p
            }
        };
        self.check()?;
        Ok(PinDiscovery {
            inner: self.inner.clone(),
            next: 0,
            end: p.active - 1,
            walk: None,
            cleanup: None,
            own_records: BTreeSet::new(),
            failed: false,
        })
    }
}
impl PinDiscovery {
    fn check(&self, owner: &MaintenanceLease) -> Result<()> {
        if self.failed {
            return Err(Error::InvalidInput("failed pin discovery"));
        }
        self.inner.check()?;
        owner.check()?;
        // A transferred continuation may use a fresh coordinator for this same
        // physical namespace. Both sides remain freshly checked, with exactly
        // the original realm and admission policy; Arc identity is incidental.
        if self.inner.directory != owner.inner.directory
            || self.inner.identity != owner.inner.identity
            || self.inner.area_identity != owner.inner.area_identity
            || self.inner.pins_identity != owner.inner.pins_identity
            || self.inner.uuid != owner.inner.uuid
            || self.inner.limits.max_pin_records != owner.inner.limits.max_pin_records
            || self.inner.limits.max_inventory_entries != owner.inner.limits.max_inventory_entries
        {
            return Err(Error::InvalidInput("foreign pin discovery owner"));
        }
        Ok(())
    }
    pub fn advance(
        &mut self,
        owner: &mut MaintenanceLease,
        budget: PinDiscoveryBudget,
    ) -> Result<PinDiscoveryProgress> {
        self.check(owner)?;
        let result = self.advance_inner(owner, budget);
        if result.is_err() {
            self.failed = true;
            self.walk = None;
            self.cleanup = None;
        }
        result
    }
    fn advance_inner(
        &mut self,
        owner: &mut MaintenanceLease,
        budget: PinDiscoveryBudget,
    ) -> Result<PinDiscoveryProgress> {
        let mut out = PinDiscoveryProgress {
            pins: Vec::new(),
            work_units: 0,
            complete: false,
        };
        if budget.records == 0 {
            return Ok(out);
        }
        let registry = self.inner.registry()?;
        owner.check()?;
        let mut p = profile(&self.inner, &registry.file)?
            .ok_or(Error::Corrupt("pin discovery profile lost"))?;
        while p.migrating && out.work_units < budget.records {
            // Reopen at the first non-control entry for every move. The two
            // fixed generation directories are the only controls at this phase.
            // Old Drop can only remove flat names, never add or remove moved
            // members. No mutable directory offset is a completeness proof.
            let mut source = None;
            for entry in fs::read_dir(&self.inner.pins)? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name
                    .to_str()
                    .ok_or(Error::Corrupt("invalid legacy pin name"))?;
                if name == "gen-0000000000000000" || name == "gen-0000000000000001" {
                    continue;
                }
                parse_name(name)?;
                source = Some(entry.path());
                break;
            }
            out.work_units += 1;
            let Some(source) = source else {
                p.migrating = false;
                store(&self.inner, &registry.file, p)?;
                break;
            };
            let metadata = match fs::symlink_metadata(&source) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(Error::Corrupt("nonregular legacy pin"));
            }
            // Reserve before the namespace change: errors may overcount, never
            // lose a source lease from the quota or bypass the configured cap.
            p.records = p.records.checked_add(1).ok_or(Error::LimitExceeded)?;
            store(&self.inner, &registry.file, p)?;
            let target = generation(&self.inner, 0).join(source.file_name().unwrap());
            match fs::hard_link(&source, &target) {
                Ok(()) => {
                    let file = File::open(&target)?;
                    remove_owned(&source, &file)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    p.records -= 1;
                    store(&self.inner, &registry.file, p)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let file = File::open(&target)?;
                    if Identity::of(&file.metadata()?) != Identity::of(&metadata) {
                        return Err(Error::Corrupt("legacy pin migration collision"));
                    }
                    remove_owned(&source, &file)?;
                }
                Err(e) => return Err(e.into()),
            }
        }
        if p.migrating {
            return Ok(out);
        }
        while self.next <= self.end && out.work_units < budget.records {
            if let Some(cleanup) = self.cleanup.as_mut() {
                if cleanup.exclusion.is_none() {
                    match gate(&generation(&self.inner, cleanup.id).join(SCAN), false) {
                        Ok(lease) => cleanup.exclusion = Some(lease),
                        Err(Error::Busy) => return Ok(out),
                        Err(e) => return Err(e),
                    }
                }
                let named = fs::symlink_metadata(generation(&self.inner, cleanup.id))?;
                if !named.is_dir()
                    || named.file_type().is_symlink()
                    || Identity::of(&named) != cleanup.identity
                    || Identity::of(&cleanup.directory.metadata()?) != cleanup.identity
                {
                    return Err(Error::Corrupt("cleanup generation replaced"));
                }
                if let Some(path) = cleanup.records.pop_front() {
                    out.work_units += 1;
                    let file = match opened(&path, false, false) {
                        Ok(file) => file,
                        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(e),
                    };
                    match file.try_lock() {
                        Ok(()) => {
                            let stale = Lease {
                                file,
                                creator: std::process::id(),
                            };
                            if remove_owned(&path, &stale.file)? {
                                removed(&self.inner, &registry.file, &stale.file)?;
                            }
                        }
                        Err(TryLockError::WouldBlock) => {}
                        Err(TryLockError::Error(e)) => return Err(e.into()),
                    }
                    continue;
                }
                self.cleanup = None;
                self.next = self.next.checked_add(1).ok_or(Error::LimitExceeded)?;
                continue;
            }
            if self.walk.is_none() {
                let path = generation(&self.inner, self.next);
                let directory = opened(&path, false, true)?;
                let membership = gate(&path.join(SCAN), true)?;
                self.inner.check()?;
                let identity = Identity::of(&directory.metadata()?);
                let entries = fs::read_dir(&path)?;
                self.walk = Some(Walk {
                    id: self.next,
                    directory,
                    identity,
                    entries,
                    _membership: membership,
                    stale: VecDeque::new(),
                });
                out.work_units += 1;
                if out.work_units == budget.records {
                    break;
                }
            }
            let walk = self.walk.as_mut().unwrap();
            let path = generation(&self.inner, walk.id);
            let named = fs::symlink_metadata(&path)?;
            if !named.is_dir()
                || named.file_type().is_symlink()
                || Identity::of(&named) != walk.identity
                || Identity::of(&walk.directory.metadata()?) != walk.identity
            {
                return Err(Error::Corrupt("pin generation replaced"));
            }
            out.work_units += 1;
            let Some(entry) = walk.entries.next() else {
                let walk = self.walk.take().unwrap();
                self.cleanup = Some(Cleanup {
                    id: walk.id,
                    directory: walk.directory,
                    identity: walk.identity,
                    records: walk.stale,
                    exclusion: None,
                });
                // Membership SH is dropped before trying generation EX. This
                // does not retain Capture or Registry between calls.
                continue;
            };
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or(Error::Corrupt("invalid generation entry"))?;
            if name == SCAN {
                continue;
            }
            let nonce = parse_name(name)?;
            let path = entry.path();
            if self.own_records.contains(&path) {
                continue;
            }
            let file = opened(&path, false, false)?;
            match file.try_lock() {
                Ok(()) => {
                    file.unlock()?;
                    walk.stale.push_back(path);
                    continue;
                }
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(e)) => return Err(e.into()),
            }
            let mut bytes = [0; RECORD_BYTES];
            file.read_exact_at(&mut bytes, 0)?;
            validate_record(&self.inner, &file, &bytes, nonce)?;
            let (checkpoint, generation, sequence, commit) = record_source(&bytes)?;
            // Released copies can leave historical ownership names in this
            // continuation. Bound that bookkeeping even when Drop has already
            // returned the corresponding registry capacity.
            if self.own_records.len() >= MAX_PENDING_RECORDS as usize {
                return Err(Error::LimitExceeded);
            }
            let directory = reserve_record(&self.inner, &registry.file, true)?
                .ok_or(Error::Corrupt("pin discovery profile lost"))?;
            let birth = birth(&self.inner, &directory)?;
            let new_nonce = new_identity()?;
            let new_path = directory.join(format!("pin-{new_nonce:032x}.lease"));
            let new_file = crate::access::options_for_entry(&new_path)?
                .read(true)
                .write(true)
                .create_new(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&new_path)?;
            crate::access::validate_entry(&new_path, &new_file)?;
            let lease = Lease::acquire(new_file, false)?;
            if bytes[8..10] == 1u16.to_le_bytes() {
                bytes[8..10].copy_from_slice(&4u16.to_le_bytes());
            }
            bytes[136..152].copy_from_slice(&new_nonce.to_le_bytes());
            let crc = crate::publication::crc(&bytes[..152]);
            bytes[152..156].copy_from_slice(&crc.to_le_bytes());
            lease.file.write_all_at(&bytes, 0)?;
            self.own_records.insert(new_path.clone());
            out.pins.push(PendingPhysicalPin {
                inner: self.inner.clone(),
                path: new_path,
                lease,
                birth,
                checkpoint,
                generation,
                sequence,
                commit,
            });
        }
        owner.check()?;
        out.complete = self.next > self.end;
        Ok(out)
    }
    /// Under fresh exclusive Capture, seal all births since the last frontier.
    /// Returns true when more sealed membership must be walked. False grants no
    /// unlink authority: Native must also cover selected/named/retained roots.
    pub fn refresh(&mut self, owner: &mut MaintenanceLease) -> Result<bool> {
        self.check(owner)?;
        if self.walk.is_some() || self.cleanup.is_some() || self.next <= self.end {
            return Ok(true);
        }
        let registry = self.inner.registry()?;
        owner.check()?;
        let mut p = profile(&self.inner, &registry.file)?
            .ok_or(Error::Corrupt("pin discovery profile lost"))?;
        if p.migrating {
            return Ok(true);
        }
        if is_empty(&self.inner, p.active)? {
            owner.check()?;
            return Ok(false);
        }
        let next = p.active.checked_add(1).ok_or(Error::LimitExceeded)?;
        create_generation(&self.inner, next)?;
        self.end = p.active;
        p.active = next;
        store(&self.inner, &registry.file, p)?;
        owner.check()?;
        Ok(true)
    }
}
pub(super) fn validate_record(
    inner: &Inner,
    file: &File,
    b: &[u8; RECORD_BYTES],
    nonce: u128,
) -> Result<()> {
    if file.metadata()?.len() != RECORD_BYTES as u64
        || &b[..8] != b"ULMDBPIN"
        || !(1..=4).contains(&u16::from_le_bytes(b[8..10].try_into().unwrap()))
        || b[10..12] != (RECORD_BYTES as u16).to_le_bytes()
        || b[12..16] != [0; 4]
        || b[156..] != [0; 4]
        || b[152..156] != crate::publication::crc(&b[..152]).to_le_bytes()
        || b[16..24] != inner.identity.device.to_le_bytes()
        || b[24..32] != inner.identity.inode.to_le_bytes()
        || b[32..48] != inner.uuid
        || b[136..152] != nonce.to_le_bytes()
    {
        return Err(Error::Corrupt("pending physical pin binding"));
    }
    Ok(())
}

pub(super) fn record_source(
    b: &[u8; RECORD_BYTES],
) -> Result<(Option<[u8; 64]>, u128, Option<u64>, Option<u128>)> {
    if b[8..10] != 2u16.to_le_bytes() {
        let checkpoint: [u8; 64] = b[48..112].try_into().unwrap();
        let generation = checkpoint_identity(&checkpoint)?.0;
        if b[8..10] == 3u16.to_le_bytes() {
            if b[112..136] != [0; 24] {
                return Err(Error::Corrupt("opaque checkpoint pin identity"));
            }
            Ok((Some(checkpoint), generation, None, None))
        } else {
            Ok((
                Some(checkpoint),
                generation,
                Some(u64::from_le_bytes(b[112..120].try_into().unwrap())),
                Some(u128::from_le_bytes(b[120..136].try_into().unwrap())),
            ))
        }
    } else {
        let generation = u128::from_le_bytes(b[48..64].try_into().unwrap());
        if generation == 0 || b[64..136] != [0; 72] {
            return Err(Error::Corrupt("unresolved pin source"));
        }
        Ok((None, generation, None, None))
    }
}
impl MaintenanceLease {
    /// Protect the actual predecessor link of a genuine selected descriptor
    /// before its hash/descriptor has been incrementally resolved. Any other
    /// maintenance scanner encountering this live unresolved record must defer.
    pub fn protect_previous_generation(
        &self,
        selected: &PhysicalPin,
    ) -> Result<Option<PendingPhysicalPin>> {
        selected.inner.check()?;
        self.check()?;
        if selected.inner.identity != self.inner.identity || selected.inner.uuid != self.inner.uuid
        {
            return Err(Error::InvalidInput("foreign predecessor source pin"));
        }
        let generation = selected.root.manifest.previous;
        if generation == 0 {
            return Ok(None);
        }
        let registry = self.inner.registry()?;
        let directory = reserve_record(&self.inner, &registry.file, true)?
            .ok_or(Error::InvalidInput("generation registry not enabled"))?;
        let birth = birth(&self.inner, &directory)?;
        let nonce = new_identity()?;
        let path = directory.join(format!("pin-{nonce:032x}.lease"));
        let file = crate::access::options_for_entry(&path)?
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        crate::access::validate_entry(&path, &file)?;
        let lease = Lease::acquire(file, false)?;
        let mut b = [0; RECORD_BYTES];
        b[..8].copy_from_slice(b"ULMDBPIN");
        b[8..10].copy_from_slice(&2u16.to_le_bytes());
        b[10..12].copy_from_slice(&(RECORD_BYTES as u16).to_le_bytes());
        b[16..24].copy_from_slice(&self.inner.identity.device.to_le_bytes());
        b[24..32].copy_from_slice(&self.inner.identity.inode.to_le_bytes());
        b[32..48].copy_from_slice(&self.inner.uuid);
        b[48..64].copy_from_slice(&generation.to_le_bytes());
        b[136..152].copy_from_slice(&nonce.to_le_bytes());
        let crc = crate::publication::crc(&b[..152]);
        b[152..156].copy_from_slice(&crc.to_le_bytes());
        lease.file.write_all_at(&b, 0)?;
        self.check()?;
        Ok(Some(PendingPhysicalPin {
            inner: self.inner.clone(),
            path,
            lease,
            birth,
            checkpoint: None,
            generation,
            sequence: None,
            commit: None,
        }))
    }
}

impl MaintenanceLease {
    /// Protect a durable pointer under genuine writer exclusion without
    /// decoding its descriptor. Authentication by the bounded cursor is still
    /// required; this capability grants no graph or currentness authority.
    pub fn protect_checkpoint_pending(
        &self,
        checkpoint: &[u8; 64],
        writer: &crate::WriterFence,
    ) -> Result<PendingPhysicalPin> {
        self.check()?;
        writer.check_directory(&self.inner.directory)?;
        let generation = checkpoint_identity(checkpoint)?.0;
        let registry = self.inner.registry()?;
        writer.check_directory(&self.inner.directory)?;
        let directory = reserve_record(&self.inner, &registry.file, true)?
            .ok_or(Error::InvalidInput("generation registry not enabled"))?;
        let birth = birth(&self.inner, &directory)?;
        let nonce = new_identity()?;
        let path = directory.join(format!("pin-{nonce:032x}.lease"));
        let file = crate::access::options_for_entry(&path)?
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        crate::access::validate_entry(&path, &file)?;
        let lease = Lease::acquire(file, false)?;
        let mut b = [0; RECORD_BYTES];
        b[..8].copy_from_slice(b"ULMDBPIN");
        b[8..10].copy_from_slice(&3u16.to_le_bytes());
        b[10..12].copy_from_slice(&(RECORD_BYTES as u16).to_le_bytes());
        b[16..24].copy_from_slice(&self.inner.identity.device.to_le_bytes());
        b[24..32].copy_from_slice(&self.inner.identity.inode.to_le_bytes());
        b[32..48].copy_from_slice(&self.inner.uuid);
        b[48..112].copy_from_slice(checkpoint);
        b[136..152].copy_from_slice(&nonce.to_le_bytes());
        let crc = crate::publication::crc(&b[..152]);
        b[152..156].copy_from_slice(&crc.to_le_bytes());
        lease.file.write_all_at(&b, 0)?;
        self.check()?;
        Ok(PendingPhysicalPin {
            inner: self.inner.clone(),
            path,
            lease,
            birth,
            checkpoint: Some(*checkpoint),
            generation,
            sequence: None,
            commit: None,
        })
    }
}
