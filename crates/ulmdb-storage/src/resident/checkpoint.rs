//! Streamed Resident checkpoint. Slot identities and generation floors are never
//! translated. This format is separate from Hybrid's append-only chunk archive.
use super::{
    Pool, VersionKey, View,
    wal_codec::{Boot, DATA_START, MAX_GENERATION, PAGE, SlotState},
};
use crate::{Error, Reference, Result, hybrid::redo::WalReceipt};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};
const HEADER: usize = 256;
const LIVE_BYTES: u64 = 1024 + PAGE as u64;
const MAGIC: &[u8; 8] = b"ULMDBRC1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Seed {
    pub generation: u128,
    pub digest: [u8; 32],
    pub lsn: u64,
    pub sequence: u64,
    pub root: Reference,
    pub next_database: u64,
}
impl Seed {
    pub fn receipt(self) -> WalReceipt {
        WalReceipt {
            lsn: self.lsn,
            sequence: self.sequence,
            offset: DATA_START,
            length: 0,
            digest: self.digest,
        }
    }
    pub fn validate(self, boot: Boot) -> Result<()> {
        boot.slots()?;
        if self.generation == 0
            || self.digest == [0; 32]
            || self.lsn == 0
            || self.sequence > self.lsn
            || self.next_database == 0
        {
            return Err(Error::Corrupt("resident checkpoint identity"));
        }
        super::wal_codec::catalog_key(boot, self.root)?;
        boot.commit
            .checked_add(self.sequence as u128)
            .ok_or(Error::LimitExceeded)?;
        Ok(())
    }
    pub fn encode(self, bytes: &mut [u8]) {
        bytes[..16].copy_from_slice(&self.generation.to_le_bytes());
        bytes[16..48].copy_from_slice(&self.digest);
        bytes[48..56].copy_from_slice(&self.lsn.to_le_bytes());
        bytes[56..64].copy_from_slice(&self.sequence.to_le_bytes());
        bytes[64..72].copy_from_slice(&self.root.segment.to_le_bytes());
        bytes[72..80].copy_from_slice(&self.root.offset.to_le_bytes());
        bytes[80..88].copy_from_slice(&self.next_database.to_le_bytes());
    }
    pub fn decode(bytes: &[u8]) -> Self {
        Self {
            generation: u128::from_le_bytes(bytes[..16].try_into().unwrap()),
            digest: bytes[16..48].try_into().unwrap(),
            lsn: u64::from_le_bytes(bytes[48..56].try_into().unwrap()),
            sequence: u64::from_le_bytes(bytes[56..64].try_into().unwrap()),
            root: Reference {
                segment: u64::from_le_bytes(bytes[64..72].try_into().unwrap()),
                offset: u64::from_le_bytes(bytes[72..80].try_into().unwrap()),
            },
            next_database: u64::from_le_bytes(bytes[80..88].try_into().unwrap()),
        }
    }
    pub fn path(self, parent: &Path) -> PathBuf {
        parent.join(format!(".resident-checkpoint-{:032x}", self.generation))
    }
}
fn header(boot: Boot, mut seed: Seed) -> [u8; HEADER] {
    let mut h = [0; HEADER];
    h[..8].copy_from_slice(MAGIC);
    h[8..24].copy_from_slice(&boot.uuid);
    h[24..32].copy_from_slice(&boot.epoch.to_le_bytes());
    h[32..48].copy_from_slice(&boot.commit.to_le_bytes());
    h[48..52].copy_from_slice(&boot.kv_pages.to_le_bytes());
    h[52..56].copy_from_slice(&boot.tiny_pages.to_le_bytes());
    seed.digest = [0; 32];
    seed.encode(&mut h[64..152]);
    h
}
fn hash() -> Sha256 {
    let mut h = Sha256::new();
    h.update(b"ULMDB/resident/checkpoint/v1\0");
    h
}
fn tag(state: SlotState) -> [u8; 16] {
    let mut t = [0; 16];
    let (kind, g) = match state {
        SlotState::Free => (0, 0),
        SlotState::Live(g) => (1, g),
        SlotState::Retired(g) => (2, g),
    };
    t[0] = kind;
    t[8..].copy_from_slice(&g.to_le_bytes());
    t
}
fn parse_tag(t: [u8; 16]) -> Result<SlotState> {
    let g = u64::from_le_bytes(t[8..].try_into().unwrap());
    if t[1..8] != [0; 7] {
        return Err(Error::Corrupt("resident checkpoint slot reserved"));
    }
    match (t[0], g) {
        (0, 0) => Ok(SlotState::Free),
        (1, g) if g > 0 && g <= MAX_GENERATION => Ok(SlotState::Live(g)),
        (2, g) if g > 0 && g <= MAX_GENERATION => Ok(SlotState::Retired(g)),
        _ => Err(Error::Corrupt("resident checkpoint slot state")),
    }
}
/// Creation streams pages directly from the immutable view; no record-size or
/// contiguous live-image allocation restriction applies to a checkpoint.
pub(super) fn create(
    parent: &Path,
    boot: Boot,
    slots: &[SlotState],
    selected: WalReceipt,
    root: Reference,
    next_database: u64,
    view: &View,
    context: Option<Arc<crate::transform::TransformContext>>,
) -> Result<Seed> {
    let seed = new_seed(boot, slots, selected, root, next_database, view)?;
    let path = seed.path(parent);
    let file = ulmdb_platform::create_private_file(&path, false)?;
    let result = write_checkpoint(file, seed, boot, slots, view, context).and_then(|seed| {
        ulmdb_platform::sync_path(parent)?;
        Ok(seed)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    result
}
pub(super) fn create_in_export(
    output: &mut ulmdb_platform::owned_export::OwnedExport,
    boot: Boot,
    slots: &[SlotState],
    selected: WalReceipt,
    root: Reference,
    next_database: u64,
    view: &View,
    context: Option<Arc<crate::transform::TransformContext>>,
) -> Result<Seed> {
    let seed = new_seed(boot, slots, selected, root, next_database, view)?;
    let name = format!(".resident-checkpoint-{:032x}", seed.generation);
    let file = output.create_file(std::ffi::OsStr::new(&name))?;
    write_checkpoint(file, seed, boot, slots, view, context)
}
fn new_seed(
    boot: Boot,
    slots: &[SlotState],
    selected: WalReceipt,
    root: Reference,
    next_database: u64,
    view: &View,
) -> Result<Seed> {
    let seed = Seed {
        generation: crate::new_identity()?,
        digest: [1; 32],
        lsn: selected.lsn.checked_add(1).ok_or(Error::LimitExceeded)?,
        sequence: selected.sequence,
        root,
        next_database,
    };
    seed.validate(boot)?;
    if slots.len() != boot.slots()? || view.slot_capacity() != slots.len() {
        return Err(Error::Corrupt("resident checkpoint geometry"));
    }
    Ok(seed)
}
fn write_checkpoint(
    file: File,
    mut seed: Seed,
    boot: Boot,
    slots: &[SlotState],
    view: &View,
    context: Option<Arc<crate::transform::TransformContext>>,
) -> Result<Seed> {
    let length = HEADER as u64
        + slots.len() as u64 * 16
        + slots
            .iter()
            .filter(|s| matches!(s, SlotState::Live(_)))
            .count() as u64
            * LIVE_BYTES;
    let mut f = crate::transform::StreamWriter::new(
        file,
        context,
        crate::transform::ObjectKind::ResidentCheckpoint,
        seed.generation,
        length,
    )?;
    (|| {
        let h = header(boot, seed);
        let mut digest = hash();
        f.write_all(&h)?;
        digest.update(h);
        let mut live = 0;
        for (slot, state) in slots.iter().copied().enumerate() {
            let present = view.version(slot as u32).map(|p| p.key());
            if present
                != match state {
                    SlotState::Live(g) => Some(VersionKey {
                        slot: slot as u32,
                        generation: g,
                    }),
                    _ => None,
                }
            {
                return Err(Error::Corrupt("resident checkpoint view/state mismatch"));
            }
            live += usize::from(matches!(state, SlotState::Live(_)));
            let t = tag(state);
            f.write_all(&t)?;
            digest.update(t);
        }
        if live != view.member_count() {
            return Err(Error::Corrupt("resident checkpoint member count"));
        }
        for key in view.members() {
            let v = view
                .version(key.slot)
                .ok_or(Error::Corrupt("resident checkpoint missing page"))?;
            let ranges = v.literal_ranges();
            super::wal_codec::validate_ranges(boot, key.slot, ranges)?;
            let mut encoded = [0; 1024];
            for (i, (start, len)) in ranges.iter().enumerate() {
                encoded[i * 4..i * 4 + 2].copy_from_slice(&start.to_le_bytes());
                encoded[i * 4 + 2..i * 4 + 4].copy_from_slice(&len.to_le_bytes());
            }
            f.write_all(&encoded)?;
            digest.update(encoded);
            f.write_all(v.literal_bytes())?;
            digest.update(v.literal_bytes());
        }
        f.sync_all()?;
        seed.digest = digest.finalize().into();
        Ok(seed)
    })()
}
/// Metadata is bounded by configured slot count. Live page materialization is
/// streamed and authenticated again before exposing a recovery view.
pub(super) struct Reader {
    file: crate::transform::StreamReader,
    boot: Boot,
    seed: Seed,
    pub slots: Vec<SlotState>,
    live: u64,
    length: u64,
}
impl Reader {
    pub fn open(
        parent: &Path,
        boot: Boot,
        seed: Seed,
        context: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        seed.validate(boot)?;
        let path = seed.path(parent);
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
            .open(path)?;
        let m = ulmdb_platform::file_identity(&file)?;
        if !m.is_file() || m.links != 1 {
            return Err(Error::Corrupt("resident checkpoint file kind/links"));
        }
        let file = crate::transform::StreamReader::new(
            file,
            context,
            crate::transform::ObjectKind::ResidentCheckpoint,
            seed.generation,
        )?;
        let length = file.len();
        let mut r = Self {
            file,
            boot,
            seed,
            slots: Vec::new(),
            live: 0,
            length,
        };
        let mut h = [0; HEADER];
        r.file.read_exact(&mut h)?;
        if h != header(boot, seed) {
            return Err(Error::Corrupt("resident checkpoint header identity"));
        }
        r.slots
            .try_reserve_exact(boot.slots()?)
            .map_err(|_| Error::LimitExceeded)?;
        for _ in 0..boot.slots()? {
            let mut t = [0; 16];
            r.file.read_exact(&mut t)?;
            let state = parse_tag(t)?;
            r.live += u64::from(matches!(state, SlotState::Live(_)));
            r.slots.push(state);
        }
        let expected = HEADER as u64 + boot.slots()? as u64 * 16 + r.live * LIVE_BYTES;
        if r.length != expected {
            return Err(Error::Corrupt("resident checkpoint extent"));
        }
        Ok(r)
    }
    pub fn seed(&self) -> Seed {
        self.seed
    }
    pub fn restore(&mut self, pool: &Arc<Pool>) -> Result<View> {
        use std::io::SeekFrom;
        if pool.geometry() != (self.boot.kv_pages as usize, self.boot.tiny_pages as usize) {
            return Err(Error::Corrupt("resident checkpoint pool geometry"));
        }
        self.file.seek(SeekFrom::Start(0))?;
        let mut digest = hash();
        let mut h = [0; HEADER];
        self.file.read_exact(&mut h)?;
        if h != header(self.boot, self.seed) {
            return Err(Error::Corrupt("resident checkpoint changed header"));
        }
        digest.update(h);
        for state in &self.slots {
            let mut actual = [0; 16];
            self.file.read_exact(&mut actual)?;
            if actual != tag(*state) {
                return Err(Error::Corrupt("resident checkpoint changed state"));
            }
            digest.update(actual);
        }
        let mut view = pool.empty()?;
        for (slot, state) in self.slots.iter().copied().enumerate() {
            match state {
                SlotState::Retired(g) => pool.restore_retired(slot as u32, g)?,
                SlotState::Free => {}
                SlotState::Live(g) => {
                    let mut encoded = [0; 1024];
                    self.file.read_exact(&mut encoded)?;
                    digest.update(encoded);
                    let mut ranges = Vec::new();
                    let mut zero = false;
                    for r in encoded.chunks_exact(4) {
                        let start = u16::from_le_bytes(r[..2].try_into().unwrap());
                        let len = u16::from_le_bytes(r[2..].try_into().unwrap());
                        if len == 0 {
                            if start != 0 {
                                return Err(Error::Corrupt("resident checkpoint range padding"));
                            }
                            zero = true;
                        } else {
                            if zero {
                                return Err(Error::Corrupt("resident checkpoint range order"));
                            }
                            ranges.push((start, len));
                        }
                    }
                    super::wal_codec::validate_ranges(self.boot, slot as u32, &ranges)?;
                    let mut bytes = [0; PAGE];
                    self.file.read_exact(&mut bytes)?;
                    digest.update(bytes);
                    let mut p = pool.replay_permit(VersionKey {
                        slot: slot as u32,
                        generation: g,
                    })?;
                    p.write(0, &bytes)?;
                    p.ranges = ranges;
                    p.cursor = PAGE;
                    let v = p.seal();
                    v.publish();
                    view = view.changed([v], &[])?;
                }
            }
        }
        if <[u8; 32]>::from(digest.finalize()) != self.seed.digest || self.file.len() != self.length
        {
            return Err(Error::Corrupt("resident checkpoint digest/extent"));
        }
        self.file.check()?;
        pool.enable_checkpoint_tiny_reuse()?;
        Ok(view)
    }
}
