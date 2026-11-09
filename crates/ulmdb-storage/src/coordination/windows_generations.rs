//! Opt-in immutable-membership physical-pin generations. This module supplies
//! bounded record discovery and source leases, never graph or unlink authority.
use super::*;
use std::collections::{BTreeSet, VecDeque};
use ulmdb_platform::FileExt;

const HEADER_BYTES: usize = 128;
const SCAN: &str = "_scan";
const MAX_PENDING_RECORDS: u64 = 4096;

#[path = "generation_index.rs"]
mod index;
use index::LINKS;

/// Full-width namespace identities captured under Registry before the birth.
/// Walkers retain their directory/scan handles while membership is immutable;
/// no additional lifetime handles are required for an ordinary reader.
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
        directory: Identity::file(&held)?,
        scan: Identity::file(&scan)?,
    }))
}

/// Best effort ordinary-owner release while its record remains range-locked.
/// Busy gates or changed namespaces leave a conservative stale member.
pub(super) fn release(
    inner: &Inner,
    path: &Path,
    record: &File,
    birth: Option<Birth>,
) -> Result<()> {
    let registry = inner.try_registry()?;
    let current = profile(inner, registry.file())?;
    let Some(birth) = birth else {
        if path.parent() != Some(inner.pins.as_path()) {
            return Err(Error::Corrupt("flat pin release outside registry"));
        }
        // Flat names are counted only after migration moves them into gen-0.
        // Releasing an unmoved flat name therefore never decrements a profile.
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
    if Identity::file(&held)? != birth.directory {
        return Err(Error::Corrupt("pin birth generation replaced"));
    }
    let scan_path = directory.join(SCAN);
    let scan = opened(&scan_path, false, false)?;
    if scan.metadata()?.len() != 0 || Identity::file(&scan)? != birth.scan {
        return Err(Error::Corrupt("pin birth scan gate replaced"));
    }
    // Registry stabilizes the active generation, and generation EX excludes
    // any retained discovery SH walk from observing a membership mutation.
    let exclusion = Lease::acquire(scan, false)?;
    inner.check()?;
    let named_directory = ulmdb_platform::path_identity(&directory)?;
    let named_scan = ulmdb_platform::path_identity(&scan_path)?;
    if !named_directory.is_dir()
        || named_directory.reparse
        || (Identity {
            device: named_directory.volume,
            inode: named_directory.file_id,
        }) != birth.directory
        || !named_scan.is_file()
        || named_scan.reparse
        || named_scan.length != 0
        || (Identity {
            device: named_scan.volume,
            inode: named_scan.file_id,
        }) != birth.scan
        || Identity::file(exclusion.file())? != birth.scan
    {
        return Err(Error::Corrupt("pin release namespace replaced"));
    }
    // Windows removal is handle-based and can change the record's metadata.
    // Read the counter category before removing this exact owned record.
    let pending = pending_record(record)?;
    if remove_owned(path, record)? {
        removed(inner, registry.file(), pending)?;
        index::retire(inner, registry.file(), birth.id, &held, &exclusion)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct Profile {
    active: u64,
    migrating: bool,
    // Conservative reservations: a failed creation may consume capacity until
    // an explicit owner rebuild. It can never undercount a published record.
    records: u64,
    pending_records: u64,
    indexed: bool,
    legacy: bool,
    dirty: bool,
    first: u64,
    generations: u64,
    index_cursor: u64,
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
    let legacy = &b[..8] == b"ULMDBRG2" && b[8..10] == 2u16.to_le_bytes();
    let indexed_profile = &b[..8] == b"ULMDBRG3" && b[8..10] == 3u16.to_le_bytes();
    if !(legacy || indexed_profile)
        || b[10..12] != (HEADER_BYTES as u16).to_le_bytes()
        || b[12] > 3
        || b[13..16] != [0; 3]
        || (legacy && b[12] != 0)
        || b[16..32] != inner.uuid
        || b[32..40] != inner.identity.device.to_le_bytes()
        || b[40..56] != inner.identity.inode.to_le_bytes()
        || b[64] > 1
        || b[65..72] != [0; 7]
        || (legacy && b[88..120] != [0; 32])
        || b[112..120] != [0; 8]
        || b[120..124] != crate::publication::crc(&b[..120]).to_le_bytes()
        || b[124..] != [0; 4]
    {
        return Err(Error::Corrupt("pin registry profile binding"));
    }
    let active = u64::from_le_bytes(b[56..64].try_into().unwrap());
    let records = u64::from_le_bytes(b[72..80].try_into().unwrap());
    let pending_records = u64::from_le_bytes(b[80..88].try_into().unwrap());
    let indexed = b[12] & 1 != 0;
    let dirty = b[12] & 2 != 0;
    let first = u64::from_le_bytes(b[88..96].try_into().unwrap());
    let generations = u64::from_le_bytes(b[96..104].try_into().unwrap());
    let index_cursor = u64::from_le_bytes(b[104..112].try_into().unwrap());
    if dirty {
        return Err(Error::Corrupt("interrupted pin registry index mutation"));
    }
    if active == 0
        || active == index::NONE
        || index_cursor > active
        || (indexed
            && (generations == 0
                || generations > active + 1
                || first > active
                || index_cursor != 0))
        || (!indexed && (first != 0 || generations != 0))
        || records > MAX_LIMIT as u64
        || pending_records > MAX_PENDING_RECORDS
        || pending_records > records
    {
        return Err(Error::Corrupt("pin registry profile counters"));
    }
    Ok(Some(Profile {
        active,
        migrating: b[64] != 0,
        records,
        pending_records,
        indexed,
        legacy,
        dirty,
        first,
        generations,
        index_cursor,
    }))
}
fn store(inner: &Inner, file: &File, p: Profile) -> Result<()> {
    inner.check()?;
    let mut b = [0; HEADER_BYTES];
    b[..8].copy_from_slice(if p.legacy { b"ULMDBRG2" } else { b"ULMDBRG3" });
    b[8..10].copy_from_slice(&(if p.legacy { 2u16 } else { 3u16 }).to_le_bytes());
    b[12] = u8::from(p.indexed) | (u8::from(p.dirty) << 1);
    b[10..12].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    b[16..32].copy_from_slice(&inner.uuid);
    b[32..40].copy_from_slice(&inner.identity.device.to_le_bytes());
    b[40..56].copy_from_slice(&inner.identity.inode.to_le_bytes());
    b[56..64].copy_from_slice(&p.active.to_le_bytes());
    b[64] = u8::from(p.migrating);
    b[72..80].copy_from_slice(&p.records.to_le_bytes());
    b[80..88].copy_from_slice(&p.pending_records.to_le_bytes());
    b[88..96].copy_from_slice(&p.first.to_le_bytes());
    b[96..104].copy_from_slice(&p.generations.to_le_bytes());
    b[104..112].copy_from_slice(&p.index_cursor.to_le_bytes());
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
    // There are at most two fixed control entries, so finding a member is constant work.
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or(Error::Corrupt("invalid generation entry"))?;
        if name == SCAN || name == LINKS {
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
    let mut out = Vec::new();
    let ids = if p.indexed {
        Some(index::ids(inner, p)?.into_iter().collect::<BTreeSet<_>>())
    } else {
        None
    };
    let mut directories = 0u64;
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
        if id > p.active || ids.as_ref().is_some_and(|ids| !ids.contains(&id)) {
            return Err(Error::Corrupt("invalid registry generation"));
        }
        directories = directories.checked_add(1).ok_or(Error::LimitExceeded)?;
        let directory = entry.path();
        let _held = opened(&directory, false, true)?;
        for member in fs::read_dir(&directory)? {
            let member = member?;
            let name = member.file_name();
            let name = name
                .to_str()
                .ok_or(Error::Corrupt("invalid generation entry"))?;
            if name == SCAN || name == LINKS {
                continue;
            }
            push_path(inner, &mut out, member.path(), parse_name(name)?)?;
        }
    }
    if p.indexed && directories != p.generations {
        return Err(Error::Corrupt("pin generation directory count"));
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
    let m = match ulmdb_platform::path_identity(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if !m.is_file() || m.reparse {
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
    } else if p.migrating || p.records >= inner.limits.max_pin_records as u64 {
        // Conservative total reservations below the cap prove admission in
        // constant work. During migration unmoved flat pins are not counted.
        // At/above the cap preserve the live-record walk so stale reservations
        // do not consume an application's ordinary reader allowance.
        // Preserve the existing ordinary admission walk; only live ordinary
        // records charge that cap. Stale immutable generation members do not.
        let mut live = 0usize;
        let mut seen = BTreeSet::new();
        for (path, _) in paths(inner)?.unwrap() {
            let file = match opened(&path, false, false) {
                Ok(file) => file,
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            let object = ulmdb_platform::file_identity(&file)?;
            if !seen.insert((object.volume, object.file_id)) {
                continue;
            }
            let mut version = [0; 10];
            if file.metadata()?.len() < 10 {
                continue;
            }
            file.read_exact_at(&mut version, 0)?;
            if version[8..10] != 2u16.to_le_bytes() {
                continue;
            }
            match Lease::admit(file, false, false) {
                Ok(lease) => drop(lease),
                Err((Error::Busy, _)) => {
                    live += 1;
                    if live >= inner.limits.max_pin_records {
                        return Err(Error::LimitExceeded);
                    }
                }
                Err((e, _)) => return Err(e),
            }
        }
    }
    p.records = p.records.checked_add(1).ok_or(Error::LimitExceeded)?;
    store(inner, registry, p)?;
    Ok(Some(generation(inner, p.active)))
}
pub(super) fn pending_record(record: &File) -> Result<bool> {
    let mut b = [0; 10];
    if record.metadata()?.len() < 10 {
        return Ok(false);
    }
    record.read_exact_at(&mut b, 0)?;
    Ok(b[8..10] != 2u16.to_le_bytes())
}
pub(super) fn removed(inner: &Inner, registry: &File, pending: bool) -> Result<()> {
    if let Some(mut p) = profile(inner, registry)? {
        p.records = p
            .records
            .checked_sub(1)
            .ok_or(Error::Corrupt("pin registry count"))?;
        if pending {
            p.pending_records = p
                .pending_records
                .checked_sub(1)
                .ok_or(Error::Corrupt("pending registry count"))?;
        }
        store(inner, registry, p)?;
    }
    Ok(())
}

pub(super) fn cleanup_leases(inner: &Inner, registry: &File) -> Result<Vec<(u64, File, Lease)>> {
    let Some(p) = profile(inner, registry)? else {
        return Ok(Vec::new());
    };
    if p.migrating || !p.indexed {
        return Err(Error::Busy);
    }
    if p.generations == 1 && p.first == p.active {
        // No sealed membership needs exclusion. The caller immediately runs
        // paths() under this same Registry lease; it still validates links,
        // counts and every directory before granting any unlink authority.
        return Ok(Vec::new());
    }
    let mut leases = Vec::new();
    for id in index::ids(inner, p)? {
        if id == p.active {
            continue;
        }
        // A scanner holding generation SH makes this return Busy. No sealed
        // member can then be removed while a resumable ReadDir is alive.
        let directory = opened(&generation(inner, id), false, true)?;
        let lease = gate(&generation(inner, id).join(SCAN), false)?;
        leases.push((id, directory, lease));
    }
    Ok(leases)
}

pub(super) fn retire_empty(
    inner: &Inner,
    registry: &File,
    generations: &[(u64, File, Lease)],
) -> Result<()> {
    for (id, directory, exclusion) in generations {
        index::retire(inner, registry, *id, directory, exclusion)?;
    }
    Ok(())
}

/// Source protection for one boundedly validated physical record. Its sequence
/// and commit are record claims until an authenticated descriptor agrees.
/// There is intentionally no Manifest, graph or currentness accessor.
pub struct PendingPhysicalPin {
    inner: Arc<Inner>,
    path: PathBuf,
    lease: Lease,
    checkpoint: Option<[u8; 64]>,
    generation: u128,
    sequence: Option<u64>,
    commit: Option<u128>,
    birth: Option<Birth>,
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
        let named = ulmdb_platform::path_identity(&self.path)?;
        let held = ulmdb_platform::file_identity(self.lease.file())?;
        if !named.is_file()
            || named.reparse
            || (Identity {
                device: named.volume,
                inode: named.file_id,
            }) != (Identity {
                device: held.volume,
                inode: held.file_id,
            })
            || named.length != RECORD_BYTES as u64
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
            // Active temporary copies can retire immediately. A sealed walk's
            // membership SH makes release defer rather than mutate its entries.
            let _ = release(&self.inner, &self.path, self.lease.file(), self.birth);
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
    seek: Option<u64>,
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
        let mut p = match profile(&self.inner, registry.file())? {
            Some(p) => p,
            None => {
                let p = Profile {
                    active: 1,
                    migrating: true,
                    records: 0,
                    pending_records: 0,
                    indexed: false,
                    legacy: false,
                    dirty: false,
                    first: 0,
                    generations: 0,
                    index_cursor: 0,
                };
                // Publish the rejecting header before adding directory entries.
                // A failure is fail-closed; no physical files are reclaimed.
                store(&self.inner, registry.file(), p)?;
                create_generation(&self.inner, 0)?;
                create_generation(&self.inner, 1)?;
                p
            }
        };
        if p.migrating {
            create_generation(&self.inner, 0)?;
            create_generation(&self.inner, p.active)?;
        }
        if p.legacy {
            // Explicit RG3 negotiation fences old clients before index controls
            // appear. Ordinary RG2 admission/release alone never upgrades it.
            p.legacy = false;
            store(&self.inner, registry.file(), p)?;
        }
        self.check()?;
        Ok(PinDiscovery {
            inner: self.inner.clone(),
            next: 0,
            end: p.active - 1,
            seek: None,
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
        if self.inner.identity != owner.inner.identity
            || self.inner.uuid != owner.inner.uuid
            || self.inner.directory != owner.inner.directory
            || self.inner.limits.max_pin_records != owner.inner.limits.max_pin_records
            || self.inner.limits.max_inventory_entries != owner.inner.limits.max_inventory_entries
        {
            return Err(Error::InvalidInput("foreign pin discovery owner"));
        }
        owner.check()
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
        let mut p = profile(&self.inner, registry.file())?
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
                store(&self.inner, registry.file(), p)?;
                break;
            };
            let metadata = match ulmdb_platform::path_identity(&source) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if !metadata.is_file() || metadata.reparse {
                return Err(Error::Corrupt("nonregular legacy pin"));
            }
            // Reserve before the namespace change: errors may overcount, never
            // lose a source lease from the quota or bypass the configured cap.
            p.records = p.records.checked_add(1).ok_or(Error::LimitExceeded)?;
            store(&self.inner, registry.file(), p)?;
            let target = generation(&self.inner, 0).join(source.file_name().unwrap());
            match migrate_member(&source, &target) {
                Ok(()) => {}
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    p.records -= 1;
                    store(&self.inner, registry.file(), p)?;
                }
                Err(e) => return Err(e),
            }
        }
        if p.migrating {
            return Ok(out);
        }
        while !p.indexed && out.work_units < budget.records {
            index::index_one(&self.inner, registry.file(), &mut p)?;
            out.work_units += 1;
        }
        if !p.indexed {
            return Ok(out);
        }
        while self.next <= self.end && out.work_units < budget.records {
            if let Some(cleanup) = self.cleanup.as_mut() {
                if cleanup.exclusion.is_none() {
                    match gate(&generation(&self.inner, cleanup.id).join(SCAN), false) {
                        Ok(lease) => cleanup.exclusion = Some(lease),
                        Err(Error::Busy) => return Ok(out),
                        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                            self.cleanup = None;
                            self.seek = None;
                            out.work_units += 1;
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                let named = ulmdb_platform::path_identity(&generation(&self.inner, cleanup.id))?;
                if !named.is_dir()
                    || named.reparse
                    || (Identity {
                        device: named.volume,
                        inode: named.file_id,
                    }) != cleanup.identity
                    || Identity::file(&cleanup.directory)? != cleanup.identity
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
                    match Lease::admit(file, false, false) {
                        Ok(stale) => {
                            let pending = pending_record(stale.file())?;
                            if remove_owned(&path, stale.file())? {
                                removed(&self.inner, registry.file(), pending)?;
                            }
                        }
                        Err((Error::Busy, _)) => {}
                        Err((e, _)) => return Err(e),
                    }
                    continue;
                }
                let links = index::read(&self.inner, cleanup.id)?;
                index::retire(
                    &self.inner,
                    registry.file(),
                    cleanup.id,
                    &cleanup.directory,
                    cleanup.exclusion.as_ref().unwrap(),
                )?;
                self.cleanup = None;
                self.next = self.next.checked_add(1).ok_or(Error::LimitExceeded)?;
                self.seek = Some(links.next);
                p = profile(&self.inner, registry.file())?
                    .ok_or(Error::Corrupt("pin discovery profile lost"))?;
                continue;
            }
            if self.walk.is_none() {
                let id = self.seek.take().unwrap_or(p.first);
                if id == index::NONE || id > self.end {
                    self.next = self.end.checked_add(1).ok_or(Error::LimitExceeded)?;
                    break;
                }
                let links = match index::read(&self.inner, id) {
                    Ok(links) => links,
                    // Another exclusive owner may already have retired a saved
                    // candidate. Restart from the authoritative linked head.
                    Err(Error::Io(e))
                        if e.kind() == std::io::ErrorKind::NotFound && id != p.first =>
                    {
                        out.work_units += 1;
                        self.seek = Some(p.first);
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let links = index::check_edges(&self.inner, p, id, links)?;
                if id < self.next {
                    out.work_units += 1;
                    self.seek = Some(links.next);
                    continue;
                }
                self.next = id;
                let path = generation(&self.inner, self.next);
                let directory = opened(&path, false, true)?;
                let membership = gate(&path.join(SCAN), true)?;
                self.inner.check()?;
                let identity = Identity::file(&directory)?;
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
            let named = ulmdb_platform::path_identity(&path)?;
            if !named.is_dir()
                || named.reparse
                || (Identity {
                    device: named.volume,
                    inode: named.file_id,
                }) != walk.identity
                || Identity::file(&walk.directory)? != walk.identity
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
            if name == SCAN || name == LINKS {
                continue;
            }
            let nonce = parse_name(name)?;
            let path = entry.path();
            if self.own_records.contains(&path) {
                continue;
            }
            let file = opened(&path, false, false)?;
            let file = match Lease::admit(file, false, false) {
                Ok(lease) => {
                    drop(lease);
                    walk.stale.push_back(path);
                    continue;
                }
                Err((Error::Busy, file)) => file,
                Err((e, _)) => return Err(e),
            };
            let mut bytes = [0; RECORD_BYTES];
            file.read_exact_at(&mut bytes, 0)?;
            validate_record(&self.inner, &file, &bytes, nonce)?;
            let (checkpoint, generation, sequence, commit) = record_source(&bytes)?;
            // Copy paths remain skip identities until this continuation ends,
            // even if active-owner Drop has already removed their records.
            if self.own_records.len() >= MAX_PENDING_RECORDS as usize {
                return Err(Error::LimitExceeded);
            }
            let directory = reserve_record(&self.inner, registry.file(), true)?
                .ok_or(Error::Corrupt("pin discovery profile lost"))?;
            let birth = birth(&self.inner, &directory)?;
            let new_nonce = new_identity()?;
            let new_path = directory.join(format!("pin-{new_nonce:032x}.lease"));
            let new_file = create_record(&new_path)?;
            let lease = Lease::acquire(new_file, false)?;
            if bytes[8..10] == 2u16.to_le_bytes() {
                bytes[8..10].copy_from_slice(&5u16.to_le_bytes());
            }
            bytes[144..160].copy_from_slice(&new_nonce.to_le_bytes());
            let crc = crate::publication::crc(&bytes[..160]);
            bytes[160..164].copy_from_slice(&crc.to_le_bytes());
            lease.file().write_all_at(&bytes, 0)?;
            self.own_records.insert(new_path.clone());
            out.pins.push(PendingPhysicalPin {
                inner: self.inner.clone(),
                path: new_path,
                lease,
                checkpoint,
                generation,
                sequence,
                commit,
                birth,
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
        let mut p = profile(&self.inner, registry.file())?
            .ok_or(Error::Corrupt("pin discovery profile lost"))?;
        if p.migrating || !p.indexed {
            return Ok(true);
        }
        if is_empty(&self.inner, p.active)? {
            owner.check()?;
            return Ok(false);
        }
        self.end = p.active;
        index::append(&self.inner, registry.file(), &mut p)?;
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
        || !(2..=5).contains(&u16::from_le_bytes(b[8..10].try_into().unwrap()))
        || b[10..12] != (RECORD_BYTES as u16).to_le_bytes()
        || b[12..16] != [0; 4]
        || b[164..] != [0; 12]
        || b[160..164] != crate::publication::crc(&b[..160]).to_le_bytes()
        || b[16..24] != inner.identity.device.to_le_bytes()
        || b[24..40] != inner.identity.inode.to_le_bytes()
        || b[40..56] != inner.uuid
        || b[144..160] != nonce.to_le_bytes()
    {
        return Err(Error::Corrupt("pending physical pin binding"));
    }
    Ok(())
}

pub(super) fn record_source(
    b: &[u8; RECORD_BYTES],
) -> Result<(Option<[u8; 64]>, u128, Option<u64>, Option<u128>)> {
    if b[8..10] != 3u16.to_le_bytes() {
        let checkpoint: [u8; 64] = b[56..120].try_into().unwrap();
        let generation = checkpoint_identity(&checkpoint)?.0;
        if b[8..10] == 4u16.to_le_bytes() {
            if b[120..144] != [0; 24] {
                return Err(Error::Corrupt("opaque checkpoint pin identity"));
            }
            Ok((Some(checkpoint), generation, None, None))
        } else {
            Ok((
                Some(checkpoint),
                generation,
                Some(u64::from_le_bytes(b[120..128].try_into().unwrap())),
                Some(u128::from_le_bytes(b[128..144].try_into().unwrap())),
            ))
        }
    } else {
        let generation = u128::from_le_bytes(b[56..72].try_into().unwrap());
        if generation == 0 || b[72..144] != [0; 72] {
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
        let directory = reserve_record(&self.inner, registry.file(), true)?
            .ok_or(Error::InvalidInput("generation registry not enabled"))?;
        let birth = birth(&self.inner, &directory)?;
        let nonce = new_identity()?;
        let path = directory.join(format!("pin-{nonce:032x}.lease"));
        let file = create_record(&path)?;
        let lease = Lease::acquire(file, false)?;
        let mut b = [0; RECORD_BYTES];
        b[..8].copy_from_slice(b"ULMDBPIN");
        b[8..10].copy_from_slice(&3u16.to_le_bytes());
        b[10..12].copy_from_slice(&(RECORD_BYTES as u16).to_le_bytes());
        b[16..24].copy_from_slice(&self.inner.identity.device.to_le_bytes());
        b[24..40].copy_from_slice(&self.inner.identity.inode.to_le_bytes());
        b[40..56].copy_from_slice(&self.inner.uuid);
        b[56..72].copy_from_slice(&generation.to_le_bytes());
        b[144..160].copy_from_slice(&nonce.to_le_bytes());
        let crc = crate::publication::crc(&b[..160]);
        b[160..164].copy_from_slice(&crc.to_le_bytes());
        lease.file().write_all_at(&b, 0)?;
        self.check()?;
        Ok(Some(PendingPhysicalPin {
            inner: self.inner.clone(),
            path,
            lease,
            checkpoint: None,
            generation,
            sequence: None,
            commit: None,
            birth,
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
        let directory = reserve_record(&self.inner, registry.file(), true)?
            .ok_or(Error::InvalidInput("generation registry not enabled"))?;
        let birth = birth(&self.inner, &directory)?;
        let nonce = new_identity()?;
        let path = directory.join(format!("pin-{nonce:032x}.lease"));
        let file = create_record(&path)?;
        let lease = Lease::acquire(file, false)?;
        let mut b = [0; RECORD_BYTES];
        b[..8].copy_from_slice(b"ULMDBPIN");
        b[8..10].copy_from_slice(&4u16.to_le_bytes());
        b[10..12].copy_from_slice(&(RECORD_BYTES as u16).to_le_bytes());
        b[16..24].copy_from_slice(&self.inner.identity.device.to_le_bytes());
        b[24..40].copy_from_slice(&self.inner.identity.inode.to_le_bytes());
        b[40..56].copy_from_slice(&self.inner.uuid);
        b[56..120].copy_from_slice(checkpoint);
        b[144..160].copy_from_slice(&nonce.to_le_bytes());
        let crc = crate::publication::crc(&b[..160]);
        b[160..164].copy_from_slice(&crc.to_le_bytes());
        lease.file().write_all_at(&b, 0)?;
        self.check()?;
        Ok(PendingPhysicalPin {
            inner: self.inner.clone(),
            path,
            lease,
            checkpoint: Some(*checkpoint),
            generation,
            sequence: None,
            commit: None,
            birth,
        })
    }
}

fn create_record(path: &Path) -> Result<File> {
    let parent = opened(
        path.parent().ok_or(Error::InvalidInput("pin parent"))?,
        false,
        true,
    )?;
    Ok(ulmdb_platform::open_relative_writable(
        &parent,
        path.file_name().ok_or(Error::InvalidInput("pin name"))?,
        RelativeKind::File,
        true,
        true,
    )?)
}

// Open the original link before adding its immutable generation member. Windows
// disposition acts on that handle's original link, never the target link.
fn migrate_member(source: &Path, target: &Path) -> Result<()> {
    let parent = opened(
        source
            .parent()
            .ok_or(Error::InvalidInput("migration parent"))?,
        false,
        true,
    )?;
    let file = ulmdb_platform::open_relative_writable(
        &parent,
        source
            .file_name()
            .ok_or(Error::InvalidInput("migration name"))?,
        RelativeKind::File,
        false,
        false,
    )?;
    let held = ulmdb_platform::file_identity(&file)?;
    let named = ulmdb_platform::path_identity(source)?;
    if !held.is_file()
        || !named.is_file()
        || !held.same_object(&named)
        || !(1..=2).contains(&held.links)
    {
        return Err(Error::Corrupt("migration source binding"));
    }
    match fs::hard_link(source, target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let member = ulmdb_platform::path_identity(target)?;
    if !member.is_file() || !member.same_object(&held) {
        return Err(Error::Corrupt("migration member binding"));
    }
    match ulmdb_platform::path_identity(source) {
        Ok(now) if now.is_file() && now.same_object(&held) => {
            ulmdb_platform::remove_owned_file(&file)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(Error::Corrupt("migration source replaced")),
    }
    let member = ulmdb_platform::path_identity(target)?;
    if !member.is_file() || !member.same_object(&held) {
        return Err(Error::Corrupt("migration member changed"));
    }
    Ok(())
}
