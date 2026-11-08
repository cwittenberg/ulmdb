//! Linux-only, opt-in two-bank snapshot/transaction log.
//!
//! New persistent environments on4KiB-page hosts use v4 packed small redo plus
//! compact bulk frames. New RAM/other-page environments retain v3 compact
//! frames. Existing v3/v2 single-inode and legacy codecs remain unchanged.
//! New environments use one owned inode: a checksummed header page followed by
//! two fixed banks. Legacy lock-plus-two-bank environments remain readable and
//! writable in their original layout. Each bank has a complete self-contained
//! checkpoint; recovery selects the highest valid generation and consecutive
//! frames. Persistent append acknowledgment follows one synchronous
//! positioned write of header, stored body, footer and zero next-tail marker.
//! RWF_DSYNC supplies data-integrity completion on the exact owned inode; an
//! unsupported syscall/filesystem falls back to positioned writes+fdatasync.
//! Actual tmpfs files use mapped mutation followed by sync_data instead; that
//! choice is made once from the owned inode's filesystem at create/open.
//! Checkpoint selection follows the same synchronous fence. No async flag or
//! existing format fence is removed.
//! A v4 OPEN packet terminates replay without reading or repairing an outside
//! marker. Moving to a successor co-fences a CLOSED certificate with that
//! child's complete bytes; recovery requires every CLOSED child to validate
//! before returning any transactions. A torn nonzero certificate or missing
//! required child fails closed, including an unacknowledged interrupted write.
//! Rollover may co-fence at most16KiB of unused-bank zeros. Initial packets and
//! setup do not reserve future pages. All submitted bytes are reported.
//!
//! The writer owns an exclusive cooperative filesystem lock and never lends
//! mapping bytes to callers. Hostile truncation/mutation outside that protocol
//! can cause SIGBUS or corruption. Neither fdatasync nor SHA-256 proves atomic
//! sectors or publisher identity. Power-cut availability remains unqualified:
//! a complete current-generation frame with a plausible extent and a bad
//! checksum fails closed, including an interrupted attempt whose footer reached
//! storage before its body. A compact-frame generation mismatch or impossible
//! extent is an unselected frontier unless a CLOSED parent requires that child;
//! this recovery policy does
//! not distinguish every corrupted acknowledged tail from an interrupted tail.
//! An incomplete frontier without a complete footer is unselected and only its
//! first eight bytes are repaired; previously selected frames are not erased.

mod codec;
mod io;
mod pages;
mod stream;

use crate::{Error, Result};
use memmap2::MmapMut;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};
use ulmdb_platform::{FileExt, LockMode, RangeLock};

const LOCK_NAME: &str = "SNAPSHOTLOG.lock";
const BANK_NAMES: [&str; 2] = ["SNAPSHOTLOG.0", "SNAPSHOTLOG.1"];
const FILE_NAME: &str = "SNAPSHOTLOG";
const LOCK_MAGIC: &[u8; 8] = b"ULMSLK01";
const FILE_MAGIC_V2: &[u8; 8] = b"ULMSJL02";
const FILE_MAGIC: &[u8; 8] = b"ULMSJL03";
const FILE_MAGIC_V4: &[u8; 8] = b"ULMSJL04";
const CP_MAGIC: &[u8; 8] = b"ULMSCP01";
const TX_MAGIC: &[u8; 8] = b"ULMSTX01";
const COMPACT_TX_MAGIC: &[u8; 8] = b"ULMSTC03";
const COMPRESSED_TX_MAGIC: &[u8; 8] = b"ULMSTZ03";
const COMPACT_DOMAIN: &[u8; 8] = b"ULMSCTX3";
const CP_END: &[u8; 8] = b"ULMSCE01";
const TX_END: &[u8; 8] = b"ULMSTE01";
const LOCK_SIZE: usize = 64;
const FILE_HEADER: usize = 96;
const CP_HEADER: usize = 96;
const TX_HEADER: usize = 80;
const COMPACT_TX_HEADER: usize = 24;
const FOOTER: usize = 48;
const MARKER: usize = 8;
const MAX_BANK: usize = 1 << 30;
// Keep compression work bounded to a small record even when a newer logical
// codec packs a multirow transaction below the decoder's2KiB format limit.
const WRITE_CODEC_MAX_INPUT: usize = 1024;
const WRITE_PAGE_MAX_BODY: usize = 256;
const PACKET_WINDOW_BYTES: usize = 16 * 1024;
static PACKET_ZEROS: [u8; PACKET_WINDOW_BYTES] = [0; PACKET_WINDOW_BYTES];

// The single-inode header selects a codec. Existing v2 single-inode and
// legacy three-file environments keep their byte-identical original frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameFormat {
    Original,
    Compact,
    Paged,
}
impl FrameFormat {
    fn header(self) -> usize {
        match self {
            Self::Original => TX_HEADER,
            Self::Compact | Self::Paged => COMPACT_TX_HEADER,
        }
    }
    fn magic(self) -> &'static [u8; 8] {
        match self {
            Self::Original => TX_MAGIC,
            Self::Compact | Self::Paged => COMPACT_TX_MAGIC,
        }
    }
    fn length_offset(self) -> usize {
        match self {
            Self::Original => 40,
            Self::Compact | Self::Paged => 16,
        }
    }
    fn is_compact(self) -> bool {
        self != Self::Original
    }
    fn version(self) -> u8 {
        match self {
            Self::Original => 2,
            Self::Compact => 3,
            Self::Paged => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Header format:1 legacy,2 original single-inode,3 compact,4 packed hybrid.
    pub format_version: u8,
    /// Fixed bank extents, excluding the separately reported header; not RSS.
    pub capacity_bytes: usize,
    /// New single-inode layout; false for the preserved legacy three files.
    pub single_file: bool,
    /// Header page in the new layout or the legacy 64-byte lock header.
    pub header_bytes: usize,
    /// RAM filesystems use mapped mutation plus sync_data; persistent v3 files
    /// use synchronous positioned writes. Existing formats stay mapped.
    pub mapped_appends: bool,
    /// Current writer policy; the decoder retains its2KiB format bound.
    pub codec_max_input_bytes: usize,
    /// Producer limit; the page decoder accepts at most1024 stored bytes.
    pub packed_max_body_bytes: usize,
    pub packed_pages: u64,
    pub packed_records: u64,
    pub closed_pages: u64,
    pub reservation_window_bytes: usize,
    pub initialized_end_bytes: usize,
    /// Attempted co-fenced rollover reservations, including uncertain I/O.
    pub reservation_calls: u64,
    pub reservation_zero_bytes: u64,
    pub bank_capacity_bytes: usize,
    pub used_bytes: usize,
    /// Selected prefix measured with uncompressed transaction frame extents.
    pub logical_used_bytes: usize,
    /// Eligible optional encoder attempts before transaction I/O.
    pub codec_attempts: u64,
    /// Successfully appended compressed frames in this Journal instance.
    pub compressed_frames: u64,
    /// Committed uncompressed BODY bytes; excludes frame headers/footer/marker.
    pub compressed_input_bytes: u64,
    /// Committed encoded BODY bytes, including decoded-length prefix only.
    pub compressed_stored_bytes: u64,
    /// Attempted journal durability fences, including synchronous appends.
    /// A fallback syscall attempt is not counted as a second logical fence.
    pub sync_calls: u64,
    pub append_frames: u64,
    /// All submitted bytes, including closed-page certificates and future zeros.
    pub append_bytes: u64,
    /// Successful new checkpoints, including initial creation.
    pub checkpoints: u64,
    /// Bytes copied for checkpoint prefixes and markers, not entire banks.
    pub checkpoint_bytes: u64,
    /// Serialized bytes examined by recovery; not a physical I/O counter.
    pub replay_bytes: u64,
    pub tail_repairs: u64,
}

struct Bank {
    map: MmapMut,
    // Keep the owned exact inode open for the full mapping lifetime.
    file: File,
    offset: u64,
}

/// One exclusive process-local owner. Keep it behind the engine's writer gate.
pub struct Journal {
    banks: [Bank; 2],
    active: usize,
    format: FrameFormat,
    mapped_appends: bool,
    source: [u8; 16],
    generation: u64,
    sequence: u64,
    tail: usize,
    // Replay/retention budget in uncompressed frame extents, preventing a
    // compressed physical bank from expanding beyond its configured capacity.
    logical_tail: usize,
    page: Option<pages::PageState>,
    last_hash: [u8; 32],
    capacity: usize,
    creator: u32,
    poisoned: bool,
    stats: Stats,
    // Declared last so bank mappings disappear before the writer lease.
    _lock: RangeLock,
}

#[derive(Clone, Copy)]
struct Checkpoint {
    generation: u64,
    sequence: u64,
    payload_len: usize,
    end: usize,
    hash: [u8; 32],
}

fn checksum(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn frame_checksum(
    format: FrameFormat,
    source: [u8; 16],
    sequence: u64,
    previous: [u8; 32],
    bytes: &[u8],
) -> [u8; 32] {
    if format == FrameFormat::Original {
        return checksum(bytes);
    }
    compact_checksum(source, sequence, previous, bytes, &[])
}
fn compact_checksum(
    source: [u8; 16],
    sequence: u64,
    previous: [u8; 32],
    header: &[u8],
    payload: &[u8],
) -> [u8; 32] {
    let mut digest = Sha256::new();
    // Fixed-width domain/context binds source, expected position and parent
    // without copying those fields into every on-disk frame. The complete
    // compact header (including generation/length) and payload follow it.
    digest.update(COMPACT_DOMAIN);
    digest.update(source);
    digest.update(sequence.to_le_bytes());
    digest.update(previous);
    digest.update(header);
    digest.update(payload);
    digest.finalize().into()
}
fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("checked header"),
    )
}
fn aligned(length: usize) -> Result<usize> {
    length
        .checked_add(7)
        .map(|n| n & !7)
        .ok_or(Error::LimitExceeded)
}
fn record_extent(header: usize, payload: usize) -> Result<usize> {
    aligned(
        header
            .checked_add(payload)
            .and_then(|n| n.checked_add(FOOTER))
            .ok_or(Error::LimitExceeded)?,
    )
}
fn marker_end(end: usize, capacity: usize) -> usize {
    if capacity - end >= MARKER {
        end + MARKER
    } else {
        end
    }
}
fn check_capacity(capacity: usize) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(Error::InvalidInput(
            "snapshot log currently requires Linux mapped synchronous data",
        ));
    }
    let page = ulmdb_platform::page_size()?;
    if capacity < page || capacity > MAX_BANK || !capacity.is_multiple_of(page) {
        return Err(Error::InvalidInput(
            "snapshot log bank capacity must be page aligned and <=1GiB",
        ));
    }
    Ok(())
}

mod mapped_io {
    use super::*;
    /// The exclusive cooperative writer lease is already held. The file is a
    /// checked regular engine-owned inode with fixed extent. No mapping slice
    /// escapes Journal, and the file must never be externally truncated.
    #[allow(unsafe_code)]
    pub(super) fn map(file: &File, capacity: usize, offset: u64) -> Result<MmapMut> {
        // SAFETY: the caller establishes the ownership/extent contract above;
        // MmapMut uses MAP_SHARED, and all mutation requires &mut Journal.
        Ok(unsafe {
            memmap2::MmapOptions::new()
                .offset(offset)
                .len(capacity)
                .map_mut(file)?
        })
    }
}

