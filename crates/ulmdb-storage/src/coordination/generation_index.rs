//! Registry-serialized index of generations which still own namespace entries.
//! Generation ids are never reused. All mutations publish a rejecting dirty
//! header before changing links; an interrupted mutation therefore grants no
//! discovery or reclamation authority.
use super::*;

pub(super) const LINKS: &str = "_links";
pub(super) const NONE: u64 = u64::MAX;
const LINK_BYTES: usize = 128;

#[derive(Clone, Copy)]
pub(super) struct Links {
    pub previous: u64,
    pub next: u64,
}

fn identity(file: &File) -> Result<Identity> {
    #[cfg(not(windows))]
    {
        Ok(Identity::of(&file.metadata()?))
    }
    #[cfg(windows)]
    {
        Identity::file(file)
    }
}

fn identity_bytes(value: Identity, bytes: &mut [u8]) {
    bytes.fill(0);
    bytes[..8].copy_from_slice(&value.device.to_le_bytes());
    bytes[8..8 + std::mem::size_of_val(&value.inode)].copy_from_slice(&value.inode.to_le_bytes());
}

fn binding(inner: &Inner, id: u64) -> Result<[u8; LINK_BYTES]> {
    let directory = opened(&generation(inner, id), false, true)?;
    let mut bytes = [0; LINK_BYTES];
    bytes[..8].copy_from_slice(b"ULMDBRL3");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(LINK_BYTES as u16).to_le_bytes());
    bytes[16..32].copy_from_slice(&inner.uuid);
    identity_bytes(inner.identity, &mut bytes[32..56]);
    bytes[56..64].copy_from_slice(&id.to_le_bytes());
    identity_bytes(identity(&directory)?, &mut bytes[64..88]);
    Ok(bytes)
}

pub(super) fn read(inner: &Inner, id: u64) -> Result<Links> {
    let file = opened(&generation(inner, id).join(LINKS), false, false)?;
    if file.metadata()?.len() != LINK_BYTES as u64 {
        return Err(Error::Corrupt("pin generation links length"));
    }
    let mut bytes = [0; LINK_BYTES];
    file.read_exact_at(&mut bytes, 0)?;
    let expected = binding(inner, id)?;
    let previous = u64::from_le_bytes(bytes[88..96].try_into().unwrap());
    let next = u64::from_le_bytes(bytes[96..104].try_into().unwrap());
    if bytes[..88] != expected[..88]
        || bytes[104..120] != [0; 16]
        || bytes[120..124] != crate::publication::crc(&bytes[..120]).to_le_bytes()
        || bytes[124..] != [0; 4]
        || (previous != NONE && previous >= id)
        || (next != NONE && next <= id)
    {
        return Err(Error::Corrupt("pin generation links binding"));
    }
    Ok(Links { previous, next })
}

/// Validate both neighboring edges before a bounded traversal trusts one.
pub(super) fn checked(inner: &Inner, p: Profile, id: u64) -> Result<Links> {
    let links = read(inner, id)?;
    check_edges(inner, p, id, links)
}

pub(super) fn check_edges(inner: &Inner, p: Profile, id: u64, links: Links) -> Result<Links> {
    let neighbor = |id| match read(inner, id) {
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(Error::Corrupt("pin generation index neighbor missing"))
        }
        result => result,
    };
    if id > p.active
        || ((links.previous == NONE) != (id == p.first))
        || ((links.next == NONE) != (id == p.active))
        || (links.previous != NONE && neighbor(links.previous)?.next != id)
        || (links.next != NONE && (links.next > p.active || neighbor(links.next)?.previous != id))
    {
        return Err(Error::Corrupt("pin generation index edge"));
    }
    Ok(links)
}

fn write(inner: &Inner, id: u64, links: Links, create: bool) -> Result<()> {
    let mut bytes = binding(inner, id)?;
    bytes[88..96].copy_from_slice(&links.previous.to_le_bytes());
    bytes[96..104].copy_from_slice(&links.next.to_le_bytes());
    let crc = crate::publication::crc(&bytes[..120]);
    bytes[120..124].copy_from_slice(&crc.to_le_bytes());
    let file = opened(&generation(inner, id).join(LINKS), create, false)?;
    file.write_all_at(&bytes, 0)?;
    file.set_len(LINK_BYTES as u64)?;
    inner.check()
}

/// RG2 upgrade is resumable, one historical generation per caller work unit.
/// Until the final header publication no links or absences are authoritative.
pub(super) fn index_one(inner: &Inner, registry: &File, p: &mut Profile) -> Result<()> {
    let id = p.index_cursor;
    let scan = opened(&generation(inner, id).join(SCAN), false, false)?;
    if scan.metadata()?.len() != 0 {
        return Err(Error::Corrupt("nonempty generation scan gate"));
    }
    write(
        inner,
        id,
        Links {
            previous: if id == 0 { NONE } else { id - 1 },
            next: if id == p.active { NONE } else { id + 1 },
        },
        true,
    )?;
    if id == p.active {
        p.indexed = true;
        p.first = 0;
        p.generations = p.active.checked_add(1).ok_or(Error::LimitExceeded)?;
        p.index_cursor = 0;
    } else {
        p.index_cursor = id.checked_add(1).ok_or(Error::LimitExceeded)?;
    }
    store(inner, registry, *p)
}

