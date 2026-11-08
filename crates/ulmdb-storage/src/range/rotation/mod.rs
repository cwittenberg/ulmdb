//! Explicit selected-pack/floor protocol. Never rewrites a selected prefix.
mod anchor;
mod preparation;
use super::{Backend, FILE, MARKER, RangeSnapshot, RangeStore, direct, pack};
use crate::{Arena, Error, Manifest, ResidencyController, Result, WriterFence};
pub(crate) use anchor::Anchor;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_ASSETS: usize = 4096;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Selection {
    pub file: [u8; 16],
    pub uuid: [u8; 16],
    pub floor: [u8; 16],
    pub rotated: bool,
}
pub(crate) fn selection(directory: &Path) -> Result<Selection> {
    let mut b = Vec::new();
    crate::read_mode::ReadMode::Sync
        .open(&directory.join(MARKER))?
        .take(97)
        .read_to_end(&mut b)?;
    if b.len() != 96
        || &b[..8] != b"ULMDBB06"
        || b[10..12] != 6u16.to_le_bytes()
        || b[12..16] != 96u32.to_le_bytes()
        || b[16..32] == [0; 16]
        || b[32..48] == [0; 16]
        || <[u8; 32]>::from(Sha256::digest(&b[..64])) != b[64..]
    {
        return Err(Error::Corrupt("range selector header/hash"));
    }
    let rotated = b[8..10] == 1u16.to_le_bytes();
    if (!rotated && (b[8..10] != [0; 2] || b[48..64] != [0; 16]))
        || (rotated && b[48..64] == [0; 16])
    {
        return Err(Error::Corrupt("range selector version/floor"));
    }
    let file = b[16..32].try_into().unwrap();
    Ok(Selection {
        file,
        uuid: b[32..48].try_into().unwrap(),
        floor: if rotated {
            b[48..64].try_into().unwrap()
        } else {
            file
        },
        rotated,
    })
}
fn encoded(s: Selection) -> [u8; 96] {
    let mut b = direct::checkpoint_marker(s.file, s.uuid);
    b[8..10].copy_from_slice(&1u16.to_le_bytes());
    b[48..64].copy_from_slice(&s.floor);
    let hash = Sha256::digest(&b[..64]);
    b[64..].copy_from_slice(&hash);
    b
}
pub(crate) fn named(directory: &Path, id: [u8; 16], extension: &str) -> PathBuf {
    let mut hex = String::with_capacity(32);
    for b in id {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    directory.join(format!("RANGE.{hex}.{extension}"))
}
pub(crate) fn asset(directory: &Path, id: [u8; 16]) -> Result<PathBuf> {
    optional_asset(directory, id)?
        .ok_or_else(|| Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound)))
}
fn optional_asset(directory: &Path, id: [u8; 16]) -> Result<Option<PathBuf>> {
    let p = named(directory, id, "pack");
    match fs::symlink_metadata(&p) {
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(Error::Corrupt("range asset file type"));
            }
            return Ok(Some(p));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let p = directory.join(FILE);
    match fs::symlink_metadata(&p) {
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(Error::Corrupt("range initial asset file type"));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let f = crate::read_mode::ReadMode::Sync.open(&p)?;
    if global(&f)?.file_id == id {
        Ok(Some(p))
    } else {
        Ok(None)
    }
}
pub(crate) fn global(file: &File) -> Result<pack::codec::Global> {
    let mut b = [0; pack::BLOCK];
    pack::io::read_exact_at(file, &mut b, 0)?;
    pack::codec::decode_global(&b)
}
pub(crate) fn global_bytes(file: &File) -> Result<[u8; pack::BLOCK]> {
    let mut b = [0; pack::BLOCK];
    pack::io::read_exact_at(file, &mut b, 0)?;
    pack::codec::decode_global(&b)?;
    Ok(b)
}
pub(crate) fn archived_global(
    directory: &Path,
    id: [u8; 16],
    expected: [u8; 32],
) -> Result<pack::codec::Global> {
    let p = named(directory, id, "anchor");
    let metadata = fs::symlink_metadata(&p)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != pack::BLOCK as u64
    {
        return Err(Error::Corrupt("range anchor file type/length"));
    }
    let f = File::open(p)?;
    let b = global_bytes(&f)?;
    if <[u8; 32]>::from(Sha256::digest(b)) != expected {
        return Err(Error::Corrupt("range anchor lineage hash"));
    }
    let g = pack::codec::decode_global(&b)?;
    if g.file_id != id {
        return Err(Error::Corrupt("range anchor fileID"));
    }
    Ok(g)
}
fn publish(
    directory: &Path,
    expected: Selection,
    next: Selection,
    namespace: &super::checkpoint::Namespace,
    mut preparation: Option<&mut preparation::Preparation>,
) -> Result<()> {
    if !namespace.matches(directory)? {
        return Err(Error::StaleHandle);
    }
    if selection(directory)? != expected {
        return Err(Error::Busy);
    }
    let (path, mut file) = crate::temporary::create(directory, "BACKEND")?;
    let attempt = (|| {
        file.write_all(&encoded(next))?;
        ulmdb_platform::sync_file(&file)?;
        if selection(directory)? != expected {
            return Err(Error::Busy);
        }
        if !namespace.matches(directory)? {
            return Err(Error::StaleHandle);
        }
        if let Some(candidate) = preparation.as_mut() {
            candidate.selector_attempted();
        }
        // From the first selector syscall onward even an error may leave a
        // visible selector, so preserve the candidate and report uncertainty.
        ulmdb_platform::rename_replace(&path, &directory.join(MARKER))
            .map_err(|_| Error::OutcomeUnknown)?;
        if !namespace.matches(directory).unwrap_or(false) {
            return Err(Error::OutcomeUnknown);
        }
        ulmdb_platform::sync_directory(directory).map_err(|_| Error::OutcomeUnknown)?;
        Ok(())
    })();
    if attempt.is_err() && namespace.matches(directory).unwrap_or(false) {
        if let (Ok(named), Ok(owned)) = (
            ulmdb_platform::path_identity(&path),
            ulmdb_platform::file_identity(&file),
        ) {
            if named.is_file() && owned.is_file() && named.same_object(&owned) {
                let _ = fs::remove_file(path);
            }
        }
    }
    attempt
}
pub(crate) fn stable_writer(directory: &Path) -> Result<WriterFence> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("RANGE-WRITER"))?;
    if !file.metadata()?.is_file()
        || !ulmdb_platform::file_identity(&file)?.same_object(&ulmdb_platform::path_identity(
            &directory.join("RANGE-WRITER"),
        )?)
    {
        return Err(Error::Corrupt("range writer gate identity"));
    }
    WriterFence::try_lock(file).map_err(|_| Error::Busy)
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RangeReclaimReport {
    pub scanned_assets: usize,
    pub removed_assets: usize,
    pub removed_bytes: u64,
    pub busy_assets: usize,
}
impl RangeStore {
    /// Whole-asset rotation. Native constructs/validates the compacted candidate;
    /// this method still binds exact current authority and logical identity.
    pub fn rotate_checked<F: FnOnce(&crate::SegmentTable, &Manifest) -> Result<()>>(
        &mut self,
        directory: &Path,
        source: &super::RangeCheckpoint,
        arena: Arena,
        mut manifest: Manifest,
        controller: &ResidencyController,
        validate: F,
    ) -> Result<RangeSnapshot> {
        self.check_lifecycle_owner()?;
        let namespace = self.namespace.as_ref().unwrap().clone();
        if !namespace.matches(directory)? || !source.matches_directory(directory)? {
            return Err(Error::StaleHandle);
        }
        let expected = selection(directory)?;
        if source.physical_asset()?.file_id != expected.file {
            return Err(Error::StaleHandle);
        }
        let Backend::Direct(store) = &self.backend else {
            return Err(Error::InvalidInput("range rotation requires format6"));
        };
        store.check_rotation_source(source)?;
        // Bound the certificate chain before allocating a replacement asset.
        let mut depth_global = global(&source.reader())?;
        let mut depth_file = expected.file;
        let mut depth = 0;
        while depth_file != expected.floor {
            if depth >= MAX_ASSETS - 2 {
                return Err(Error::LimitExceeded);
            }
            let prior = depth_global
                .anchor
                .ok_or(Error::Corrupt("rotation floor lineage"))?;
            depth_file = prior.file();
            depth_global = archived_global(directory, depth_file, prior.source_global)?;
            depth += 1;
        }
        let anchor = source.rotation_anchor(&manifest)?;
        manifest.previous = 0;
        let id = crate::new_identity()?.to_le_bytes();
        let path = named(directory, id, "pack");
        let mut preparation = preparation::Preparation::create(directory, id, namespace.clone())?;
        let attempt = (|| {
            let (next, state) = direct::Store::create_rotated(
                &path,
                preparation.writer_file()?,
                id,
                anchor,
                arena,
                manifest,
                controller,
                validate,
            )?;
            let state = self.bind_snapshot(state);
            if !state
                .checkpoint
                .as_ref()
                .unwrap()
                .matches_directory(directory)?
            {
                return Err(Error::Corrupt("range candidate asset replaced"));
            }
            let bytes = global_bytes(&source.reader())?;
            preparation.archive_global(expected.file, &bytes)?;
            preparation.archive_global(id, &global_bytes(&File::open(&path)?)?)?;
            ulmdb_platform::sync_directory(directory)?;
            if !namespace.matches(directory)? {
                return Err(Error::Corrupt("range rotation namespace replaced"));
            }
            if let Err(e) = publish(
                directory,
                expected,
                Selection {
                    file: id,
                    uuid: expected.uuid,
                    floor: expected.floor,
                    rotated: true,
                },
                &namespace,
                Some(&mut preparation),
            ) {
                if matches!(e, Error::OutcomeUnknown) {
                    self.poison_rotation();
                }
                return Err(e);
            }
            self.backend = Backend::Direct(next);
            Ok(state)
        })();
        // Drop all private writer/maps before explicit cleanup; the preparation
        // still owns the exact original create_new descriptors. Drop also backs
        // this up for panic/early-return paths. A selector attempt disables it.
        if attempt.is_err() {
            preparation.cleanup()?;
        }
        attempt
    }
    fn poison_rotation(&mut self) {
        if let Backend::Direct(s) = &mut self.backend {
            s.poison();
        }
    }
    /// Bounded deletion of complete old assets only. Pins and live shared leases
    /// determine the oldest authorized lineage floor before any unlink.
    pub fn reclaim_rotated(
        &mut self,
        directory: &Path,
        source: &super::RangeCheckpoint,
        budget: usize,
    ) -> Result<RangeReclaimReport> {
        self.check_lifecycle_owner()?;
        if budget == 0 || budget > MAX_ASSETS {
            return Err(Error::LimitExceeded);
        }
        let ns = self.namespace.as_ref().unwrap();
        if !ns.matches(directory)? {
            return Err(Error::StaleHandle);
        }
        let selected = selection(directory)?;
        if source.physical_asset()?.file_id != selected.file
            || !source.matches_directory(directory)?
        {
            return Err(Error::StaleHandle);
        }
        let pins = retained_records(directory, source)?;
        if !selected.rotated {
            return Ok(RangeReclaimReport::default());
        }
        let mut at = selected.file;
        let mut g = global(&source.reader())?;
        let mut chain = Vec::new();
        let mut floor_index = 0usize;
        let mut report = RangeReclaimReport::default();
        let mut seen = std::collections::BTreeSet::new();
        let mut reached_floor = false;
        loop {
            if chain.len() == MAX_ASSETS || !seen.insert(at) {
                return Err(Error::Corrupt("range lineage cycle/bound"));
            }
            let path = optional_asset(directory, at)?;
            // Discovery holds no per-asset descriptor. Only the explicitly
            // bounded deletion batch later owns exclusive leases and FDs.
            chain.push((at, path, None));
            report.scanned_assets += 1;
            reached_floor |= at == selected.floor;
            let Some(a) = g.anchor else {
                if !reached_floor {
                    return Err(Error::Corrupt("range floor outside anchor chain"));
                }
                break;
            };
            at = a.file();
            match archived_global(directory, at, a.source_global) {
                Ok(prior) => g = prior,
                Err(Error::Io(e))
                    if reached_floor
                        && e.kind() == std::io::ErrorKind::NotFound
                        && optional_asset(directory, at)?.is_none() =>
                {
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        for pin in &pins {
            if pin[32..48] != selected.uuid
                || !seen.contains(&<[u8; 16]>::try_from(&pin[16..32]).unwrap())
            {
                return Err(Error::Corrupt("range pin outside retained floor"));
            }
        }
        let mut remove = std::collections::BTreeSet::new();
        for (index, (id, path, lease)) in chain.iter_mut().enumerate().rev() {
            if remove.len() == budget {
                break;
            }
            if *id == selected.file || pins.iter().any(|p| p[16..32] == *id) {
                continue;
            }
            let Some(path) = path else {
                continue;
            };
            let expected = File::open(&path)?;
            match crate::hybrid::retirement::ExclusiveLease::try_acquire(&path, &expected) {
                Ok(owned) => {
                    *lease = Some((owned, expected));
                    remove.insert(index);
                }
                Err(Error::Busy) => report.busy_assets += 1,
                Err(e) => return Err(e),
            }
        }
        for (index, (_, path, _)) in chain.iter().enumerate() {
            if path.is_some() && !remove.contains(&index) {
                floor_index = floor_index.max(index);
            }
        }
        let next_floor = chain[floor_index].0;
        if !ns.matches(directory)? {
            return Err(Error::Corrupt("range reclaim namespace replaced"));
        }
        if next_floor != selected.floor {
            publish(
                directory,
                selected,
                Selection {
                    floor: next_floor,
                    ..selected
                },
                ns,
                None,
            )?;
        }
        for index in remove.into_iter().rev() {
            let (id, path, lease) = &chain[index];
            let path = path.as_ref().unwrap();
            let (_guard, expected) = lease.as_ref().unwrap();
            if !ns.matches(directory)?
                || !ulmdb_platform::path_identity(path)?
                    .same_object(&ulmdb_platform::file_identity(expected)?)
            {
                return Err(Error::Corrupt("range reclaim asset/namespace replaced"));
            }
            let len = expected.metadata()?.len();
            fs::remove_file(path)?;
            report.removed_assets += 1;
            report.removed_bytes += len;
            if index > floor_index {
                let _ = fs::remove_file(named(directory, *id, "anchor"));
            }
        }
        ulmdb_platform::sync_directory(directory)?;
        Ok(report)
    }
}

fn retained_records(directory: &Path, source: &super::RangeCheckpoint) -> Result<Vec<[u8; 256]>> {
    let path = directory.join("retained-range");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Corrupt("Range pin directory"));
    }
    let mut pins = Vec::new();
    let demand = ResidencyController::new(crate::ResidencyPolicy::Demand)?;
    for (scanned, item) in fs::read_dir(path)?.enumerate() {
        if scanned >= MAX_ASSETS * 2 {
            return Err(Error::LimitExceeded);
        }
        let item = item?;
        let name = item.file_name();
        let Some(name) = name.to_str().and_then(|n| n.strip_suffix(".pin")) else {
            continue;
        };
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::Corrupt("Range pin name"));
        }
        let m = fs::symlink_metadata(item.path())?;
        if !m.is_file() || m.file_type().is_symlink() || m.len() != 256 {
            return Err(Error::Corrupt("Range pin file"));
        }
        if pins.len() == MAX_ASSETS {
            return Err(Error::LimitExceeded);
        }
        let mut b = [0; 256];
        let mut f = File::open(item.path())?;
        f.read_exact(&mut b)?;
        source.open_retained_checked(&b, &demand, |_, _| Ok(()))?;
        pins.push(b);
    }
    Ok(pins)
}