fn open_owned(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    let id = ulmdb_platform::file_identity(&file)?;
    if !id.is_file() || id.links != 1 {
        return Err(Error::Corrupt("snapshot log inode kind/links"));
    }
    Ok(file)
}
fn lease(file: File) -> Result<RangeLock> {
    RangeLock::acquire(file, LockMode::Exclusive, 0, 1, false).map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            e.into()
        }
    })
}
fn bank(file: File, capacity: usize, offset: usize, file_extent: usize) -> Result<Bank> {
    if file.metadata()?.len() != file_extent as u64 {
        return Err(Error::Corrupt("snapshot log bank extent"));
    }
    Ok(Bank {
        map: mapped_io::map(&file, capacity, offset as u64)?,
        file,
        offset: offset as u64,
    })
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn legacy_exists(directory: &Path) -> Result<bool> {
    for name in [LOCK_NAME, BANK_NAMES[0], BANK_NAMES[1]] {
        if exists(&directory.join(name))? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn single_file_extent(capacity: usize, header_bytes: usize) -> Result<usize> {
    capacity
        .checked_mul(2)
        .and_then(|n| n.checked_add(header_bytes))
        .ok_or(Error::LimitExceeded)
}

fn single_banks(lock: &RangeLock, capacity: usize, header_bytes: usize) -> Result<[Bank; 2]> {
    let extent = single_file_extent(capacity, header_bytes)?;
    Ok([
        bank(lock.file().try_clone()?, capacity, header_bytes, extent)?,
        bank(
            lock.file().try_clone()?,
            capacity,
            header_bytes + capacity,
            extent,
        )?,
    ])
}

fn write_checkpoint(
    map: &mut [u8],
    source: [u8; 16],
    generation: u64,
    sequence: u64,
    parent_generation: u64,
    parent_hash: [u8; 32],
    payload: &[u8],
) -> Result<(usize, [u8; 32], usize)> {
    let end = record_extent(CP_HEADER, payload.len())?;
    if end > map.len() {
        return Err(Error::LimitExceeded);
    }
    map[..CP_HEADER].fill(0);
    map[..8].copy_from_slice(CP_MAGIC);
    map[8..24].copy_from_slice(&source);
    map[24..32].copy_from_slice(&generation.to_le_bytes());
    map[32..40].copy_from_slice(&sequence.to_le_bytes());
    map[40..48].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    map[48..56].copy_from_slice(&parent_generation.to_le_bytes());
    map[56..88].copy_from_slice(&parent_hash);
    map[CP_HEADER..CP_HEADER + payload.len()].copy_from_slice(payload);
    let footer = CP_HEADER + payload.len();
    let hash = checksum(&map[..footer]);
    map[footer..footer + 32].copy_from_slice(&hash);
    map[footer + 32..footer + 40].copy_from_slice(CP_END);
    map[footer + 40..footer + FOOTER].copy_from_slice(&(end as u64).to_le_bytes());
    map[footer + FOOTER..end].fill(0);
    let flushed_end = marker_end(end, map.len());
    map[end..flushed_end].fill(0);
    Ok((end, hash, flushed_end))
}

fn parse_checkpoint(
    map: &[u8],
    source: [u8; 16],
    examined: &mut u64,
) -> Result<Option<Checkpoint>> {
    *examined += CP_HEADER as u64;
    if map[..8] != *CP_MAGIC {
        if complete_footer(map, 0, CP_HEADER, 40, CP_END) {
            return Err(Error::Corrupt("snapshot log complete checkpoint magic"));
        }
        return Ok(None);
    }
    // The leased header durably binds this source independently of either bank.
    if map[8..24] != source {
        return Err(Error::Corrupt("snapshot log checkpoint source"));
    }
    let generation = read_u64(map, 24);
    let sequence = read_u64(map, 32);
    let Ok(payload_len) = usize::try_from(read_u64(map, 40)) else {
        return Ok(None);
    };
    let Ok(end) = record_extent(CP_HEADER, payload_len) else {
        return Ok(None);
    };
    if end > map.len() {
        return Ok(None);
    }
    let footer = CP_HEADER + payload_len;
    if generation == 0 || map[88..96] != [0; 8] {
        return if complete_footer(map, 0, CP_HEADER, 40, CP_END) {
            Err(Error::Corrupt("snapshot log complete checkpoint header"))
        } else {
            Ok(None)
        };
    }
    let parent = read_u64(map, 48);
    if (generation == 1 && (parent != 0 || map[56..88] != [0; 32]))
        || (generation != 1 && parent.checked_add(1) != Some(generation))
    {
        return if complete_footer(map, 0, CP_HEADER, 40, CP_END) {
            Err(Error::Corrupt("snapshot log complete checkpoint parent"))
        } else {
            Ok(None)
        };
    }
    *examined += (end - CP_HEADER) as u64;
    if map[footer + 32..footer + 40] != *CP_END {
        return Ok(None);
    }
    if read_u64(map, footer + 40) != end as u64 || map[footer + FOOTER..end].iter().any(|&b| b != 0)
    {
        return Err(Error::Corrupt("snapshot log complete checkpoint footer"));
    }
    let hash = checksum(&map[..footer]);
    if map[footer..footer + 32] != hash {
        return Err(Error::Corrupt("snapshot log complete checkpoint checksum"));
    }
    Ok(Some(Checkpoint {
        generation,
        sequence,
        payload_len,
        end,
        hash,
    }))
}

fn complete_footer(
    map: &[u8],
    start: usize,
    header: usize,
    length_offset: usize,
    magic: &[u8; 8],
) -> bool {
    let Some(remaining) = map.len().checked_sub(start) else {
        return false;
    };
    if remaining < header + FOOTER {
        return false;
    }
    let Ok(length) = usize::try_from(read_u64(map, start + length_offset)) else {
        return false;
    };
    let Ok(extent) = record_extent(header, length) else {
        return false;
    };
    if extent > remaining {
        return false;
    }
    let footer = start + header + length;
    map[footer + 32..footer + 40] == *magic
}

impl Journal {
    pub fn create(directory: &Path, capacity: usize, initial: &[u8]) -> Result<Self> {
        Self::create_inner(directory, capacity, initial, true)
    }
    fn create_inner(
        directory: &Path,
        capacity: usize,
        initial: &[u8],
        prefer_packed: bool,
    ) -> Result<Self> {
        check_capacity(capacity)?;
        if record_extent(CP_HEADER, initial.len())? > capacity {
            return Err(Error::LimitExceeded);
        }
        let header_bytes = ulmdb_platform::page_size()?;
        let extent = single_file_extent(capacity, header_bytes)?;
        let created_directory = !directory.exists();
        if created_directory {
            fs::create_dir(directory)?;
        }
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::InvalidInput("snapshot log directory"));
        }
        if legacy_exists(directory)? {
            return Err(Error::InvalidInput(
                "snapshot log legacy layout already exists",
            ));
        }
        let file = ulmdb_platform::create_private_file(&directory.join(FILE_NAME), true)?;
        let lock = lease(file)?;
        let source = crate::new_identity()?.to_le_bytes();
        let mapped_appends = io::is_ram_backed(lock.file()).unwrap_or(false);
        let format = if prefer_packed && !mapped_appends && header_bytes == pages::PAGE_BYTES {
            FrameFormat::Paged
        } else {
            FrameFormat::Compact
        };
        let mut header = [0; FILE_HEADER];
        header[..8].copy_from_slice(if format == FrameFormat::Paged {
            FILE_MAGIC_V4
        } else {
            FILE_MAGIC
        });
        header[8..24].copy_from_slice(&source);
        header[24..32].copy_from_slice(&(capacity as u64).to_le_bytes());
        header[32..40].copy_from_slice(&(header_bytes as u64).to_le_bytes());
        let hash = checksum(&header[..64]);
        header[64..].copy_from_slice(&hash);
        lock.file().set_len(extent as u64)?;
        lock.file().write_all_at(&header, 0)?;
        let mut banks = single_banks(&lock, capacity, header_bytes)?;
        let (tail, last_hash, flush_end) =
            write_checkpoint(&mut banks[0].map, source, 1, 0, 0, [0; 32], initial)?;
        // One synchronous fence covers the exact inode's retrieval-critical
        // extent, header and mapped checkpoint before publishing its directory.
        lock.file().sync_data()?;
        ulmdb_platform::sync_directory(directory)?;
        let mut directory_fences = 1;
        if created_directory {
            if let Some(parent) = directory.parent() {
                // Path::new("db").parent() is the empty path, denoting CWD.
                let parent = if parent.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    parent
                };
                ulmdb_platform::sync_directory(parent)?;
                directory_fences += 1;
            }
        }
        Ok(Self {
            banks,
            active: 0,
            format,
            mapped_appends,
            source,
            generation: 1,
            sequence: 0,
            tail,
            logical_tail: tail,
            page: None,
            last_hash,
            capacity,
            creator: std::process::id(),
            poisoned: false,
            stats: Stats {
                format_version: format.version(),
                capacity_bytes: capacity * 2,
                single_file: true,
                header_bytes,
                mapped_appends,
                codec_max_input_bytes: WRITE_CODEC_MAX_INPUT,
                packed_max_body_bytes: if format == FrameFormat::Paged {
                    WRITE_PAGE_MAX_BODY
                } else {
                    0
                },
                reservation_window_bytes: if format == FrameFormat::Paged {
                    PACKET_WINDOW_BYTES
                } else {
                    0
                },
                initialized_end_bytes: if format == FrameFormat::Paged {
                    flush_end.div_ceil(pages::PAGE_BYTES) * pages::PAGE_BYTES
                } else {
                    0
                },
                bank_capacity_bytes: capacity,
                used_bytes: tail,
                logical_used_bytes: tail,
                sync_calls: 1 + directory_fences,
                checkpoints: 1,
                checkpoint_bytes: flush_end as u64,
                ..Stats::default()
            },
            _lock: lock,
        })
    }

    /// Return the selected full checkpoint and its ordered transaction payloads.
    /// Total copied payload bytes are bounded by one configured bank capacity.
    pub fn open(directory: &Path, capacity: usize) -> Result<(Self, Vec<u8>, Vec<Vec<u8>>)> {
        check_capacity(capacity)?;
        let single_file = exists(&directory.join(FILE_NAME))?;
        let (lock, source, banks, header_bytes, format) = if single_file {
            let lock = lease(open_owned(&directory.join(FILE_NAME))?)?;
            if legacy_exists(directory)? {
                return Err(Error::Corrupt("snapshot log mixed layouts"));
            }
            let header_bytes = ulmdb_platform::page_size()?;
            if lock.file().metadata()?.len() != single_file_extent(capacity, header_bytes)? as u64 {
                return Err(Error::Corrupt("snapshot log single file extent"));
            }
            let mut header = [0; FILE_HEADER];
            lock.file().read_exact_at(&mut header, 0)?;
            let format = if header[..8] == *FILE_MAGIC_V4 && header_bytes == pages::PAGE_BYTES {
                FrameFormat::Paged
            } else if header[..8] == *FILE_MAGIC {
                FrameFormat::Compact
            } else if header[..8] == *FILE_MAGIC_V2 {
                FrameFormat::Original
            } else {
                return Err(Error::Corrupt("snapshot log single file header"));
            };
            if checksum(&header[..64]) != header[64..]
                || read_u64(&header, 24) != capacity as u64
                || read_u64(&header, 32) != header_bytes as u64
                || header[40..64] != [0; 24]
            {
                return Err(Error::Corrupt("snapshot log single file header"));
            }
            let source = header[8..24].try_into().expect("fixed source");
            let banks = single_banks(&lock, capacity, header_bytes)?;
            (lock, source, banks, header_bytes, format)
        } else {
            let lock = lease(open_owned(&directory.join(LOCK_NAME))?)?;
            if lock.file().metadata()?.len() != LOCK_SIZE as u64 {
                return Err(Error::Corrupt("snapshot log lock extent"));
            }
            let mut header = [0; LOCK_SIZE];
            lock.file().read_exact_at(&mut header, 0)?;
            if header[..8] != *LOCK_MAGIC
                || checksum(&header[..32]) != header[32..]
                || read_u64(&header, 24) != capacity as u64
            {
                return Err(Error::Corrupt("snapshot log source/capacity header"));
            }
            let source = header[8..24].try_into().expect("fixed source");
            let banks = [
                bank(
                    open_owned(&directory.join(BANK_NAMES[0]))?,
                    capacity,
                    0,
                    capacity,
                )?,
                bank(
                    open_owned(&directory.join(BANK_NAMES[1]))?,
                    capacity,
                    0,
                    capacity,
                )?,
            ];
            (lock, source, banks, LOCK_SIZE, FrameFormat::Original)
        };
        let mut examined = if single_file { FILE_HEADER } else { LOCK_SIZE } as u64;
        let a = parse_checkpoint(&banks[0].map, source, &mut examined)?;
        let b = parse_checkpoint(&banks[1].map, source, &mut examined)?;
        let (active, cp) = match (a, b) {
            (Some(a), Some(b)) if a.generation > b.generation => (0, a),
            (Some(a), Some(b)) if b.generation > a.generation => (1, b),
            (Some(_), Some(_)) => {
                return Err(Error::Corrupt(
                    "snapshot log duplicate checkpoint generation",
                ));
            }
            (Some(a), None) => (0, a),
            (None, Some(b)) => (1, b),
            (None, None) => return Err(Error::Corrupt("snapshot log has no complete checkpoint")),
        };
        if let (Some(a), Some(b)) = (a, b) {
            if a.generation.abs_diff(b.generation) != 1
                || (a.generation > b.generation && a.sequence < b.sequence)
                || (b.generation > a.generation && b.sequence < a.sequence)
            {
                return Err(Error::Corrupt("snapshot log checkpoint lineage"));
            }
        }
        let baseline = copy_payload(&banks[active].map[CP_HEADER..CP_HEADER + cp.payload_len])?;
        let mapped_appends = format == FrameFormat::Original
            || (format == FrameFormat::Compact && io::is_ram_backed(lock.file()).unwrap_or(false));
        let mut journal = Self {
            banks,
            active,
            format,
            mapped_appends,
            source,
            generation: cp.generation,
            sequence: cp.sequence,
            tail: cp.end,
            logical_tail: cp.end,
            page: None,
            last_hash: cp.hash,
            capacity,
            creator: std::process::id(),
            poisoned: false,
            stats: Stats {
                format_version: if single_file { format.version() } else { 1 },
                capacity_bytes: capacity * 2,
                single_file,
                header_bytes,
                mapped_appends,
                codec_max_input_bytes: if format.is_compact() {
                    WRITE_CODEC_MAX_INPUT
                } else {
                    0
                },
                packed_max_body_bytes: if format == FrameFormat::Paged {
                    WRITE_PAGE_MAX_BODY
                } else {
                    0
                },
                reservation_window_bytes: if format == FrameFormat::Paged {
                    PACKET_WINDOW_BYTES
                } else {
                    0
                },
                bank_capacity_bytes: capacity,
                replay_bytes: examined,
                ..Stats::default()
            },
            _lock: lock,
        };
        let mut transactions = Vec::new();
        let dirty_tail = if format == FrameFormat::Paged {
            let recovered = stream::recover(
                &journal.banks[active].map,
                cp.end,
                cp.end,
                pages::Context {
                    source,
                    generation: cp.generation,
                    sequence: cp.sequence,
                    hash: cp.hash,
                },
                capacity,
            )?;
            journal.tail = recovered.tail;
            journal.logical_tail = recovered.logical_tail;
            journal.sequence = recovered.context.sequence;
            journal.last_hash = recovered.context.hash;
            journal.page = recovered.page;
            journal.stats.replay_bytes += recovered.replay_bytes;
            journal.stats.append_frames = recovered.frames;
            journal.stats.packed_pages = recovered.packed_pages;
            journal.stats.packed_records = recovered.packed_records;
            journal.stats.closed_pages = recovered.closed_pages;
            transactions = recovered.transactions;
            recovered.dirty_tail
        } else {
            journal.replay(&mut transactions)?
        };
        if dirty_tail {
            // Repair only the unselected frontier. Never zero a selected frame.
            let start = journal.tail;
            let end = marker_end(start, capacity);
            if end > start {
                journal.banks[active].map[start..end].fill(0);
                journal.stats.sync_calls += 1;
                journal.banks[active].file.sync_data()?;
                journal.stats.tail_repairs += 1;
            }
        }
        journal.stats.used_bytes = journal.tail;
        journal.stats.logical_used_bytes = journal.logical_tail;
        if format == FrameFormat::Paged {
            let initialized = if journal.page.is_some() {
                journal.tail
            } else {
                marker_end(journal.tail, capacity)
            };
            journal.stats.initialized_end_bytes = initialized
                .div_ceil(pages::PAGE_BYTES)
                .saturating_mul(pages::PAGE_BYTES)
                .min(capacity);
        }
        Ok((journal, baseline, transactions))
    }

    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited snapshot log writer"));
        }
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        Ok(())
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    /// New file formats admit dense logical effects; existing formats retain
    /// their original effect encoder for reader/writer compatibility.
    pub fn uses_compact_frames(&self) -> bool {
        self.format.is_compact()
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    pub fn stats(&self) -> Stats {
        self.stats
    }
    /// Conservative payload allowance, including worst-case alignment/marker.
    pub fn remaining(&self) -> usize {
        self.remaining_transaction(true)
    }
    pub fn remaining_transaction(&self, has_row_changes: bool) -> usize {
        let envelope = self.format.header() + FOOTER + MARKER + 7;
        let bulk = (self.capacity - self.tail).saturating_sub(envelope);
        let logical = (self.capacity - self.logical_tail).saturating_sub(envelope);
        if self.format != FrameFormat::Paged || !has_row_changes {
            return bulk.min(logical);
        }
        let packed = self
            .page
            .as_ref()
            .filter(|page| page.count() < pages::MAX_COUNT && !page.is_closed())
            .and_then(|page| page.available_body())
            .unwrap_or(0)
            .min(WRITE_PAGE_MAX_BODY);
        bulk.max(packed).min(logical)
    }

    pub fn append(&mut self, payload: &[u8]) -> Result<u64> {
        self.append_transaction(payload, true)
    }
    pub fn append_transaction(&mut self, payload: &[u8], has_row_changes: bool) -> Result<u64> {
        self.check()?;
        let header = self.format.header();
        let logical_extent = record_extent(header, payload.len())?;
        let logical_end = self
            .logical_tail
            .checked_add(logical_extent)
            .ok_or(Error::LimitExceeded)?;
        if logical_end > self.capacity {
            return Err(Error::LimitExceeded);
        }
        // Ineligible large transactions and small dense records do not scan
        // the payload or initialize scratch. Compression is optional/bounded.
        let mut scratch;
        let encoded = if self.format.is_compact()
            && (codec::MIN_INPUT..=WRITE_CODEC_MAX_INPUT).contains(&payload.len())
        {
            scratch = [0; codec::SCRATCH_BYTES];
            self.stats.codec_attempts = self.stats.codec_attempts.saturating_add(1);
            codec::encode(payload, &mut scratch)
        } else {
            None
        };
        let compressed = encoded.is_some();
        let stored = encoded.unwrap_or(payload);
        if self.format == FrameFormat::Paged
            && has_row_changes
            && stored.len() <= WRITE_PAGE_MAX_BODY
        {
            if let Some(sequence) =
                self.try_append_packet(stored, compressed, logical_end, payload.len())?
            {
                return Ok(sequence);
            }
        }
        let extent = record_extent(header, stored.len())?;
        let end = self.tail.checked_add(extent).ok_or(Error::LimitExceeded)?;
        if end > self.capacity {
            return Err(Error::LimitExceeded);
        }
        let sequence = self.sequence.checked_add(1).ok_or(Error::LimitExceeded)?;
        let start = self.tail;
        let flush_end = marker_end(end, self.capacity);
        if self.format == FrameFormat::Paged && self.page.is_some() {
            return self.append_bulk_with_close(
                stored,
                extent,
                flush_end,
                logical_end,
                payload.len(),
                compressed,
            );
        }
        let hash = if self.format.is_compact() && !self.mapped_appends {
            let bank = &self.banks[self.active];
            let offset = bank
                .offset
                .checked_add(start as u64)
                .ok_or(Error::LimitExceeded)?;
            bank.offset
                .checked_add(flush_end as u64)
                .filter(|&end| end <= libc::off_t::MAX as u64)
                .ok_or(Error::LimitExceeded)?;
            let mut prefix = [0; COMPACT_TX_HEADER];
            prefix[..8].copy_from_slice(if compressed {
                COMPRESSED_TX_MAGIC
            } else {
                COMPACT_TX_MAGIC
            });
            prefix[8..16].copy_from_slice(&self.generation.to_le_bytes());
            prefix[16..24].copy_from_slice(&(stored.len() as u64).to_le_bytes());
            let hash = compact_checksum(self.source, sequence, self.last_hash, &prefix, stored);
            // Fixed scratch holds the complete footer, at most7 alignment
            // bytes, and the optional8-byte next-tail marker. No transaction
            // body copy or mapped write occurs on this positioned append path.
            let mut suffix = [0; FOOTER + 7 + MARKER];
            suffix[..32].copy_from_slice(&hash);
            suffix[32..40].copy_from_slice(TX_END);
            suffix[40..48].copy_from_slice(&(extent as u64).to_le_bytes());
            let suffix_len = flush_end - start - COMPACT_TX_HEADER - stored.len();
            self.poisoned = true;
            self.stats.sync_calls += 1;
            self.stats.append_bytes += (flush_end - start) as u64;
            if io::write_sync(
                &bank.file,
                offset,
                &[&prefix, stored, &suffix[..suffix_len]],
            )
            .is_err()
            {
                return Err(Error::OutcomeUnknown);
            }
            hash
        } else {
            // Existing formats retain their original mapped-write path. RAM
            // filesystems use the same synchronous method with compact bytes:
            // their kernel write path adds copying without persistent I/O.
            self.poisoned = true;
            let map = &mut self.banks[self.active].map;
            map[start..start + header].fill(0);
            map[start..start + 8].copy_from_slice(if compressed {
                COMPRESSED_TX_MAGIC
            } else {
                self.format.magic()
            });
            match self.format {
                FrameFormat::Original => {
                    map[start + 8..start + 24].copy_from_slice(&self.source);
                    map[start + 24..start + 32].copy_from_slice(&self.generation.to_le_bytes());
                    map[start + 32..start + 40].copy_from_slice(&sequence.to_le_bytes());
                    map[start + 40..start + 48]
                        .copy_from_slice(&(stored.len() as u64).to_le_bytes());
                    map[start + 48..start + 80].copy_from_slice(&self.last_hash);
                }
                FrameFormat::Compact | FrameFormat::Paged => {
                    map[start + 8..start + 16].copy_from_slice(&self.generation.to_le_bytes());
                    map[start + 16..start + 24]
                        .copy_from_slice(&(stored.len() as u64).to_le_bytes());
                }
            }
            map[start + header..start + header + stored.len()].copy_from_slice(stored);
            let footer = start + header + stored.len();
            let hash = frame_checksum(
                self.format,
                self.source,
                sequence,
                self.last_hash,
                &map[start..footer],
            );
            map[footer..footer + 32].copy_from_slice(&hash);
            map[footer + 32..footer + 40].copy_from_slice(TX_END);
            map[footer + 40..footer + FOOTER].copy_from_slice(&(extent as u64).to_le_bytes());
            map[footer + FOOTER..flush_end].fill(0);
            self.stats.sync_calls += 1;
            self.stats.append_bytes += (flush_end - start) as u64;
            if self.banks[self.active].file.sync_data().is_err() {
                return Err(Error::OutcomeUnknown);
            }
            hash
        };
        self.sequence = sequence;
        self.tail = end;
        self.logical_tail = logical_end;
        self.last_hash = hash;
        self.stats.used_bytes = end;
        self.stats.logical_used_bytes = logical_end;
        if self.format == FrameFormat::Paged {
            self.stats.initialized_end_bytes = self
                .stats
                .initialized_end_bytes
                .max(flush_end.div_ceil(pages::PAGE_BYTES) * pages::PAGE_BYTES)
                .min(self.capacity);
        }
        self.stats.append_frames += 1;
        if compressed {
            self.stats.compressed_frames = self.stats.compressed_frames.saturating_add(1);
            self.stats.compressed_input_bytes = self
                .stats
                .compressed_input_bytes
                .saturating_add(payload.len() as u64);
            self.stats.compressed_stored_bytes = self
                .stats
                .compressed_stored_bytes
                .saturating_add(stored.len() as u64);
        }
        self.poisoned = false;
        Ok(sequence)
    }

    // Keep private480B page candidates and certificate construction out of
    // the retained RAM/format03 append stack, including under LTO.
    #[inline(never)]
    fn try_append_packet(
        &mut self,
        stored: &[u8],
        compressed: bool,
        logical_end: usize,
        input_len: usize,
    ) -> Result<Option<u64>> {
        if let Some(page) = &self.page {
            self.validate_page_context(page)?;
        }
        let staged = match self
            .page
            .as_ref()
            .map(|page| page.prepare(stored, compressed))
        {
            Some(Ok(prepared)) => Some((prepared, None)),
            Some(Err(Error::LimitExceeded)) | None => {
                match pages::PageState::new_packet(
                    pages::Context {
                        source: self.source,
                        generation: self.generation,
                        sequence: self.sequence,
                        hash: self.last_hash,
                    },
                    self.tail,
                    self.capacity,
                    stored,
                    compressed,
                ) {
                    Ok(prepared) => Some((
                        prepared,
                        self.page
                            .as_ref()
                            .map(|page| page.prepare_successor())
                            .transpose()?,
                    )),
                    Err(Error::LimitExceeded) => None, // A smaller03 fallback can still fit.
                    Err(error) => return Err(error),
                }
            }
            Some(Err(error)) => return Err(error),
        };
        if let Some((prepared, close)) = staged {
            return self
                .append_packet(prepared, close, logical_end, input_len, compressed)
                .map(Some);
        }
        Ok(None)
    }
    fn validate_page_context(&self, page: &pages::PageState) -> Result<()> {
        if page.end() != self.tail
            || page.is_closed()
            || page.context()
                != (pages::Context {
                    source: self.source,
                    generation: self.generation,
                    sequence: self.sequence,
                    hash: self.last_hash,
                })
        {
            return Err(Error::Corrupt("snapshot private page context"));
        }
        Ok(())
    }
    #[inline(never)]
    fn append_packet(
        &mut self,
        prepared: pages::Prepared<'_>,
        close: Option<pages::ClosingFooter>,
        logical_end: usize,
        input_len: usize,
        compressed: bool,
    ) -> Result<u64> {
        let is_new = prepared.is_new_packet();
        let page_end = prepared.page_end();
        let context = prepared.next_state().context();
        let mut combined = [0; pages::FOOTER_BYTES + pages::HEADER_BYTES + 2];
        let (offset, prefix) = if let Some(close) = &close {
            if !is_new || prepared.offset() != self.tail {
                return Err(Error::Corrupt("snapshot packet successor geometry"));
            }
            combined[..pages::FOOTER_BYTES].copy_from_slice(close.footer());
            combined[pages::FOOTER_BYTES..pages::FOOTER_BYTES + prepared.prefix().len()]
                .copy_from_slice(prepared.prefix());
            (
                close.offset(),
                &combined[..pages::FOOTER_BYTES + prepared.prefix().len()],
            )
        } else {
            (prepared.offset(), prepared.prefix())
        };
        // Only a genuine rollover may warm future pages. The initial packet
        // leaves startup allocation unchanged, and current-page edits do not
        // repeatedly write or round an already initialized suffix.
        let future_end = if close.is_some() && page_end > self.stats.initialized_end_bytes {
            page_end
                .div_ceil(PACKET_WINDOW_BYTES)
                .checked_mul(PACKET_WINDOW_BYTES)
                .ok_or(Error::LimitExceeded)?
                .min(self.capacity)
        } else {
            page_end
        };
        let future_len = future_end
            .checked_sub(page_end)
            .ok_or(Error::LimitExceeded)?;
        let gap = PACKET_ZEROS
            .get(..prepared.gap_len())
            .ok_or(Error::LimitExceeded)?;
        let future = PACKET_ZEROS.get(..future_len).ok_or(Error::LimitExceeded)?;
        let submitted = prefix
            .len()
            .checked_add(prepared.body().len())
            .and_then(|n| n.checked_add(gap.len()))
            .and_then(|n| n.checked_add(pages::FOOTER_BYTES))
            .and_then(|n| n.checked_add(future.len()))
            .ok_or(Error::LimitExceeded)?;
        if offset.checked_add(submitted) != Some(future_end) {
            return Err(Error::Corrupt("snapshot packet write geometry"));
        }
        let bank = &self.banks[self.active];
        let absolute = bank
            .offset
            .checked_add(offset as u64)
            .ok_or(Error::LimitExceeded)?;
        bank.offset
            .checked_add(future_end as u64)
            .filter(|end| *end <= libc::off_t::MAX as u64)
            .ok_or(Error::LimitExceeded)?;
        let stored_len = prepared.body().len();
        self.poisoned = true;
        self.stats.sync_calls += 1;
        self.stats.append_bytes += submitted as u64;
        if future_len != 0 {
            self.stats.reservation_calls = self.stats.reservation_calls.saturating_add(1);
            self.stats.reservation_zero_bytes = self
                .stats
                .reservation_zero_bytes
                .saturating_add(future_len as u64);
        }
        if io::write_packet_sync(
            &bank.file,
            absolute,
            &[prefix, prepared.body(), gap, prepared.footer(), future],
        )
        .is_err()
        {
            return Err(Error::OutcomeUnknown);
        }
        self.page = Some(prepared.into_state());
        self.tail = page_end;
        self.logical_tail = logical_end;
        self.sequence = context.sequence;
        self.last_hash = context.hash;
        self.stats.used_bytes = page_end;
        self.stats.logical_used_bytes = logical_end;
        self.stats.initialized_end_bytes = self.stats.initialized_end_bytes.max(future_end);
        self.stats.append_frames += 1;
        self.stats.packed_records += 1;
        self.stats.packed_pages += u64::from(is_new);
        self.stats.closed_pages += u64::from(close.is_some());
        self.record_compression(input_len, stored_len, compressed);
        self.poisoned = false;
        Ok(context.sequence)
    }
    #[inline(never)]
    fn append_bulk_with_close(
        &mut self,
        stored: &[u8],
        extent: usize,
        flush_end: usize,
        logical_end: usize,
        input_len: usize,
        compressed: bool,
    ) -> Result<u64> {
        let sequence = self.sequence.checked_add(1).ok_or(Error::LimitExceeded)?;
        let page = self
            .page
            .as_ref()
            .expect("open page checked before bulk close");
        self.validate_page_context(page)?;
        let close = page.prepare_successor()?;
        let mut prefix = [0; pages::FOOTER_BYTES + COMPACT_TX_HEADER];
        prefix[..pages::FOOTER_BYTES].copy_from_slice(close.footer());
        let header = &mut prefix[pages::FOOTER_BYTES..];
        header[..8].copy_from_slice(if compressed {
            COMPRESSED_TX_MAGIC
        } else {
            COMPACT_TX_MAGIC
        });
        header[8..16].copy_from_slice(&self.generation.to_le_bytes());
        header[16..24].copy_from_slice(&(stored.len() as u64).to_le_bytes());
        let hash = compact_checksum(self.source, sequence, self.last_hash, header, stored);
        let mut suffix = [0; FOOTER + 7 + MARKER];
        suffix[..32].copy_from_slice(&hash);
        suffix[32..40].copy_from_slice(TX_END);
        suffix[40..48].copy_from_slice(&(extent as u64).to_le_bytes());
        let suffix_len = flush_end - self.tail - COMPACT_TX_HEADER - stored.len();
        let submitted = pages::FOOTER_BYTES + flush_end - self.tail;
        if close.offset().checked_add(submitted) != Some(flush_end) {
            return Err(Error::Corrupt("snapshot closed bulk write geometry"));
        }
        let bank = &self.banks[self.active];
        let absolute = bank
            .offset
            .checked_add(close.offset() as u64)
            .ok_or(Error::LimitExceeded)?;
        bank.offset
            .checked_add(flush_end as u64)
            .filter(|end| *end <= libc::off_t::MAX as u64)
            .ok_or(Error::LimitExceeded)?;
        self.poisoned = true;
        self.stats.sync_calls += 1;
        self.stats.append_bytes += submitted as u64;
        if io::write_sync(
            &bank.file,
            absolute,
            &[&prefix, stored, &suffix[..suffix_len]],
        )
        .is_err()
        {
            return Err(Error::OutcomeUnknown);
        }
        self.page = None;
        self.sequence = sequence;
        self.tail += extent;
        self.logical_tail = logical_end;
        self.last_hash = hash;
        self.stats.used_bytes = self.tail;
        self.stats.logical_used_bytes = logical_end;
        self.stats.initialized_end_bytes = self
            .stats
            .initialized_end_bytes
            .max(flush_end.div_ceil(pages::PAGE_BYTES) * pages::PAGE_BYTES)
            .min(self.capacity);
        self.stats.append_frames += 1;
        self.stats.closed_pages += 1;
        self.record_compression(input_len, stored.len(), compressed);
        self.poisoned = false;
        Ok(sequence)
    }
    fn record_compression(&mut self, input: usize, stored: usize, compressed: bool) {
        if compressed {
            self.stats.compressed_frames = self.stats.compressed_frames.saturating_add(1);
            self.stats.compressed_input_bytes = self
                .stats
                .compressed_input_bytes
                .saturating_add(input as u64);
            self.stats.compressed_stored_bytes = self
                .stats
                .compressed_stored_bytes
                .saturating_add(stored as u64);
        }
    }

    /// Publish a complete selected logical state without resetting sequence.
    /// The caller must encode exactly all effects through sequence().
    pub fn checkpoint(&mut self, payload: &[u8]) -> Result<()> {
        self.check()?;
        if record_extent(CP_HEADER, payload.len())? > self.capacity {
            return Err(Error::LimitExceeded);
        }
        let generation = self.generation.checked_add(1).ok_or(Error::LimitExceeded)?;
        let inactive = 1 - self.active;
        self.poisoned = true;
        let (end, hash, flush_end) = write_checkpoint(
            &mut self.banks[inactive].map,
            self.source,
            generation,
            self.sequence,
            self.generation,
            self.last_hash,
            payload,
        )?;
        self.stats.sync_calls += 1;
        self.stats.checkpoint_bytes += flush_end as u64;
        if self.banks[inactive].file.sync_data().is_err() {
            return Err(Error::OutcomeUnknown);
        }
        self.active = inactive;
        self.generation = generation;
        self.tail = end;
        self.logical_tail = end;
        self.page = None;
        self.last_hash = hash;
        self.stats.used_bytes = end;
        self.stats.logical_used_bytes = end;
        if self.format == FrameFormat::Paged {
            self.stats.initialized_end_bytes =
                flush_end.div_ceil(pages::PAGE_BYTES) * pages::PAGE_BYTES;
        }
        self.stats.checkpoints += 1;
        self.poisoned = false;
        Ok(())
    }

    fn replay(&mut self, out: &mut Vec<Vec<u8>>) -> Result<bool> {
        let map = &self.banks[self.active].map;
        let header = self.format.header();
        loop {
            if self.tail == self.capacity {
                return Ok(false);
            }
            let remaining = self.capacity - self.tail;
            self.stats.replay_bytes += MARKER.min(remaining) as u64;
            if remaining < MARKER {
                return Ok(true);
            }
            let start = self.tail;
            if map[start..start + MARKER] == [0; MARKER] {
                return Ok(false);
            }
            let compressed =
                self.format.is_compact() && map[start..start + 8] == *COMPRESSED_TX_MAGIC;
            if map[start..start + 8] != *self.format.magic() && !compressed {
                // Classify recognizable foreign codecs BEFORE applying this
                // codec's minimum size. A complete72-byte v3 frame can fit in
                // a v2 tail too short for v2's128-byte minimum.
                let complete =
                    complete_footer(map, start, header, self.format.length_offset(), TX_END)
                        || (map[start..start + 8] == *TX_MAGIC
                            && complete_footer(map, start, TX_HEADER, 40, TX_END))
                        || ((map[start..start + 8] == *COMPACT_TX_MAGIC
                            || map[start..start + 8] == *COMPRESSED_TX_MAGIC)
                            && complete_footer(map, start, COMPACT_TX_HEADER, 16, TX_END));
                return if complete {
                    Err(Error::Corrupt("snapshot log complete frame magic"))
                } else {
                    Ok(true)
                };
            }
            if remaining < header + FOOTER {
                return Ok(true);
            }
            self.stats.replay_bytes += (header - MARKER) as u64;
            let generation = match self.format {
                FrameFormat::Original => {
                    if map[start + 8..start + 24] != self.source {
                        return Err(Error::Corrupt("snapshot log frame source"));
                    }
                    read_u64(map, start + 24)
                }
                FrameFormat::Compact | FrameFormat::Paged => read_u64(map, start + 8),
            };
            // A previous checkpoint generation's suffix is unselected in both
            // codecs. Never reinterpret a codec after selecting its file header.
            if generation != self.generation {
                return Ok(true);
            }
            let Ok(length) = usize::try_from(read_u64(map, start + self.format.length_offset()))
            else {
                return Ok(true);
            };
            let Ok(extent) = record_extent(header, length) else {
                return Ok(true);
            };
            if extent > remaining {
                return Ok(true);
            }
            let footer = start + header + length;
            self.stats.replay_bytes += (extent - header) as u64;
            if map[footer + 32..footer + 40] != *TX_END {
                return Ok(true);
            }
            if read_u64(map, footer + 40) != extent as u64
                || map[footer + FOOTER..start + extent].iter().any(|&b| b != 0)
            {
                return Err(Error::Corrupt("snapshot log complete frame footer"));
            }
            let sequence = self
                .sequence
                .checked_add(1)
                .ok_or(Error::Corrupt("snapshot log frame sequence/chain"))?;
            let hash = frame_checksum(
                self.format,
                self.source,
                sequence,
                self.last_hash,
                &map[start..footer],
            );
            if map[footer..footer + 32] != hash {
                return Err(Error::Corrupt("snapshot log complete frame checksum"));
            }
            if self.format == FrameFormat::Original
                && (read_u64(map, start + 32) != sequence
                    || map[start + 48..start + 80] != self.last_hash)
            {
                return Err(Error::Corrupt("snapshot log frame sequence/chain"));
            }
            let stored = &map[start + header..footer];
            // Hash validation precedes even the decoded length. The aggregate
            // uncompressed bound precedes allocation, not merely each record's
            // local2KiB decoder limit.
            let decoded_length = if compressed {
                codec::decoded_length(stored)?
            } else {
                stored.len()
            };
            let logical_extent = record_extent(header, decoded_length)?;
            let logical_end = self
                .logical_tail
                .checked_add(logical_extent)
                .filter(|end| *end <= self.capacity)
                .ok_or(Error::LimitExceeded)?;
            out.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
            let decoded = if compressed {
                codec::decode(stored)?
            } else {
                copy_payload(stored)?
            };
            out.push(decoded);
            self.logical_tail = logical_end;
            self.sequence = sequence;
            self.last_hash = hash;
            self.tail += extent;
            self.stats.append_frames += 1;
        }
    }
}