pub(super) fn ids(inner: &Inner, p: Profile) -> Result<Vec<u64>> {
    if !p.indexed {
        return Err(Error::Busy);
    }
    let mut ids = Vec::new();
    let mut id = p.first;
    let mut previous = NONE;
    while id != NONE {
        if ids.len() as u64 >= p.generations || id > p.active {
            return Err(Error::Corrupt("pin generation index count"));
        }
        let links = read(inner, id)?;
        if links.previous != previous {
            return Err(Error::Corrupt("pin generation index predecessor"));
        }
        ids.push(id);
        previous = id;
        id = links.next;
    }
    if ids.len() as u64 != p.generations || previous != p.active {
        return Err(Error::Corrupt("pin generation index tail"));
    }
    Ok(ids)
}

pub(super) fn append(inner: &Inner, registry: &File, p: &mut Profile) -> Result<()> {
    let next = p
        .active
        .checked_add(1)
        .filter(|id| *id != NONE)
        .ok_or(Error::LimitExceeded)?;
    let mut tail = checked(inner, *p, p.active)?;
    if !p.indexed || tail.next != NONE {
        return Err(Error::Corrupt("pin generation index append"));
    }
    p.dirty = true;
    store(inner, registry, *p)?;
    create_generation(inner, next)?;
    write(
        inner,
        next,
        Links {
            previous: p.active,
            next: NONE,
        },
        true,
    )?;
    tail.next = next;
    write(inner, p.active, tail, false)?;
    p.active = next;
    p.generations = p.generations.checked_add(1).ok_or(Error::LimitExceeded)?;
    p.dirty = false;
    store(inner, registry, *p)
}

/// Caller holds Registry and this generation's membership EX. The active
/// generation is always retained, and a live/stale member prevents retirement.
pub(super) fn retire(
    inner: &Inner,
    registry: &File,
    id: u64,
    directory: &File,
    exclusion: &Lease,
) -> Result<bool> {
    retire_with_fault(inner, registry, id, directory, exclusion, |_| Ok(()))
}

// Monomorphized no-op in production; tests inject process-loss boundaries after
// each persistent namespace mutation without depending on a racy filesystem.
pub(super) fn retire_with_fault(
    inner: &Inner,
    registry: &File,
    id: u64,
    directory: &File,
    exclusion: &Lease,
    fault: impl Fn(u8) -> Result<()>,
) -> Result<bool> {
    let mut p = profile(inner, registry)?.ok_or(Error::Corrupt("pin registry profile lost"))?;
    if !p.indexed || p.migrating || id == p.active || !is_empty(inner, id)? {
        return Ok(false);
    }
    let path = generation(inner, id);
    let named = opened(&path, false, true)?;
    if identity(&named)? != identity(directory)? {
        return Err(Error::Corrupt("retiring pin generation replaced"));
    }
    let scan = opened(&path.join(SCAN), false, false)?;
    #[cfg(not(windows))]
    let held_scan = &exclusion.file;
    #[cfg(windows)]
    let held_scan = exclusion.file();
    if scan.metadata()?.len() != 0 || identity(&scan)? != identity(held_scan)? {
        return Err(Error::Corrupt("retiring pin generation scan replaced"));
    }
    let links = read(inner, id)?;
    let mut previous = if links.previous == NONE {
        None
    } else {
        Some(read(inner, links.previous)?)
    };
    let mut next = if links.next == NONE {
        None
    } else {
        Some(read(inner, links.next)?)
    };
    if links.next == NONE
        || p.generations <= 1
        || previous.is_some_and(|node| node.next != id)
        || next.is_some_and(|node| node.previous != id)
        || (links.previous == NONE && p.first != id)
    {
        return Err(Error::Corrupt("retiring pin generation links"));
    }
    p.dirty = true;
    store(inner, registry, p)?;
    fault(1)?;
    if let Some(node) = previous.as_mut() {
        node.next = links.next;
        write(inner, links.previous, *node, false)?;
    } else {
        p.first = links.next;
    }
    if let Some(node) = next.as_mut() {
        node.previous = links.previous;
        write(inner, links.next, *node, false)?;
    }
    fault(2)?;
    let link_file = opened(&path.join(LINKS), false, false)?;
    remove_owned(&path.join(LINKS), &link_file)?;
    remove_owned(&path.join(SCAN), held_scan)?;
    fault(3)?;
    // Cooperating writers are serialized by Registry and membership EX; only
    // the checked empty directory can be removed. No recursive unlink is used.
    let current = opened(&path, false, true)?;
    if identity(&current)? != identity(directory)? {
        return Err(Error::Corrupt("retiring pin directory replaced"));
    }
    fs::remove_dir(&path)?;
    fault(4)?;
    p.generations -= 1;
    p.dirty = false;
    store(inner, registry, p)?;
    Ok(true)
}
