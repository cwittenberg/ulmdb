//! Explicit persisted address layout. This is process placement authority, not
//! logical graph/pin authority. Only checked immutable Mapping owners use it.
use crate::{
    Error, Result, SegmentEntry,
    fixed_address::{ByteLease, Reservation},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};
const HEADER: usize = 128;
const WIDE_HEADER: usize = 160;
const MAGIC_V3: &[u8; 8] = b"ULMDBFM3";
const ROW: usize = 80;
const MAX_SLOTS: usize = 1_048_576;
const MAGIC: &[u8; 8] = b"ULMDBFM1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedMapInfo {
    pub address: usize,
    pub capacity: usize,
    pub assigned_bytes: usize,
    pub segments: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Slot {
    offset: usize,
    span: usize,
}
#[derive(Clone)]
struct State {
    reusable: bool,
    version: u16,
    lock_identity: Option<reuse::LeaseIdentity>,
    next: usize,
    slots: BTreeMap<[u8; 64], Slot>,
    // Runtime optimistic-publication token for the immutable layout image.
    source: Option<host::FileToken>,
}
pub struct FixedMap {
    directory: PathBuf,
    dir: File,
    creator: u32,
    uuid: [u8; 16],
    epoch: u64,
    region: Arc<Reservation>,
    state: Mutex<State>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    decoded: Mutex<BTreeMap<[u8; 64], Weak<crate::transform::FixedDecodedBacking>>>,
}
static OWNERS: OnceLock<Mutex<BTreeMap<(u64, u128), Weak<FixedMap>>>> = OnceLock::new();
static CACHE_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
fn owners() -> Result<&'static Mutex<BTreeMap<(u64, u128), Weak<FixedMap>>>> {
    use std::sync::atomic::Ordering;
    let pid = std::process::id();
    let creator = match CACHE_PID.compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => pid,
        Err(old) => old,
    };
    if creator != pid {
        return Err(Error::InvalidInput("inherited fixed cache requires exec"));
    }
    Ok(OWNERS.get_or_init(Default::default))
}
fn key(e: &SegmentEntry) -> [u8; 64] {
    let mut b = [0; 64];
    b[..8].copy_from_slice(&e.id.to_le_bytes());
    b[8..24].copy_from_slice(&e.cohort.to_le_bytes());
    b[24..32].copy_from_slice(&e.length.to_le_bytes());
    b[32..].copy_from_slice(&e.hash);
    b
}
fn page() -> Result<usize> {
    ulmdb_platform::page_size().map_err(Into::into)
}