fn copy_payload(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| Error::LimitExceeded)?;
    out.extend_from_slice(bytes);
    Ok(out)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    // Retained format03 fixtures keep testing their original exact bytes and
    // recovery policy even when new persistent environments select format04.
    fn create_compat(directory: &Path, capacity: usize, initial: &[u8]) -> Result<Journal> {
        Journal::create_inner(directory, capacity, initial, false)
    }
    fn paged_fixture(capacity: usize, initial: &[u8]) -> (std::path::PathBuf, Journal) {
        // The checkout-backed fixture exercises the real persistent transport,
        // independent of whether the host places its system tempdir on tmpfs.
        let dir = std::env::current_dir().unwrap().join(format!(
            "ulmdb-paged-test-{:032x}",
            crate::new_identity().unwrap(),
        ));
        let journal = Journal::create(&dir, capacity, initial).unwrap();
        assert_eq!(journal.format, FrameFormat::Paged);
        (dir, journal)
    }
    #[test]
    fn paged_real_create_mixes_catalog_small_bulk_and_checkpoint_without_setup_reservation() {
        if ulmdb_platform::page_size().unwrap() != pages::PAGE_BYTES {
            return;
        }
        let capacity = 64 << 10;
        let (dir, mut journal) = paged_fixture(capacity, b"base");
        assert_eq!(&image(&dir)[..8], FILE_MAGIC_V4);
        assert_eq!(journal.stats().format_version, 4);
        let start = journal.tail;
        journal.append_transaction(b"catalog", false).unwrap();
        assert_eq!(
            &journal.banks[journal.active].map[start..start + 8],
            COMPACT_TX_MAGIC
        );
        assert!(journal.page.is_none());
        journal.append(b"small").unwrap();
        assert!(journal.page.is_some());
        assert_eq!(journal.stats().packed_records, 1);
        assert_eq!(journal.stats().reservation_calls, 0);
        let bulk = vec![19; WRITE_CODEC_MAX_INPUT + 1];
        journal.append(&bulk).unwrap();
        assert!(journal.page.is_none());
        assert_eq!(journal.stats().closed_pages, 1);
        journal.append(b"next").unwrap();
        assert_eq!(journal.stats().packed_pages, 2);
        assert_eq!(journal.stats().reservation_calls, 0);
        let before = image(&dir);
        drop(journal);
        let (mut journal, base, frames) = Journal::open(&dir, capacity).unwrap();
        assert_eq!(base, b"base");
        assert_eq!(
            frames,
            vec![
                b"catalog".to_vec(),
                b"small".to_vec(),
                bulk,
                b"next".to_vec()
            ]
        );
        assert_eq!(journal.sequence(), 4);
        assert!(journal.page.is_some());
        assert_eq!(journal.stats().tail_repairs, 0);
        assert_eq!(image(&dir), before);
        journal.checkpoint(b"complete-state").unwrap();
        assert!(journal.page.is_none());
        assert_eq!(journal.sequence(), 4);
        journal.append(b"after").unwrap();
        drop(journal);
        let (journal, base, frames) = Journal::open(&dir, capacity).unwrap();
        assert_eq!(base, b"complete-state");
        assert_eq!(frames, vec![b"after".to_vec()]);
        assert_eq!(journal.sequence(), 5);
        drop(journal);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn paged_reopen_resumes_actual_certificate_slot_and_original_base_context() {
        if ulmdb_platform::page_size().unwrap() != pages::PAGE_BYTES {
            return;
        }
        let (dir, mut journal) = paged_fixture(64 << 10, b"base");
        journal.append(b"one").unwrap();
        journal.append(b"two").unwrap();
        let page = journal.page.as_ref().unwrap();
        assert_eq!(page.active_slot(), 1);
        let context = page.context();
        let end = page.end();
        let footer = *page.footer();
        let map = &mut journal.banks[journal.active].map;
        map[end - pages::FOOTER_BYTES..end - pages::CERT_BYTES]
            .copy_from_slice(&footer[pages::CERT_BYTES..]);
        map[end - pages::CERT_BYTES..end].copy_from_slice(&footer[..pages::CERT_BYTES]);
        journal.banks[journal.active].file.sync_data().unwrap();
        drop(journal);
        let (mut journal, _, frames) = Journal::open(&dir, 64 << 10).unwrap();
        assert_eq!(frames, vec![b"one".to_vec(), b"two".to_vec()]);
        let page = journal.page.as_ref().unwrap();
        assert_eq!(page.active_slot(), 0);
        assert_eq!(page.context(), context);
        let preserved = page.footer()[..pages::CERT_BYTES].to_vec();
        journal.append(b"three").unwrap();
        let page = journal.page.as_ref().unwrap();
        assert_eq!(page.active_slot(), 1);
        assert_eq!(&page.footer()[..pages::CERT_BYTES], preserved.as_slice());
        assert_eq!(journal.tail, end);
        drop(journal);
        let (journal, _, frames) = Journal::open(&dir, 64 << 10).unwrap();
        assert_eq!(
            frames,
            vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()]
        );
        drop(journal);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn paged_rollover_cofences_close_and_child_and_only_then_reserves_future_pages() {
        if ulmdb_platform::page_size().unwrap() != pages::PAGE_BYTES {
            return;
        }
        let (dir, mut journal) = paged_fixture(64 << 10, b"base");
        let payload: Vec<u8> = (0..WRITE_PAGE_MAX_BODY).map(|n| n as u8).collect();
        journal.append(&payload).unwrap();
        assert_eq!(journal.stats().reservation_calls, 0);
        while journal.page.as_ref().unwrap().available_body().unwrap_or(0) >= payload.len() {
            journal.append(&payload).unwrap();
        }
        let parent = journal.page.as_ref().unwrap().clone();
        let count = journal.sequence();
        let before_stats = journal.stats();
        let before_extent = journal.banks[journal.active].file.metadata().unwrap().len();
        journal.append(&payload).unwrap();
        assert_eq!(journal.sequence(), count + 1);
        assert_eq!(journal.stats().packed_pages, 2);
        assert_eq!(journal.stats().closed_pages, 1);
        assert_eq!(journal.stats().sync_calls, before_stats.sync_calls + 1);
        assert_eq!(journal.stats().reservation_calls, 1);
        let child_end = journal.page.as_ref().unwrap().end();
        let future_end = child_end.div_ceil(PACKET_WINDOW_BYTES) * PACKET_WINDOW_BYTES;
        assert_eq!(
            journal.stats().reservation_zero_bytes,
            (future_end - child_end) as u64
        );
        assert_eq!(
            journal.stats().append_bytes - before_stats.append_bytes,
            (future_end - (parent.end() - pages::FOOTER_BYTES)) as u64
        );
        assert_eq!(
            journal.banks[journal.active].file.metadata().unwrap().len(),
            before_extent
        );
        let map = &journal.banks[journal.active].map;
        let at = parent.end() - pages::FOOTER_BYTES + parent.active_slot() * pages::CERT_BYTES;
        assert_eq!(
            &map[at..at + pages::CERT_BYTES],
            &parent.footer()[parent.active_slot() * pages::CERT_BYTES
                ..(parent.active_slot() + 1) * pages::CERT_BYTES]
        );
        let selected = pages::recover(
            map,
            parent.start(),
            pages::Context {
                source: journal.source,
                generation: journal.generation,
                sequence: 0,
                hash: checksum(&map[..CP_HEADER + b"base".len()]),
            },
        )
        .unwrap()
        .unwrap();
        assert!(selected.state().is_closed());
        let total = journal.sequence();
        drop(journal);
        let (journal, _, frames) = Journal::open(&dir, 64 << 10).unwrap();
        assert_eq!(frames.len() as u64, total);
        assert!(frames.iter().all(|frame| frame == &payload));
        assert_eq!(journal.stats().tail_repairs, 0);
        drop(journal);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn paged_required_bulk_child_corruption_never_repairs_or_exposes_a_prefix() {
        if ulmdb_platform::page_size().unwrap() != pages::PAGE_BYTES {
            return;
        }
        for wrong in 0..3 {
            let (dir, mut journal) = paged_fixture(64 << 10, b"base");
            journal.append(b"one").unwrap();
            let child = journal.tail;
            journal
                .append(&vec![23; WRITE_CODEC_MAX_INPUT + 1])
                .unwrap();
            let map = &mut journal.banks[journal.active].map;
            match wrong {
                0 => map[child..child + 8].fill(0),
                1 => map[child + 8] ^= 1,
                _ => map[child + COMPACT_TX_HEADER] ^= 1,
            }
            journal.banks[journal.active].file.sync_data().unwrap();
            drop(journal);
            let before = image(&dir);
            assert!(matches!(
                Journal::open(&dir, 64 << 10),
                Err(Error::Corrupt(_))
            ));
            assert_eq!(before, image(&dir));
            fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn paged_open_terminal_ignores_even_a_complete_valid_unreferenced_bulk_child() {
        if ulmdb_platform::page_size().unwrap() != pages::PAGE_BYTES {
            return;
        }
        let (dir, mut journal) = paged_fixture(64 << 10, b"base");
        journal.append(b"one").unwrap();
        let child = journal.tail;
        let payload = b"unreferenced";
        let extent = record_extent(COMPACT_TX_HEADER, payload.len()).unwrap();
        let map = &mut journal.banks[journal.active].map;
        map[child..child + extent + MARKER].fill(0);
        map[child..child + 8].copy_from_slice(COMPACT_TX_MAGIC);
        map[child + 8..child + 16].copy_from_slice(&journal.generation.to_le_bytes());
        map[child + 16..child + 24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        map[child + COMPACT_TX_HEADER..child + COMPACT_TX_HEADER + payload.len()]
            .copy_from_slice(payload);
        let footer = child + COMPACT_TX_HEADER + payload.len();
        let hash = frame_checksum(
            FrameFormat::Compact,
            journal.source,
            journal.sequence + 1,
            journal.last_hash,
            &map[child..footer],
        );
        map[footer..footer + 32].copy_from_slice(&hash);
        map[footer + 32..footer + 40].copy_from_slice(TX_END);
        map[footer + 40..footer + FOOTER].copy_from_slice(&(extent as u64).to_le_bytes());
        journal.banks[journal.active].file.sync_data().unwrap();
        drop(journal);
        let before = image(&dir);
        let (journal, _, frames) = Journal::open(&dir, 64 << 10).unwrap();
        assert_eq!(frames, vec![b"one".to_vec()]);
        assert_eq!(journal.sequence(), 1);
        assert_eq!(journal.tail, child);
        assert_eq!(journal.stats().tail_repairs, 0);
        assert_eq!(before, image(&dir));
        drop(journal);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn paged_metadata_hint_and_logical_budget_are_independent_of_free_page_slots() {
        if ulmdb_platform::page_size().unwrap() != pages::PAGE_BYTES {
            return;
        }
        let (dir, mut journal) = paged_fixture(pages::PAGE_BYTES, b"base");
        journal.append(b"one").unwrap();
        assert_eq!(journal.tail, journal.capacity);
        assert!(journal.remaining_transaction(true) > 0);
        assert_eq!(journal.remaining_transaction(false), 0);
        journal.append(b"two").unwrap();
        let before = image(&dir);
        assert!(matches!(
            journal.append_transaction(b"catalog", false),
            Err(Error::LimitExceeded)
        ));
        assert!(!journal.is_poisoned());
        assert_eq!(journal.sequence(), 2);
        assert_eq!(image(&dir), before);
        journal.checkpoint(b"two rows").unwrap();
        journal.append_transaction(b"catalog", false).unwrap();
        journal.append(&[]).unwrap();
        loop {
            let before = image(&dir);
            let sequence = journal.sequence();
            match journal.append(&[]) {
                Ok(_) => assert!(journal.logical_tail <= journal.capacity),
                Err(Error::LimitExceeded) => {
                    assert_eq!(journal.sequence(), sequence);
                    assert_eq!(image(&dir), before);
                    assert!(!journal.is_poisoned());
                    break;
                }
                Err(error) => panic!("unexpected {error:?}"),
            }
        }
        drop(journal);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn paged_io_failure_keeps_cached_open_state_unpublished_and_poisons_owner() {
        if ulmdb_platform::page_size().unwrap() != pages::PAGE_BYTES {
            return;
        }
        for after_page in [false, true] {
            let (dir, mut journal) = paged_fixture(64 << 10, b"base");
            if after_page {
                journal.append(b"kept").unwrap();
            }
            let before = image(&dir);
            let sequence = journal.sequence();
            let page = journal.page.clone();
            let frontier = journal.stats().initialized_end_bytes;
            let active = journal.active;
            let read_only = File::open(dir.join(FILE_NAME)).unwrap();
            let original = std::mem::replace(&mut journal.banks[active].file, read_only);
            let payload = if after_page {
                vec![41; WRITE_CODEC_MAX_INPUT + 1]
            } else {
                b"new".to_vec()
            };
            assert!(matches!(
                journal.append(&payload),
                Err(Error::OutcomeUnknown)
            ));
            assert!(journal.is_poisoned());
            assert_eq!(journal.sequence(), sequence);
            assert_eq!(journal.page, page);
            assert_eq!(journal.stats().initialized_end_bytes, frontier);
            assert_eq!(image(&dir), before);
            assert!(matches!(
                journal.append(b"retry forbidden"),
                Err(Error::OutcomeUnknown)
            ));
            drop((journal, original));
            let (journal, _, frames) = Journal::open(&dir, 64 << 10).unwrap();
            assert_eq!(
                frames,
                if after_page {
                    vec![b"kept".to_vec()]
                } else {
                    Vec::new()
                }
            );
            drop(journal);
            fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn new_actual_tmpfs_journal_retains_format03_and_ignores_row_hint() {
        let root = Path::new("/dev/shm");
        let Ok(directory_file) = File::open(root) else {
            return;
        };
        if !io::is_ram_backed(&directory_file).unwrap_or(false) {
            return;
        }
        let dir = root.join(format!(
            "ulmdb-ram-journal-{:032x}",
            crate::new_identity().unwrap()
        ));
        let mut journal =
            Journal::create(&dir, 4 * ulmdb_platform::page_size().unwrap(), b"base").unwrap();
        assert_eq!(journal.format, FrameFormat::Compact);
        assert_eq!(journal.stats().format_version, 3);
        assert_eq!(&image(&dir)[..8], FILE_MAGIC);
        let start = journal.tail;
        assert_eq!(
            journal.remaining_transaction(true),
            journal.remaining_transaction(false)
        );
        journal.append_transaction(b"data", true).unwrap();
        assert_eq!(
            &journal.banks[journal.active].map[start..start + 8],
            COMPACT_TX_MAGIC
        );
        assert!(journal.page.is_none());
        assert_eq!(journal.stats().packed_records, 0);
        assert_eq!(journal.stats().reservation_zero_bytes, 0);
        drop(journal);
        fs::remove_dir_all(dir).unwrap();
    }
    fn directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ulmdb-snapshot-log-{:032x}",
            crate::new_identity().unwrap()
        ))
    }
    fn image(directory: &Path) -> Vec<u8> {
        fs::read(directory.join(FILE_NAME)).unwrap()
    }
    fn legacy_fixture(directory: &Path, capacity: usize) {
        fs::create_dir(directory).unwrap();
        let source = crate::new_identity().unwrap().to_le_bytes();
        let file = ulmdb_platform::create_private_file(&directory.join(LOCK_NAME), true).unwrap();
        let mut header = [0; LOCK_SIZE];
        header[..8].copy_from_slice(LOCK_MAGIC);
        header[8..24].copy_from_slice(&source);
        header[24..32].copy_from_slice(&(capacity as u64).to_le_bytes());
        let hash = checksum(&header[..32]);
        header[32..].copy_from_slice(&hash);
        file.write_all_at(&header, 0).unwrap();
        file.sync_data().unwrap();
        for (at, name) in BANK_NAMES.iter().enumerate() {
            let file = ulmdb_platform::create_private_file(&directory.join(name), true).unwrap();
            file.set_len(capacity as u64).unwrap();
            let mut bank = bank(file, capacity, 0, capacity).unwrap();
            if at == 0 {
                write_checkpoint(&mut bank.map, source, 1, 0, 0, [0; 32], b"legacy").unwrap();
            }
            bank.file.sync_data().unwrap();
        }
        ulmdb_platform::sync_directory(directory).unwrap();
    }
    #[test]
    fn complete_frames_rotate_twice_and_keep_global_sequence() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"initial").unwrap();
        assert_eq!(j.append(b"one").unwrap(), 1);
        j.checkpoint(b"state-one").unwrap();
        assert_eq!(j.append(b"two").unwrap(), 2);
        j.checkpoint(b"state-two").unwrap();
        assert_eq!(j.append(b"three").unwrap(), 3);
        let stats = j.stats();
        assert_eq!(stats.checkpoints, 3);
        assert!(stats.checkpoint_bytes < 4096);
        drop(j);
        let (mut j, baseline, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(baseline, b"state-two");
        assert_eq!(frames, vec![b"three".to_vec()]);
        assert_eq!(j.sequence(), 3);
        assert_eq!(j.append(b"four").unwrap(), 4);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn exclusive_owner_and_capacity_refusal_precede_mutation() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"").unwrap();
        assert!(matches!(Journal::open(&dir, 4096), Err(Error::Busy)));
        let before = image(&dir);
        assert!(matches!(
            j.append(&vec![0; 4096]),
            Err(Error::LimitExceeded)
        ));
        assert!(!j.is_poisoned());
        assert_eq!(before, image(&dir));
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn positioned_write_error_poisons_owner_without_acknowledging_a_new_sequence() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        j.append(b"acknowledged").unwrap();
        let before = image(&dir);
        let active = j.active;
        j.mapped_appends = false; // Exercise positioned errors on any test FS.
        // The lease and mapping keep the same owned inode alive. Replace only
        // the append descriptor with a read-only one to force a real EBADF.
        let read_only = File::open(dir.join(FILE_NAME)).unwrap();
        let original = std::mem::replace(&mut j.banks[active].file, read_only);
        assert!(matches!(j.append(b"uncertain"), Err(Error::OutcomeUnknown)));
        assert!(j.is_poisoned());
        assert_eq!(j.sequence(), 1);
        assert_eq!(j.stats().append_frames, 1);
        assert_eq!(before, image(&dir));
        assert!(matches!(
            j.append(b"retry forbidden"),
            Err(Error::OutcomeUnknown)
        ));
        drop((j, original));
        let (j, baseline, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(baseline, b"base");
        assert_eq!(frames, vec![b"acknowledged".to_vec()]);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn mapped_and_positioned_compact_frames_share_one_recovery_chain() {
        let dir = directory();
        let mut j = create_compat(&dir, 8192, b"base").unwrap();
        assert_eq!(
            j.stats().mapped_appends,
            io::is_ram_backed(j._lock.file()).unwrap()
        );
        let raw: Vec<u8> = (0..128).collect();
        j.mapped_appends = false;
        assert_eq!(j.append(&raw).unwrap(), 1);
        j.mapped_appends = true;
        assert_eq!(j.append(&[17; 128]).unwrap(), 2);
        drop(j);
        let (mut j, baseline, frames) = Journal::open(&dir, 8192).unwrap();
        assert_eq!(baseline, b"base");
        assert_eq!(frames, vec![raw.clone(), vec![17; 128]]);
        assert_eq!(
            j.stats().mapped_appends,
            io::is_ram_backed(j._lock.file()).unwrap()
        );
        j.checkpoint(b"selected-through-two").unwrap();
        j.mapped_appends = true;
        assert_eq!(j.append(&raw).unwrap(), 3);
        j.mapped_appends = false;
        assert_eq!(j.append(&[18; 128]).unwrap(), 4);
        drop(j);
        let (j, baseline, frames) = Journal::open(&dir, 8192).unwrap();
        assert_eq!(baseline, b"selected-through-two");
        assert_eq!(frames, vec![raw, vec![18; 128]]);
        assert_eq!(j.sequence(), 4);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn partial_frontier_repairs_only_unselected_marker() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        j.append(b"acknowledged").unwrap();
        let tail = j.tail;
        let active = j.active;
        let prefix = j.banks[active].map[..tail].to_vec();
        j.banks[active].map[tail..tail + 8].copy_from_slice(j.format.magic());
        // Header/payload interrupted before any recognizable complete footer.
        j.banks[active].map[tail + 8..tail + 16].copy_from_slice(&j.generation.to_le_bytes());
        j.banks[active].file.sync_data().unwrap();
        drop(j);
        let (mut j, base, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(base, b"base");
        assert_eq!(frames, vec![b"acknowledged".to_vec()]);
        assert_eq!(&j.banks[active].map[..tail], prefix.as_slice());
        assert_eq!(j.stats().tail_repairs, 1);
        assert_eq!(j.append(b"next").unwrap(), 2);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn complete_bad_checksum_fails_closed_without_tail_repair() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        let start = j.tail;
        j.append(b"payload").unwrap();
        let header = j.format.header();
        j.banks[j.active].map[start + header] ^= 1;
        j.banks[j.active].file.sync_data().unwrap();
        drop(j);
        let before = image(&dir);
        assert!(matches!(
            Journal::open(&dir, 4096),
            Err(Error::Corrupt("snapshot log complete frame checksum"))
        ));
        assert_eq!(before, image(&dir));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn incomplete_inactive_checkpoint_does_not_select_or_erase_old_bank() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        j.append(b"kept").unwrap();
        let active = j.active;
        let old = j.banks[active].map[..].to_vec();
        // Counterfactual cut before the new checkpoint footer/flush completes.
        let inactive = 1 - active;
        j.banks[inactive].map[..8].copy_from_slice(CP_MAGIC);
        j.banks[inactive].map[8..24].copy_from_slice(&j.source);
        j.banks[inactive].file.sync_data().unwrap();
        drop(j);
        let (j, base, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(base, b"base");
        assert_eq!(frames, vec![b"kept".to_vec()]);
        assert_eq!(&j.banks[active].map[..], old.as_slice());
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn single_inode_header_extent_and_mixed_layout_fail_closed() {
        let dir = directory();
        let j = create_compat(&dir, 4096, b"base").unwrap();
        let stats = j.stats();
        assert!(stats.single_file);
        assert_eq!(stats.header_bytes, ulmdb_platform::page_size().unwrap());
        assert_eq!(stats.capacity_bytes, 8192);
        assert_eq!(stats.sync_calls, 3); // owned file, directory and its parent.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(
            j._lock.file().metadata().unwrap().len(),
            (stats.header_bytes + 8192) as u64
        );
        drop(j);
        let original = image(&dir);
        let file = open_owned(&dir.join(FILE_NAME)).unwrap();
        file.set_len((original.len() + 4096) as u64).unwrap();
        file.sync_data().unwrap();
        assert!(matches!(
            Journal::open(&dir, 4096),
            Err(Error::Corrupt("snapshot log single file extent"))
        ));
        file.set_len(original.len() as u64).unwrap();
        file.write_all_at(&[original[8] ^ 1], 8).unwrap();
        file.sync_data().unwrap();
        assert!(matches!(
            Journal::open(&dir, 4096),
            Err(Error::Corrupt("snapshot log single file header"))
        ));
        file.write_all_at(&original, 0).unwrap();
        file.sync_data().unwrap();
        let extra = ulmdb_platform::create_private_file(&dir.join(LOCK_NAME), true).unwrap();
        drop(extra);
        assert!(matches!(
            Journal::open(&dir, 4096),
            Err(Error::Corrupt("snapshot log mixed layouts"))
        ));
        fs::remove_file(dir.join(LOCK_NAME)).unwrap();
        assert_eq!(Journal::open(&dir, 4096).unwrap().1, b"base");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn relative_directory_creation_fences_the_current_parent() {
        let dir = std::path::PathBuf::from(format!(
            "ulmdb-relative-snapshot-log-{:032x}",
            crate::new_identity().unwrap()
        ));
        assert!(dir.parent().unwrap().as_os_str().is_empty());
        let j = create_compat(&dir, 4096, b"relative").unwrap();
        assert_eq!(j.stats().sync_calls, 3); // file, child directory, CWD parent.
        drop(j);
        let (j, baseline, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(baseline, b"relative");
        assert!(frames.is_empty());
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_open_preserves_layout_and_uses_the_same_sync_contract() {
        let dir = directory();
        legacy_fixture(&dir, 4096);
        let before: Vec<_> = [LOCK_NAME, BANK_NAMES[0], BANK_NAMES[1]]
            .map(|name| fs::read(dir.join(name)).unwrap())
            .into();
        assert!(matches!(
            create_compat(&dir, 4096, b"replacement"),
            Err(Error::InvalidInput(
                "snapshot log legacy layout already exists"
            ))
        ));
        for (name, bytes) in [LOCK_NAME, BANK_NAMES[0], BANK_NAMES[1]]
            .iter()
            .zip(&before)
        {
            assert_eq!(&fs::read(dir.join(name)).unwrap(), bytes);
        }
        let (mut j, base, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(base, b"legacy");
        assert!(frames.is_empty());
        assert!(!j.stats().single_file);
        assert_eq!(j.stats().header_bytes, LOCK_SIZE);
        assert_eq!(j.append(b"one").unwrap(), 1);
        j.checkpoint(b"legacy-one").unwrap();
        assert_eq!(j.append(b"two").unwrap(), 2);
        assert_eq!(j.stats().sync_calls, 3);
        drop(j);
        let (j, base, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(base, b"legacy-one");
        assert_eq!(frames, vec![b"two".to_vec()]);
        assert!(!dir.join(FILE_NAME).exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn copied_bank_source_and_complete_checkpoint_corruption_fail_closed() {
        let first = directory();
        let second = directory();
        let mut a = create_compat(&first, 4096, b"first").unwrap();
        let b = create_compat(&second, 4096, b"second").unwrap();
        a.banks[0].map.copy_from_slice(&b.banks[0].map);
        a.banks[0].file.sync_data().unwrap();
        drop((a, b));
        let before = image(&first);
        assert!(matches!(
            Journal::open(&first, 4096),
            Err(Error::Corrupt("snapshot log checkpoint source"))
        ));
        assert_eq!(before, image(&first));
        let (mut b, _, _) = Journal::open(&second, 4096).unwrap();
        b.banks[0].map[CP_HEADER] ^= 1;
        b.banks[0].file.sync_data().unwrap();
        drop(b);
        let before = image(&second);
        assert!(matches!(
            Journal::open(&second, 4096),
            Err(Error::Corrupt("snapshot log complete checkpoint checksum"))
        ));
        assert_eq!(before, image(&second));
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }
    fn single_v2_fixture(directory: &Path, capacity: usize) -> Journal {
        let mut journal = create_compat(directory, capacity, b"v2-base").unwrap();
        let mut header = [0; FILE_HEADER];
        journal._lock.file().read_exact_at(&mut header, 0).unwrap();
        header[..8].copy_from_slice(FILE_MAGIC_V2);
        let hash = checksum(&header[..64]);
        header[64..].copy_from_slice(&hash);
        journal._lock.file().write_all_at(&header, 0).unwrap();
        journal._lock.file().sync_data().unwrap();
        journal.format = FrameFormat::Original;
        journal
    }
    #[test]
    fn compact_v3_is_200_bytes_for_the_exact_128_byte_effect_payload() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        assert_eq!(j.format, FrameFormat::Compact);
        assert_eq!(&image(&dir)[..8], FILE_MAGIC);
        let start = j.tail;
        let payload: [u8; 128] = std::array::from_fn(|at| at as u8);
        assert_eq!(j.append(&payload).unwrap(), 1);
        assert_eq!(j.tail - start, 200);
        assert_eq!(&j.banks[j.active].map[start..start + 8], COMPACT_TX_MAGIC);
        assert_eq!(read_u64(&j.banks[j.active].map, start + 8), 1);
        assert_eq!(read_u64(&j.banks[j.active].map, start + 16), 128);
        assert_eq!(j.stats().append_bytes, 208); // complete frame plus next marker.
        drop(j);
        let (j, baseline, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(baseline, b"base");
        assert_eq!(frames, vec![payload.to_vec()]);
        assert_eq!(j.sequence(), 1);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn v2_single_file_keeps_original_exact_frame_codec_after_append_and_rotation() {
        let dir = directory();
        let mut j = single_v2_fixture(&dir, 4096);
        let start = j.tail;
        let previous = j.last_hash;
        j.append(&[43; 128]).unwrap();
        assert_eq!(j.tail - start, 256);
        let bytes = &j.banks[j.active].map[start..j.tail];
        assert_eq!(&bytes[..8], TX_MAGIC);
        assert_eq!(&bytes[8..24], &j.source);
        assert_eq!(read_u64(bytes, 24), 1);
        assert_eq!(read_u64(bytes, 32), 1);
        assert_eq!(read_u64(bytes, 40), 128);
        assert_eq!(&bytes[48..80], &previous);
        assert_eq!(
            &bytes[80 + 128..80 + 128 + 32],
            &checksum(&bytes[..80 + 128])
        );
        drop(j);
        let (mut j, _, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(j.format, FrameFormat::Original);
        assert_eq!(frames, vec![vec![43; 128]]);
        j.checkpoint(b"v2-state").unwrap();
        assert_eq!(j.append(b"still-original").unwrap(), 2);
        drop(j);
        assert_eq!(&image(&dir)[..8], FILE_MAGIC_V2);
        let (j, baseline, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(j.format, FrameFormat::Original);
        assert_eq!(baseline, b"v2-state");
        assert_eq!(frames, vec![b"still-original".to_vec()]);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn compact_context_rejects_wrong_source_expected_sequence_and_parent() {
        for wrong in 0..3 {
            let dir = directory();
            let mut j = create_compat(&dir, 4096, b"base").unwrap();
            let start = j.tail;
            let previous = j.last_hash;
            j.append(b"context-bound").unwrap();
            let footer = start + COMPACT_TX_HEADER + b"context-bound".len();
            let mut source = j.source;
            let mut parent = previous;
            let mut sequence = 1;
            match wrong {
                0 => source[0] ^= 1,
                1 => sequence = 2,
                _ => parent[0] ^= 1,
            }
            let incorrect = frame_checksum(
                FrameFormat::Compact,
                source,
                sequence,
                parent,
                &j.banks[j.active].map[start..footer],
            );
            j.banks[j.active].map[footer..footer + 32].copy_from_slice(&incorrect);
            j.banks[j.active].file.sync_data().unwrap();
            drop(j);
            let before = image(&dir);
            assert!(matches!(
                Journal::open(&dir, 4096),
                Err(Error::Corrupt("snapshot log complete frame checksum"))
            ));
            assert_eq!(before, image(&dir));
            fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn compact_reordered_frames_bad_footer_and_other_codec_fail_without_repair() {
        for wrong in 0..3 {
            let dir = directory();
            let mut j = create_compat(&dir, 4096, b"base").unwrap();
            let start = j.tail;
            j.append(b"first").unwrap();
            let second = j.tail;
            j.append(b"other").unwrap();
            let end = j.tail;
            match wrong {
                0 => {
                    let first = j.banks[j.active].map[start..second].to_vec();
                    let other = j.banks[j.active].map[second..end].to_vec();
                    j.banks[j.active].map[start..second].copy_from_slice(&other);
                    j.banks[j.active].map[second..end].copy_from_slice(&first);
                }
                1 => {
                    let footer = start + COMPACT_TX_HEADER + b"first".len();
                    j.banks[j.active].map[footer + 40..footer + 48]
                        .copy_from_slice(&999u64.to_le_bytes());
                }
                _ => {
                    // Resealing a file header as v2 cannot reinterpret the
                    // complete v3 record as an incomplete repairable frontier.
                    let mut header = [0; FILE_HEADER];
                    j._lock.file().read_exact_at(&mut header, 0).unwrap();
                    header[..8].copy_from_slice(FILE_MAGIC_V2);
                    let hash = checksum(&header[..64]);
                    header[64..].copy_from_slice(&hash);
                    j._lock.file().write_all_at(&header, 0).unwrap();
                }
            }
            j.banks[j.active].file.sync_data().unwrap();
            drop(j);
            let before = image(&dir);
            assert!(matches!(Journal::open(&dir, 4096), Err(Error::Corrupt(_))));
            assert_eq!(before, image(&dir));
            fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn compact_previous_generation_suffix_is_unselected_after_checkpoint() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        let old_start = j.tail;
        j.append(b"already-in-checkpoint").unwrap();
        let old = j.banks[j.active].map[old_start..j.tail].to_vec();
        j.checkpoint(b"selected-state").unwrap();
        let active = j.active;
        let tail = j.tail;
        j.banks[active].map[tail..tail + old.len()].copy_from_slice(&old);
        j.banks[active].file.sync_data().unwrap();
        drop(j);
        let (mut j, baseline, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(baseline, b"selected-state");
        assert!(frames.is_empty());
        assert_eq!(j.stats().tail_repairs, 1);
        assert_eq!(j.sequence(), 1);
        assert_eq!(j.append(b"new-generation").unwrap(), 2);
        drop(j);
        let (j, _, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(frames, vec![b"new-generation".to_vec()]);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn compact_short_runs_compress_but_return_original_bytes_and_logical_budget() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        let start = j.tail;
        let logical = j.logical_tail;
        let payload = [7; 128];
        j.append(&payload).unwrap();
        assert_eq!(
            &j.banks[j.active].map[start..start + 8],
            COMPRESSED_TX_MAGIC
        );
        assert!(j.tail - start < 200);
        assert_eq!(j.stats().codec_attempts, 1);
        assert_eq!(j.stats().compressed_frames, 1);
        assert_eq!(j.stats().compressed_input_bytes, 128);
        assert!(j.stats().compressed_stored_bytes <= 96);
        assert_eq!(j.stats().logical_used_bytes, j.logical_tail);
        assert_eq!(j.logical_tail - logical, 200);
        assert_eq!(
            j.remaining(),
            (4096 - j.logical_tail).saturating_sub(COMPACT_TX_HEADER + FOOTER + MARKER + 7)
        );
        drop(j);
        let (mut j, _, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(frames, vec![payload.to_vec()]);
        assert_eq!(j.logical_tail - logical, 200);
        let small = j.tail;
        j.append(&[8; codec::MIN_INPUT - 1]).unwrap();
        assert_eq!(&j.banks[j.active].map[small..small + 8], COMPACT_TX_MAGIC);
        j.checkpoint(b"rotated").unwrap();
        assert_eq!(j.logical_tail, j.tail);
        j.append(&payload).unwrap();
        drop(j);
        let (j, baseline, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(baseline, b"rotated");
        assert_eq!(frames, vec![payload.to_vec()]);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn compression_never_extends_total_uncompressed_replay_or_append_capacity() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        let mut count = 0;
        while j.logical_tail + record_extent(COMPACT_TX_HEADER, 128).unwrap() <= 4096 {
            j.append(&[9; 128]).unwrap();
            count += 1;
        }
        assert!(j.tail < j.logical_tail);
        let before = image(&dir);
        assert!(matches!(j.append(&[9; 128]), Err(Error::LimitExceeded)));
        assert!(!j.is_poisoned());
        assert_eq!(before, image(&dir));
        drop(j);
        let (j, _, frames) = Journal::open(&dir, 4096).unwrap();
        assert_eq!(frames.len(), count);
        assert!(frames.iter().all(|frame| frame == &[9; 128]));
        assert!(frames.iter().map(Vec::len).sum::<usize>() <= 4096);
        drop(j);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn complete_compressed_replay_expansion_over_bank_limit_fails_without_repair() {
        let dir = directory();
        let mut j = create_compat(&dir, 4096, b"base").unwrap();
        let start = j.tail;
        j.append(&[10; WRITE_CODEC_MAX_INPUT]).unwrap();
        let end = j.tail;
        let mut previous = j.last_hash;
        let mut second = j.banks[j.active].map[start..end].to_vec();
        let stored = read_u64(&second, 16) as usize;
        let footer = COMPACT_TX_HEADER + stored;
        let mut at = end;
        // Four1024-byte records fit physically after compression, but their
        // uncompressed frame extents exceed the4096-byte replay budget.
        for sequence in 2..=4 {
            let hash = frame_checksum(
                FrameFormat::Compact,
                j.source,
                sequence,
                previous,
                &second[..footer],
            );
            second[footer..footer + 32].copy_from_slice(&hash);
            j.banks[j.active].map[at..at + second.len()].copy_from_slice(&second);
            at += second.len();
            previous = hash;
        }
        j.banks[j.active].map[at..at + MARKER].fill(0);
        j.banks[j.active].file.sync_data().unwrap();
        drop(j);
        let before = image(&dir);
        assert!(matches!(
            Journal::open(&dir, 4096),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(before, image(&dir));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn complete_foreign_v3_frame_in_short_v2_tail_is_corrupt_not_repaired() {
        let dir = directory();
        let mut j = single_v2_fixture(&dir, 4096);
        let padding = 4096 - 120 - j.tail - TX_HEADER - FOOTER;
        j.append(&vec![17; padding]).unwrap();
        assert_eq!(4096 - j.tail, 120);
        let start = j.tail;
        let extent = record_extent(COMPACT_TX_HEADER, 0).unwrap();
        let map = &mut j.banks[j.active].map;
        map[start..start + 8].copy_from_slice(COMPACT_TX_MAGIC);
        map[start + 8..start + 16].copy_from_slice(&j.generation.to_le_bytes());
        map[start + 16..start + 24].copy_from_slice(&0u64.to_le_bytes());
        let footer = start + COMPACT_TX_HEADER;
        let hash = frame_checksum(
            FrameFormat::Compact,
            j.source,
            2,
            j.last_hash,
            &map[start..footer],
        );
        map[footer..footer + 32].copy_from_slice(&hash);
        map[footer + 32..footer + 40].copy_from_slice(TX_END);
        map[footer + 40..footer + 48].copy_from_slice(&(extent as u64).to_le_bytes());
        map[start + extent..start + extent + MARKER].fill(0);
        j.banks[j.active].file.sync_data().unwrap();
        drop(j);
        let before = image(&dir);
        assert!(matches!(
            Journal::open(&dir, 4096),
            Err(Error::Corrupt("snapshot log complete frame magic"))
        ));
        assert_eq!(before, image(&dir));
        fs::remove_dir_all(dir).unwrap();
    }
}