fn span(n: u64) -> Result<usize> {
    let p = page()?;
    usize::try_from(n)
        .ok()
        .and_then(|n| n.checked_add(p - 1))
        .map(|n| n / p * p)
        .ok_or(Error::LimitExceeded)
}
impl FixedMap {
    pub fn create(
        directory: &Path,
        uuid: [u8; 16],
        epoch: u64,
        capacity: usize,
        requested: Option<usize>,
    ) -> Result<Arc<Self>> {
        Self::create_profile(directory, uuid, epoch, capacity, requested, false)
    }
    /// Fresh v2 namespace: explicit reuse negotiation, never an in-place v1
    /// upgrade while unrecorded old mapping participants may still be alive.
    pub fn create_reusable(
        directory: &Path,
        uuid: [u8; 16],
        epoch: u64,
        capacity: usize,
        requested: Option<usize>,
    ) -> Result<Arc<Self>> {
        Self::create_profile(directory, uuid, epoch, capacity, requested, true)
    }
    fn create_profile(
        directory: &Path,
        uuid: [u8; 16],
        epoch: u64,
        capacity: usize,
        requested: Option<usize>,
        reusable: bool,
    ) -> Result<Arc<Self>> {
        owners()?;
        if uuid == [0; 16] || epoch == 0 {
            return Err(Error::InvalidInput("fixed map empty realm"));
        }
        let region = Reservation::reserve(requested, capacity)?;
        let directory = directory.canonicalize()?;
        let dir = host::directory(&directory)?;
        let value = Arc::new(Self {
            directory: directory.clone(),
            dir,
            creator: std::process::id(),
            uuid,
            epoch,
            region,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            decoded: Mutex::new(BTreeMap::new()),
            state: Mutex::new(State {
                reusable,
                version: if reusable { reuse::fresh_version() } else { 1 },
                lock_identity: if reusable {
                    Some(reuse::create_identity(&directory, reuse::fresh_version())?)
                } else {
                    None
                },
                next: 0,
                slots: BTreeMap::new(),
                source: None,
            }),
        });
        let state = value.state.lock().map_err(|_| Error::Busy)?;
        let bytes = value.encode(&state)?;
        let target = directory.join("FIXEDMAP");
        let mut file = crate::access::options_for_entry(&target)?
            .write(true)
            .create_new(true)
            .open(&target)?;
        let created = host::token(&file)?;
        let result: Result<()> = (|| {
            file.write_all(&bytes)?;
            ulmdb_platform::sync_file(&file)?;
            ulmdb_platform::sync_directory(&directory)?;
            Ok(())
        })();
        if let Err(error) = result {
            // A failed creation owns only this newly-created inode, never a
            // replacement installed at the same name by an external actor.
            if host::named(&target).is_ok_and(|named| {
                named.regular
                    && !named.reparse
                    && (named.volume, named.file_id, named.length) == created
            }) {
                let _ = fs::remove_file(&target);
            }
            return Err(error);
        }
        drop(state);
        value.register_owner()?;
        Ok(value)
    }
    pub fn open(directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<Arc<Self>> {
        let directory = directory.canonicalize()?;
        let dir = host::directory(&directory)?;
        let identity = host::identity(&dir)?;
        if let Some(owner) = owners()?
            .lock()
            .map_err(|_| Error::Busy)?
            .get(&identity)
            .and_then(Weak::upgrade)
        {
            owner.check()?;
            if owner.uuid != uuid || owner.epoch != epoch {
                return Err(Error::Corrupt("fixed map realm"));
            }
            {
                let mut state = owner.state.lock().map_err(|_| Error::Busy)?;
                owner.refresh(&mut state)?;
            }
            return Ok(owner);
        }
        let (address, capacity, state) = Self::decode(&directory, uuid, epoch)?;
        let region = Reservation::reserve(Some(address), capacity)?;
        let owner = Arc::new(Self {
            directory,
            dir,
            creator: std::process::id(),
            uuid,
            epoch,
            region,
            state: Mutex::new(state),
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            decoded: Mutex::new(BTreeMap::new()),
        });
        owner.register_owner()?;
        Ok(owner)
    }
    fn register_owner(self: &Arc<Self>) -> Result<()> {
        self.check()?;
        let identity = host::identity(&self.dir)?;
        owners()?
            .lock()
            .map_err(|_| Error::Busy)?
            .insert(identity, Arc::downgrade(self));
        Ok(())
    }
    /// Checked protocol capability, never inferred from a flag or cached magic.
    pub fn supports_reuse(&self) -> Result<bool> {
        self.is_reusable()
    }
    pub fn reuse_pressure(&self) -> Result<Option<FixedSlotPressure>> {
        self.check()?;
        let mut state = self.state.lock().map_err(|_| Error::Busy)?;
        self.refresh(&mut state)?;
        reuse::pressure(&state, self.region.len())
    }
    pub fn is_reusable(&self) -> Result<bool> {
        self.check()?;
        let mut state = self.state.lock().map_err(|_| Error::Busy)?;
        self.refresh(&mut state)?;
        Ok(state.reusable)
    }
    pub fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited fixed map"));
        }
        self.region.check_creator()?;
        host::check_directory(&self.directory, &self.dir)?;
        Ok(())
    }
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn open_namespace(directory: &Path) -> Result<Arc<Self>> {
        let mut h = [0; HEADER];
        let p = directory.join("FIXEDMAP");
        let mut f = host::readonly(&p)?;
        if !f.metadata()?.is_file() {
            return Err(Error::Corrupt("fixed metadata type"));
        }
        f.read_exact(&mut h)?;
        if &h[..8] != MAGIC && &h[..8] != reuse::MAGIC_V2 && &h[..8] != MAGIC_V3 {
            return Err(Error::Corrupt("fixed metadata magic"));
        }
        Self::open(
            directory,
            h[16..32].try_into().unwrap(),
            u64::from_le_bytes(h[32..40].try_into().unwrap()),
        )
    }
    pub fn check_for(&self, directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<()> {
        self.check()?;
        if directory.canonicalize()? != self.directory || uuid != self.uuid || epoch != self.epoch {
            return Err(Error::Corrupt("fixed layout owner mismatch"));
        }
        Ok(())
    }
    pub fn info(&self) -> Result<FixedMapInfo> {
        self.check()?;
        let mut state = self.state.lock().map_err(|_| Error::Busy)?;
        self.refresh(&mut state)?;
        Ok(FixedMapInfo {
            address: self.region.address(),
            capacity: self.region.len(),
            assigned_bytes: state.next,
            segments: state.slots.len(),
        })
    }
    fn encode(&self, state: &State) -> Result<Vec<u8>> {
        Self::encode_parts(
            self.uuid,
            self.epoch,
            self.region.address(),
            self.region.len(),
            state,
        )
    }
    fn encode_parts(
        uuid: [u8; 16],
        epoch: u64,
        address: usize,
        capacity: usize,
        state: &State,
    ) -> Result<Vec<u8>> {
        if state.slots.len() > MAX_SLOTS {
            return Err(Error::LimitExceeded);
        }
        let header = if state.version == 3 {
            WIDE_HEADER
        } else {
            HEADER
        };
        if !matches!(state.version, 1..=3) || state.reusable != (state.version != 1) {
            return Err(Error::Corrupt("fixed layout version"));
        }
        let mut b = vec![0; header + ROW * state.slots.len()];
        b[..8].copy_from_slice(match state.version {
            1 => MAGIC,
            2 => reuse::MAGIC_V2,
            3 => MAGIC_V3,
            _ => unreachable!(),
        });
        b[8..10].copy_from_slice(&state.version.to_le_bytes());
        b[10..12].copy_from_slice(&(header as u16).to_le_bytes());
        if let Some(identity) = state.lock_identity {
            b[80..88].copy_from_slice(&identity.volume.to_le_bytes());
            if state.version == 2 {
                if identity.kind != reuse::LeaseKind::NativeLock {
                    return Err(Error::Corrupt("v2 fixed lease kind"));
                }
                let id = u64::try_from(identity.file_id)
                    .map_err(|_| Error::Corrupt("v2 fixed lease ID width"))?;
                b[88..96].copy_from_slice(&id.to_le_bytes());
            } else if state.version == 3 {
                if identity.kind != reuse::LeaseKind::Dedicated {
                    return Err(Error::Corrupt("v3 fixed lease kind"));
                }
                b[88..104].copy_from_slice(&identity.file_id.to_le_bytes());
                b[104..112].copy_from_slice(&1u64.to_le_bytes());
            } else {
                return Err(Error::Corrupt("append-only fixed lease"));
            }
        } else if state.reusable {
            return Err(Error::Corrupt("missing fixed lease identity"));
        }
        b[16..32].copy_from_slice(&uuid);
        b[32..40].copy_from_slice(&epoch.to_le_bytes());
        b[40..48].copy_from_slice(&(address as u64).to_le_bytes());
        b[48..56].copy_from_slice(&(capacity as u64).to_le_bytes());
        b[56..64].copy_from_slice(&(state.next as u64).to_le_bytes());
        b[64..72].copy_from_slice(&(state.slots.len() as u64).to_le_bytes());
        b[72..80].copy_from_slice(&(page()? as u64).to_le_bytes());
        for ((key, slot), row) in state.slots.iter().zip(b[header..].chunks_exact_mut(ROW)) {
            row[..64].copy_from_slice(key);
            row[64..72].copy_from_slice(&(slot.offset as u64).to_le_bytes());
            row[72..80].copy_from_slice(&(slot.span as u64).to_le_bytes());
        }
        let mut h = Sha256::new();
        h.update(&b[..header - 32]);
        h.update(&b[header..]);
        b[header - 32..header].copy_from_slice(&h.finalize());
        Ok(b)
    }
    pub fn declared_info(directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<FixedMapInfo> {
        let (address, capacity, state) = Self::decode(directory, uuid, epoch)?;
        Ok(FixedMapInfo {
            address,
            capacity,
            assigned_bytes: state.next,
            segments: state.slots.len(),
        })
    }
    pub fn validate_layout(
        directory: &Path,
        manifests: &[&crate::Manifest],
    ) -> Result<Option<FixedMapInfo>> {
        let Some(first) = manifests.first() else {
            return Err(Error::InvalidInput("empty fixed layout graph set"));
        };
        match fs::symlink_metadata(directory.join("FIXEDMAP")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        let (address, capacity, state) = Self::decode(directory, first.uuid, first.epoch)?;
        for manifest in manifests {
            if manifest.uuid != first.uuid || manifest.epoch != first.epoch {
                return Err(Error::Corrupt("fixed copied realm"));
            }
            for entry in &manifest.segments {
                if !state.slots.contains_key(&key(entry)) {
                    return Err(Error::Corrupt("fixed copied graph missing slot"));
                }
            }
        }
        Ok(Some(FixedMapInfo {
            address,
            capacity,
            assigned_bytes: state.next,
            segments: state.slots.len(),
        }))
    }
    /// Copy checked placement metadata only after the Native copy owns/audits
    /// the destination. Compact targets have genuine new roots/realm and slots;
    /// raw targets retain exact existing physical-key offsets. No target map is
    /// installed here, so a live source's address reservation is never clobbered.
    pub fn export_layout(
        source: &Path,
        target: &Path,
        source_uuid: [u8; 16],
        source_epoch: u64,
        selected: &crate::Manifest,
        compact: bool,
    ) -> Result<()> {
        match fs::symlink_metadata(source.join("FIXEDMAP")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        let (address, capacity, mut state) = Self::decode(source, source_uuid, source_epoch)?;
        if compact {
            state = State {
                reusable: state.reusable,
                version: state.version,
                lock_identity: state.lock_identity,
                next: 0,
                slots: BTreeMap::new(),
                source: None,
            };
            for entry in &selected.segments {
                let size = span(entry.length)?;
                let next = state
                    .next
                    .checked_add(size)
                    .filter(|n| *n <= capacity)
                    .ok_or(Error::LimitExceeded)?;
                state.slots.insert(
                    key(entry),
                    Slot {
                        offset: state.next,
                        span: size,
                    },
                );
                state.next = next;
            }
        } else if selected.uuid != source_uuid
            || selected.epoch != source_epoch
            || selected
                .segments
                .iter()
                .any(|entry| !state.slots.contains_key(&key(entry)))
        {
            return Err(Error::Corrupt("fixed raw copy layout mismatch"));
        }
        if state.reusable {
            state.version = reuse::target_version(state.version);
            state.lock_identity = Some(reuse::create_identity(target, state.version)?);
        }
        let bytes = Self::encode_parts(selected.uuid, selected.epoch, address, capacity, &state)?;
        let path = target.join("FIXEDMAP");
        let mut out = crate::access::options_for_entry(&path)?
            .write(true)
            .create_new(true)
            .open(&path)?;
        out.write_all(&bytes)?;
        ulmdb_platform::sync_file(&out)?;
        ulmdb_platform::sync_directory(target)?;
        Ok(())
    }
    fn decode(directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<(usize, usize, State)> {
        Self::decode_image(directory, uuid, epoch, true)
    }
    fn decode_image(
        directory: &Path,
        uuid: [u8; 16],
        epoch: u64,
        verify_lease: bool,
    ) -> Result<(usize, usize, State)> {
        let p = directory.join("FIXEDMAP");
        let mut file = host::readonly(&p)?;
        let metadata = host::token(&file)?;
        if metadata.2 < HEADER as u64 || metadata.2 > (WIDE_HEADER + ROW * MAX_SLOTS) as u64 {
            return Err(Error::Corrupt("fixed map metadata extent"));
        }
        let mut prefix = [0u8; HEADER];
        file.read_exact(&mut prefix)?;
        let version = u16::from_le_bytes(prefix[8..10].try_into().unwrap());
        let header = match (&prefix[..8], version) {
            (m, 1) if m == MAGIC => HEADER,
            (m, 2) if m == reuse::MAGIC_V2 => HEADER,
            (m, 3) if m == MAGIC_V3 => WIDE_HEADER,
            _ => return Err(Error::Corrupt("fixed map metadata version")),
        };
        let count = u64::from_le_bytes(prefix[64..72].try_into().unwrap());
        let capacity = u64::from_le_bytes(prefix[48..56].try_into().unwrap());
        if count > MAX_SLOTS as u64
            || count > capacity / page()? as u64
            || metadata.2 != header as u64 + ROW as u64 * count
        {
            return Err(Error::Corrupt("fixed map count budget"));
        }
        let mut b = Vec::with_capacity(metadata.2 as usize);
        b.extend_from_slice(&prefix);
        file.take((WIDE_HEADER + ROW * MAX_SLOTS + 1) as u64)
            .read_to_end(&mut b)?;
        let n = |at: usize| -> Result<usize> {
            usize::try_from(u64::from_le_bytes(b[at..at + 8].try_into().unwrap()))
                .map_err(|_| Error::LimitExceeded)
        };
        let reusable = version != 1;
        if b.len() < header
            || b[10..12] != (header as u16).to_le_bytes()
            || b[12..16] != [0; 4]
            || b[16..32] != uuid
            || b[32..40] != epoch.to_le_bytes()
            || n(72)? != page()?
            || (version == 1 && b[80..96] != [0; 16])
            || (version == 3 && (b[104..112] != 1u64.to_le_bytes() || b[112..128] != [0; 16]))
        {
            return Err(Error::Corrupt("fixed map metadata header"));
        }
        let lock_identity = if reusable {
            let identity = reuse::LeaseIdentity {
                volume: u64::from_le_bytes(b[80..88].try_into().unwrap()),
                file_id: if version == 2 {
                    u64::from_le_bytes(b[88..96].try_into().unwrap()) as u128
                } else {
                    u128::from_le_bytes(b[88..104].try_into().unwrap())
                },
                kind: if version == 2 {
                    reuse::LeaseKind::NativeLock
                } else {
                    reuse::LeaseKind::Dedicated
                },
            };
            if verify_lease {
                reuse::check_identity(directory, Some(identity))?;
            }
            Some(identity)
        } else {
            None
        };
        let address = n(40)?;
        let capacity = n(48)?;
        let next = n(56)?;
        let count = n(64)?;
        if count > MAX_SLOTS
            || b.len() != header + ROW * count
            || address == 0
            || capacity == 0
            || address.checked_add(capacity).is_none()
            || next > capacity
            || !next.is_multiple_of(page()?)
            || !address.is_multiple_of(page()?)
            || !capacity.is_multiple_of(page()?)
        {
            return Err(Error::Corrupt("fixed map metadata bounds"));
        }
        let mut h = Sha256::new();
        h.update(&b[..header - 32]);
        h.update(&b[header..]);
        if h.finalize().as_slice() != &b[header - 32..header] {
            return Err(Error::Corrupt("fixed map metadata checksum"));
        }
        let mut slots = BTreeMap::new();
        let mut ranges = Vec::new();
        let mut last = None;
        for row in b[header..].chunks_exact(ROW) {
            let k: [u8; 64] = row[..64].try_into().unwrap();
            let offset = usize::try_from(u64::from_le_bytes(row[64..72].try_into().unwrap()))
                .map_err(|_| Error::LimitExceeded)?;
            let size = usize::try_from(u64::from_le_bytes(row[72..80].try_into().unwrap()))
                .map_err(|_| Error::LimitExceeded)?;
            let length = u64::from_le_bytes(k[24..32].try_into().unwrap());
            if last.is_some_and(|old| old >= k)
                || length < 4096
                || u64::from_le_bytes(k[..8].try_into().unwrap()) == 0
                || u64::from_le_bytes(k[..8].try_into().unwrap()) == u64::MAX
                || span(length)? != size
                || !offset.is_multiple_of(page()?)
                || offset.checked_add(size).is_none_or(|end| end > next)
            {
                return Err(Error::Corrupt("fixed map slot bounds"));
            }
            last = Some(k);
            ranges.push((offset, offset + size));
            slots.insert(k, Slot { offset, span: size });
        }
        ranges.sort();
        if (!reusable && ranges.first().map_or(next != 0, |r| r.0 != 0))
            || ranges.last().map_or(next != 0, |r| r.1 != next)
            || ranges.windows(2).any(|w| {
                if reusable {
                    w[0].1 > w[1].0
                } else {
                    w[0].1 != w[1].0
                }
            })
        {
            return Err(Error::Corrupt("overlapping fixed map slots"));
        }
        Ok((
            address,
            capacity,
            State {
                reusable,
                version,
                lock_identity,
                next,
                slots,
                source: Some(metadata),
            },
        ))
    }
    fn refresh(&self, state: &mut State) -> Result<()> {
        let (address, capacity, new) = Self::decode(&self.directory, self.uuid, self.epoch)?;
        if address != self.region.address()
            || capacity != self.region.len()
            || new.reusable != state.reusable
            || new.version != state.version
            || new.lock_identity != state.lock_identity
            || (!state.reusable && new.next < state.next)
            || state.slots.iter().any(|(key, slot)| {
                new.slots.get(key) != Some(slot)
                    && (!state.reusable
                        || self
                            .region
                            .range_is_live(slot.offset, slot.span)
                            .unwrap_or(true))
            })
        {
            return Err(Error::Corrupt("fixed map layout changed"));
        }
        *state = new;
        Ok(())
    }
    fn store(&self, state: &State, options: &publish::Options) -> Result<()> {
        self.check()?;
        let bytes = self.encode(state)?;
        publish::replace(
            &self.directory,
            self.uuid,
            self.epoch,
            &bytes,
            state
                .source
                .ok_or(Error::Corrupt("fixed layout missing base identity"))?,
            options,
            || {
                self.check()?;
                reuse::check_identity(&self.directory, state.lock_identity)
            },
        )
    }

    /// Reserve writable staging bytes at an unused address. The native
    /// writer/caller-managed writer excludes another layout publisher. No draft
    /// physical identity enters the persisted layout until its bytes are sealed.
    #[allow(unsafe_code)]
    pub(crate) unsafe fn map_writer(
        &self,
        file: &File,
        capacity: usize,
    ) -> Result<crate::fixed_address::WritableByteLease> {
        self.check()?;
        let mut state = self.state.lock().map_err(|_| Error::Busy)?;
        self.refresh(&mut state)?;
        let (offset, capacity) = reuse::writer_extent(&state, self.region.len(), capacity)?;
        let lock = if state.reusable {
            Some(reuse::lease(
                &self.directory,
                state.lock_identity.unwrap(),
                offset,
                capacity,
                true,
            )?)
        } else {
            None
        };
        let lease = unsafe { self.region.map_file_mut(file, offset, capacity)? };
        if let Some(lock) = lock {
            lease.retain_layout_lock(lock)?;
        }
        Ok(lease)
    }
    pub(crate) fn register_writer_entry(
        &self,
        entry: &SegmentEntry,
        offset: usize,
        policy: crate::Durability,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<()> {
        self.check()?;
        let mut state = self.state.lock().map_err(|_| Error::Busy)?;
        self.refresh(&mut state)?;
        if state.reusable && external.is_some() {
            return Err(Error::InvalidInput(
                "reusable fixed layout requires coordination",
            ));
        }
        let k = key(entry);
        if state.slots.contains_key(&k) {
            return Ok(());
        }
        let size = span(entry.length)?;
        if ((!state.reusable && state.next != offset)
            || (state.reusable && !reuse::range_free(&state, offset, size, self.region.len())))
            || state.slots.len() >= MAX_SLOTS
        {
            return Err(Error::Corrupt("fixed writer placement changed"));
        }
        let next = offset
            .checked_add(size)
            .filter(|n| *n <= self.region.len())
            .ok_or(Error::LimitExceeded)?;
        let mut candidate = state.clone();
        drop(state);
        candidate.slots.insert(k, Slot { offset, span: size });
        candidate.next = candidate.next.max(next);
        // Never hold the info/cache mutex while waiting for History. A checksum
        // invoked by a concurrent force-sync may legitimately inspect info.
        self.store(&candidate, &publish::Options { policy, external })?;
        let mut state = self.state.lock().map_err(|_| Error::Busy)?;
        self.refresh(&mut state)?;
        Ok(())
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) fn decoded_backing(
        &self,
        source: &File,
        entry: &SegmentEntry,
        context: &Arc<crate::transform::TransformContext>,
    ) -> Result<(
        Arc<crate::transform::FixedDecodedBacking>,
        Option<crate::transform::DecodedLease>,
    )> {
        self.check()?;
        let k = key(entry);
        let cached = self
            .decoded
            .lock()
            .map_err(|_| Error::Busy)?
            .get(&k)
            .and_then(Weak::upgrade);
        // Callbacks and source IO run outside both placement/cache mutexes.
        let (backing, lease) = context.fixed_backing(
            source,
            crate::transform::ObjectKind::Segment,
            entry.id as u128,
            entry.length,
            entry.hash,
            cached,
        )?;
        let mut cache = self.decoded.lock().map_err(|_| Error::Busy)?;
        if let Some(winner) = cache.get(&k).and_then(Weak::upgrade) {
            if !Arc::ptr_eq(&winner, &backing) {
                drop(backing);
                drop(lease);
                let lease = context.reserve_cache_mapping(entry.length)?;
                return Ok((winner, Some(lease)));
            }
        }
        cache.retain(|_, value| value.strong_count() != 0);
        if cache.len() >= MAX_SLOTS && !cache.contains_key(&k) {
            return Err(Error::LimitExceeded);
        }
        cache.insert(k, Arc::downgrade(&backing));
        Ok((backing, lease))
    }
    /// Called only by an immutable Mapping that already checked this exact entry.
    pub(crate) fn map_entry(
        &self,
        entry: &SegmentEntry,
        file: Arc<File>,
        register: bool,
        advice: crate::ReadAhead,
        policy: crate::Durability,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<ByteLease> {
        self.check()?;
        if external.is_some() && self.is_reusable()? {
            return Err(Error::InvalidInput(
                "reusable fixed layout requires coordination",
            ));
        }
        let k = key(entry);
        let slot = {
            let mut state = self.state.lock().map_err(|_| Error::Busy)?;
            self.refresh(&mut state)?;
            if let Some(slot) = state.slots.get(&k).copied() {
                slot
            } else {
                if !register {
                    return Err(Error::Corrupt("fixed map missing committed slot"));
                }
                if state.slots.len() >= MAX_SLOTS {
                    return Err(Error::LimitExceeded);
                }
                let size = span(entry.length)?;
                let slot = reuse::allocation(&state, self.region.len(), size)?;
                let next = state.next.max(slot.offset + slot.span);
                let mut candidate = state.clone();
                drop(state);
                candidate.slots.insert(k, slot);
                candidate.next = next;
                self.store(
                    &candidate,
                    &publish::Options {
                        policy,
                        external: external.clone(),
                    },
                )?;
                let mut state = self.state.lock().map_err(|_| Error::Busy)?;
                self.refresh(&mut state)?;
                if state.slots.get(&k) != Some(&slot) {
                    return Err(Error::Corrupt("fixed published slot changed"));
                }
                slot
            }
        };
        self.check()?;
        let layout_lock = {
            let state = self.state.lock().map_err(|_| Error::Busy)?;
            if state.reusable {
                if external.is_some() {
                    return Err(Error::InvalidInput(
                        "reusable fixed mapping requires coordination",
                    ));
                }
                Some(reuse::lease(
                    &self.directory,
                    state.lock_identity.unwrap(),
                    slot.offset,
                    slot.span,
                    false,
                )?)
            } else {
                None
            }
        };
        if layout_lock.is_some() {
            let mut state = self.state.lock().map_err(|_| Error::Busy)?;
            self.refresh(&mut state)?;
            if state.slots.get(&k) != Some(&slot) {
                return Err(Error::Busy);
            }
        }
        // SAFETY: only Mapping invokes this after full immutable header/hash/extent
        // validation. Engine seals/protects these files and physical authority keeps
        // them from mutation/truncation while the returned owning mapping is alive.
        #[allow(unsafe_code)]
        let lease = unsafe {
            self.region.map_file_with_read_ahead(
                file,
                0,
                slot.offset,
                usize::try_from(entry.length).map_err(|_| Error::LimitExceeded)?,
                advice,
            )?
        };
        if let Some(lock) = layout_lock {
            lease.retain_layout_lock(lock)?;
        }
        Ok(lease)
    }
    /// Same checked placement/lease authority as map_entry, using an opaque
    /// authenticated immutable object rather than a fabricated file/number.
    #[cfg(any(target_os = "macos", windows))]
    pub(crate) fn map_decoded_entry(
        &self,
        entry: &SegmentEntry,
        object: Arc<crate::transform::ReadOnlyDecodedObject>,
        register: bool,
        advice: crate::ReadAhead,
        policy: crate::Durability,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<ByteLease> {
        self.check()?;
        object.check_creator()?;
        if u64::try_from(object.len()).map_err(|_| Error::LimitExceeded)? != entry.length {
            return Err(Error::Corrupt("fixed decoded object extent"));
        }
        if external.is_some() && self.is_reusable()? {
            return Err(Error::InvalidInput(
                "reusable fixed layout requires coordination",
            ));
        }
        let k = key(entry);
        let slot = {
            let mut state = self.state.lock().map_err(|_| Error::Busy)?;
            self.refresh(&mut state)?;
            if let Some(slot) = state.slots.get(&k).copied() {
                slot
            } else {
                if !register {
                    return Err(Error::Corrupt("fixed map missing committed slot"));
                }
                if state.slots.len() >= MAX_SLOTS {
                    return Err(Error::LimitExceeded);
                }
                let size = span(entry.length)?;
                let slot = reuse::allocation(&state, self.region.len(), size)?;
                let next = state.next.max(slot.offset + slot.span);
                let mut candidate = state.clone();
                drop(state);
                candidate.slots.insert(k, slot);
                candidate.next = next;
                self.store(
                    &candidate,
                    &publish::Options {
                        policy,
                        external: external.clone(),
                    },
                )?;
                let mut state = self.state.lock().map_err(|_| Error::Busy)?;
                self.refresh(&mut state)?;
                if state.slots.get(&k) != Some(&slot) {
                    return Err(Error::Corrupt("fixed published slot changed"));
                }
                slot
            }
        };
        self.check()?;
        let layout_lock = {
            let state = self.state.lock().map_err(|_| Error::Busy)?;
            if state.reusable {
                if external.is_some() {
                    return Err(Error::InvalidInput(
                        "reusable fixed mapping requires coordination",
                    ));
                }
                Some(reuse::lease(
                    &self.directory,
                    state.lock_identity.unwrap(),
                    slot.offset,
                    slot.span,
                    false,
                )?)
            } else {
                None
            }
        };
        if layout_lock.is_some() {
            let mut state = self.state.lock().map_err(|_| Error::Busy)?;
            self.refresh(&mut state)?;
            if state.slots.get(&k) != Some(&slot) {
                return Err(Error::Busy);
            }
        }
        let lease = self.region.map_decoded(object, slot.offset, advice)?;
        if let Some(lock) = layout_lock {
            lease.retain_layout_lock(lock)?;
        }
        Ok(lease)
    }
}

/// Numeric layout-update authority without a process address reservation. A
/// non-FIXEDMAP writer must keep the persistent layout usable by later fixed
/// readers even when another mapping already occupies the declared window.
/// Native writers/caller-managed synchronization serialize these updates.
pub struct FixedMapDeclaration {
    directory: PathBuf,
    dir: File,
    creator: u32,
    uuid: [u8; 16],
    epoch: u64,
}
impl FixedMapDeclaration {
    pub fn open(directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<Arc<Self>> {
        FixedMap::decode(directory, uuid, epoch)?;
        let directory = directory.canonicalize()?;
        let dir = host::directory(&directory)?;
        Ok(Arc::new(Self {
            directory,
            dir,
            creator: std::process::id(),
            uuid,
            epoch,
        }))
    }
    pub fn check_for(&self, directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited fixed declaration"));
        }
        host::check_directory(&self.directory, &self.dir)?;
        if directory.canonicalize()? != self.directory || uuid != self.uuid || epoch != self.epoch {
            return Err(Error::Corrupt("fixed declaration namespace"));
        }
        Ok(())
    }
    /// Checked protocol capability, never inferred from a flag or cached magic.
    pub fn supports_reuse(&self) -> Result<bool> {
        self.is_reusable()
    }
    pub fn reuse_pressure(&self) -> Result<Option<FixedSlotPressure>> {
        self.check_for(&self.directory, self.uuid, self.epoch)?;
        let (_, capacity, state) = FixedMap::decode(&self.directory, self.uuid, self.epoch)?;
        reuse::pressure(&state, capacity)
    }
    pub fn is_reusable(&self) -> Result<bool> {
        self.check_for(&self.directory, self.uuid, self.epoch)?;
        Ok(FixedMap::decode(&self.directory, self.uuid, self.epoch)?
            .2
            .reusable)
    }
    pub fn register_manifest(&self, manifest: &crate::Manifest) -> Result<()> {
        self.check_for(&self.directory, manifest.uuid, manifest.epoch)?;
        self.register_manifest_with_policy(manifest, crate::Durability::Full, None)
    }
    pub fn register_manifest_with_policy(
        &self,
        manifest: &crate::Manifest,
        policy: crate::Durability,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<()> {
        self.check_for(&self.directory, manifest.uuid, manifest.epoch)?;
        self.register_entries(manifest.segments.iter(), policy, external)
    }
    pub(crate) fn register_entry(
        &self,
        entry: &SegmentEntry,
        policy: crate::Durability,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<()> {
        self.register_entries(std::iter::once(entry), policy, external)
    }
    fn register_entries<'a>(
        &self,
        entries: impl IntoIterator<Item = &'a SegmentEntry>,
        policy: crate::Durability,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<()> {
        self.check_for(&self.directory, self.uuid, self.epoch)?;
        let (address, capacity, mut state) =
            FixedMap::decode(&self.directory, self.uuid, self.epoch)?;
        if state.reusable && external.is_some() {
            return Err(Error::InvalidInput(
                "reusable fixed layout requires coordination",
            ));
        }
        let mut changed = false;
        for entry in entries {
            let k = key(entry);
            if state.slots.contains_key(&k) {
                continue;
            }
            if state.slots.len() >= MAX_SLOTS {
                return Err(Error::LimitExceeded);
            }
            let size = span(entry.length)?;
            let slot = reuse::allocation(&state, capacity, size)?;
            state.next = state.next.max(slot.offset + slot.span);
            state.slots.insert(k, slot);
            changed = true;
        }
        if changed {
            let bytes = FixedMap::encode_parts(self.uuid, self.epoch, address, capacity, &state)?;
            publish::replace(
                &self.directory,
                self.uuid,
                self.epoch,
                &bytes,
                state
                    .source
                    .ok_or(Error::Corrupt("fixed layout missing base identity"))?,
                &publish::Options { policy, external },
                || {
                    self.check_for(&self.directory, self.uuid, self.epoch)?;
                    reuse::check_identity(&self.directory, state.lock_identity)
                },
            )?;
        }
        Ok(())
    }
}

mod host;
mod publish;

mod reuse;
pub use reuse::{FixedSlotPressure, FixedSlotReclaimLimits, FixedSlotReclaimReport, FixedSlotRoot};

#[cfg(test)]
mod m3_tests;
