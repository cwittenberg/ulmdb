//! Typed immutable roots and atomic current/predecessor publication.
use crate::{Error, Manifest, Result, cohort, manifest::number};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub(crate) mod copy;
mod policy;
pub use copy::{BootstrapPairTarget, PreparedBootstrapPair};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) use copy::{
    capture_copy_roots_expected_with_transform, read_copy_current_checkpoint_with_transform,
};
pub use policy::{
    ExternalReadyPublication, FileDurabilityState, ReadyPreviousPublication, ReadyPublication,
    ReadyRollback, prepare_publication_with_durability, prepare_publication_with_sealed_data,
    prepare_rollback_with_durability,
};
const BUNDLE_BYTES: usize = 144;
const MAX_CURRENT_BYTES: usize = policy::WEAK_BYTES;
const BUNDLE_MAGIC: &[u8; 8] = b"ULMDBPUB";
pub(crate) fn crc(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0x82f63b78u32.wrapping_mul(c & 1));
        }
    }
    !c
}
pub(crate) fn pointer(generation: u128, hash: [u8; 32]) -> [u8; 64] {
    let mut p = [0; 64];
    p[..8].copy_from_slice(b"ULMDBCUR");
    p[10..12].copy_from_slice(&64u16.to_le_bytes());
    p[12..28].copy_from_slice(&generation.to_le_bytes());
    p[28..60].copy_from_slice(&hash);
    let checksum = crc(&p[..60]);
    p[60..64].copy_from_slice(&checksum.to_le_bytes());
    p
}
pub(crate) fn decode_pointer(p: &[u8]) -> Result<(u128, [u8; 32])> {
    if p.len() != 64
        || !matches!(&p[..8], b"ULMDBCUR" | b"ULMDBC03" | b"ULMDBT04")
        || p[8..10] != [0; 2]
        || p[10..12] != 64u16.to_le_bytes()
        || u32::from_le_bytes(number(p, 60)?) != crc(&p[..60])
    {
        return Err(Error::Corrupt("invalid CURRENT checksum/header"));
    }
    let generation = u128::from_le_bytes(number(p, 12)?);
    if generation == 0 {
        return Err(Error::Corrupt("null checkpoint generation"));
    }
    Ok((generation, number(p, 28)?))
}

/// Validate one typed checkpoint record without opening storage.
pub fn checkpoint_identity(bytes: &[u8]) -> Result<(u128, [u8; 32])> {
    decode_pointer(bytes)
}
fn is_cohort(record: &[u8]) -> bool {
    matches!(record.get(..8), Some(b"ULMDBC03" | b"ULMDBT04"))
}
pub(crate) fn record(manifest: &Manifest, hash: [u8; 32]) -> [u8; 64] {
    let mut bytes = pointer(manifest.generation, hash);
    if manifest.format_minor >= 3 {
        bytes[..8].copy_from_slice(if manifest.format_minor == 4 {
            b"ULMDBT04"
        } else {
            b"ULMDBC03"
        });
        let checksum = crc(&bytes[..60]);
        bytes[60..].copy_from_slice(&checksum.to_le_bytes());
    }
    bytes
}
struct Current {
    current: [u8; 64],
    bundled: bool,
    previous: Option<[u8; 64]>,
    weak: bool,
    data_debt: bool,
}
fn bundle(current: [u8; 64], previous: Option<[u8; 64]>) -> [u8; BUNDLE_BYTES] {
    let mut bytes = [0; BUNDLE_BYTES];
    bytes[..8].copy_from_slice(BUNDLE_MAGIC);
    bytes[10..12].copy_from_slice(&(BUNDLE_BYTES as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&u32::from(previous.is_some()).to_le_bytes());
    bytes[16..80].copy_from_slice(&current);
    if let Some(previous) = previous {
        bytes[80..].copy_from_slice(&previous);
    }
    bytes
}
fn decode_current(bytes: &[u8]) -> Result<Current> {
    if bytes.starts_with(b"ULMDBWK1") {
        return policy::decode_weak(bytes);
    }
    if bytes.len() == 64 {
        decode_pointer(bytes)?;
        if is_cohort(bytes) {
            return Err(Error::Corrupt("cohort CURRENT requires complete bundle"));
        }
        return Ok(Current {
            current: bytes
                .try_into()
                .map_err(|_| Error::Corrupt("short pointer"))?,
            bundled: false,
            previous: None,
            weak: false,
            data_debt: false,
        });
    }
    if bytes.len() != BUNDLE_BYTES
        || &bytes[..8] != BUNDLE_MAGIC
        || bytes[8..10] != [0; 2]
        || bytes[10..12] != (BUNDLE_BYTES as u16).to_le_bytes()
    {
        return Err(Error::Corrupt("invalid CURRENT bundle header/length"));
    }
    let flags = u32::from_le_bytes(number(bytes, 12)?);
    let current: [u8; 64] = number(bytes, 16)?;
    decode_pointer(&current)?;
    if !is_cohort(&current) {
        return Err(Error::Corrupt("bundle requires cohort current record"));
    }
    let previous: [u8; 64] = number(bytes, 80)?;
    let previous = match flags {
        0 if previous == [0; 64] => None,
        1 => {
            decode_pointer(&previous)?;
            Some(previous)
        }
        _ => return Err(Error::Corrupt("invalid CURRENT predecessor presence")),
    };
    Ok(Current {
        current,
        bundled: true,
        previous,
        weak: false,
        data_debt: false,
    })
}

/// Checked contents of one bounded CURRENT selector image.
/// This observation grants no namespace, descriptor, graph, or physical-pin authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectorObservation {
    checkpoint: [u8; 64],
    weak: bool,
    data_debt: bool,
}
impl SelectorObservation {
    pub fn checkpoint(&self) -> &[u8; 64] {
        &self.checkpoint
    }
    pub fn is_weak(&self) -> bool {
        self.weak
    }
    pub fn has_data_debt(&self) -> bool {
        self.data_debt
    }
}

/// Decode an already captured Legacy, bundled, or weak CURRENT selector.
/// The complete image, including predecessor presence and checksums, is validated.
pub fn observe_selector(bytes: &[u8]) -> Result<SelectorObservation> {
    let current = decode_current(bytes)?;
    Ok(SelectorObservation {
        checkpoint: current.current,
        weak: current.weak,
        data_debt: current.data_debt,
    })
}
fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(maximum + 1);
    ulmdb_core::work::checkpoint(1, 0)?;
    crate::work::read_to_end_limited(&mut File::open(path)?.take((maximum + 1) as u64), &mut bytes, (maximum + 1) as u64)?;
    if bytes.len() > maximum {
        return Err(Error::Corrupt("oversized publication pointer"));
    }
    Ok(bytes)
}
// The explicit sync API must not wait for a writer on a FIFO masquerading as
// selection metadata. Keep ordinary reader contracts unchanged: only sync uses
// this no-follow, nonblocking, checked-regular pointer reader.
fn read_sync_pointer(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let mode = crate::read_mode::ReadMode::Sync;
    let mut file = mode.open(path)?;
    let mut bytes = Vec::with_capacity(maximum + 1);
    crate::work::read_to_end_limited(&mut (&mut file).take((maximum + 1) as u64), &mut bytes, (maximum + 1) as u64)?;
    if bytes.len() > maximum {
        return Err(Error::Corrupt("oversized publication pointer"));
    }
    mode.check(path, &file)?;
    Ok(bytes)
}
pub(crate) fn selected_checkpoint_only(directory: &Path) -> Result<[u8; 64]> {
    Ok(sync_current(directory)?.current)
}
fn sync_current(directory: &Path) -> Result<Current> {
    decode_current(&read_sync_pointer(
        &directory.join("CURRENT"),
        MAX_CURRENT_BYTES,
    )?)
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn read_sync_current(directory: &Path) -> Result<Manifest> {
    read_sync_current_with_transform(directory, None)
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn read_sync_current_with_transform(
    directory: &Path,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<Manifest> {
    let state = sync_current(directory)?;
    let manifest = read_checkpoint_inner(
        directory,
        None,
        &state.current,
        crate::read_mode::ReadMode::Sync.with_transform(transform),
    )?;
    check_predecessor(&state, &manifest)?;
    Ok(manifest)
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn read_sync_roots_checked<F>(
    directory: &Path,
    selected_check: F,
) -> Result<(Manifest, Option<Manifest>, bool)>
where
    F: FnOnce(&Manifest) -> Result<()>,
{
    read_sync_roots_checked_with_transform(directory, selected_check, None)
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn read_sync_roots_checked_with_transform<F: FnOnce(&Manifest) -> Result<()>>(
    directory: &Path,
    selected_check: F,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, Option<Manifest>, bool)> {
    let state = sync_current(directory)?;
    let selected = read_checkpoint_inner(
        directory,
        None,
        &state.current,
        crate::read_mode::ReadMode::Sync.with_transform(transform.clone()),
    )?;
    check_predecessor(&state, &selected)?;
    selected_check(&selected)?;
    let previous = if state.bundled {
        state
            .previous
            .map(|p| {
                read_checkpoint_inner(
                    directory,
                    None,
                    &p,
                    crate::read_mode::ReadMode::Sync.with_transform(transform.clone()),
                )
            })
            .transpose()?
    } else if selected.previous == 0 {
        None
    } else {
        match read_sync_pointer(&directory.join("PREVIOUS"), 64) {
            Ok(bytes) => Some(read_checkpoint_inner(
                directory,
                None,
                &bytes,
                crate::read_mode::ReadMode::Sync.with_transform(transform.clone()),
            )?),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        }
    };
    Ok((selected, previous, !state.bundled))
}
fn current(directory: &Path) -> Result<Current> {
    decode_current(&read_bounded(
        &directory.join("CURRENT"),
        MAX_CURRENT_BYTES,
    )?)
}
fn check_predecessor(state: &Current, manifest: &Manifest) -> Result<()> {
    if state.weak && state.previous.is_none() && manifest.previous != 0 {
        return Err(Error::Corrupt("weak selector missing declared predecessor"));
    }
    if let Some(previous) = state.previous
        && decode_pointer(&previous)?.0 != manifest.previous
    {
        return Err(Error::Corrupt("CURRENT predecessor identity mismatch"));
    }
    Ok(())
}
fn replace_policy(
    directory: &Path,
    name: &str,
    bytes: &[u8],
    policy: crate::Durability,
    sync_directory: bool,
) -> Result<()> {
    // Same-directory rename: the final parent sync covers both replacement and
    // removal of the private name. No transactions-directory flush is needed.
    let (temporary, mut f) = crate::temporary::create(directory, name)?;
    crate::fault_point!(format!("pointer.{name}.before_write").as_str());
    f.write_all(bytes)?;
    crate::fault_point!(format!("pointer.{name}.after_write").as_str());
    if policy.sync_pointer_file() {
        crate::fault_point!(format!("pointer.{name}.before_file_sync").as_str());
        ulmdb_platform::sync_file(&f)?;
        crate::fault_point!(format!("pointer.{name}.after_file_sync").as_str());
    }
    crate::fault_point!(format!("pointer.{name}.before_rename").as_str());
    if let Err(error) = ulmdb_platform::rename_replace(&temporary, &directory.join(name)) {
        return Err(if ulmdb_platform::rename_outcome_unknown(&error) {
            Error::OutcomeUnknown
        } else {
            error.into()
        });
    }
    crate::fault_point!(format!("pointer.{name}.after_rename").as_str());
    if sync_directory && policy.sync_pointer_directory() {
        crate::fault_point!(format!("pointer.{name}.before_directory_sync").as_str());
        ulmdb_platform::sync_directory(directory)?;
        crate::fault_point!(format!("pointer.{name}.after_directory_sync").as_str());
    }
    Ok(())
}
fn write_descriptor(directory: &Path, m: &Manifest) -> Result<[u8; 32]> {
    write_descriptor_policy(directory, m, crate::Durability::Full)
}
fn write_descriptor_policy(
    directory: &Path,
    m: &Manifest,
    policy: crate::Durability,
) -> Result<[u8; 32]> {
    cohort::reject_existing_generation(&cohort::path(directory, m.generation))?;
    let bytes = m.encode()?;
    let path = directory
        .join("commits")
        .join(format!("{:032x}.root", m.generation));
    let mut f = crate::access::options_for_entry(&path)?
        .create_new(true)
        .write(true)
        .open(path)?;
    crate::fault_point!("descriptor.before_write");
    if let Some(context) = m.transform_context() {
        context.write_encoded(
            &mut f,
            crate::transform::ObjectKind::Descriptor,
            m.generation,
            &bytes,
        )?;
    } else {
        f.write_all(&bytes)?;
    }
    crate::fault_point!("descriptor.after_write");
    if policy.sync_data() {
        crate::fault_point!("descriptor.before_file_sync");
        ulmdb_platform::sync_file(&f)?;
        crate::fault_point!("descriptor.after_file_sync");
        crate::fault_point!("descriptor.before_directory_sync");
        ulmdb_platform::sync_directory(&directory.join("commits"))?;
        crate::fault_point!("descriptor.after_directory_sync");
    }
    crate::work::digest(&bytes)
}

/// Prepared immutable bytes bound to one directory. Consuming publication does
/// not touch data files. The caller must retain the exclusive writer fence
/// through preparation, publication and activation, preventing orphan reclamation.
/// Failures after invoking publish require reopening.
pub struct PreparedPublication {
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    pub(crate) directory: PathBuf,
    pointer: [u8; 64],
    previous: u128,
    sequence: u64,
    data_policy: crate::Durability,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) epoch: u64,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    directory_identity: crate::coordination::DirectoryIdentity,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) uuid: [u8; 16],
}
impl PreparedPublication {
    /// Check this prepared token's original directory binding without opening
    /// descriptors or revalidating user data. Identical checkpoint bytes in a
    /// copied environment do not transfer publication authority.
    pub fn matches_directory(&self, directory: impl AsRef<Path>) -> Result<bool> {
        let directory = directory.as_ref();
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Ok(false);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            use std::os::unix::fs::MetadataExt;
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let actual = (metadata.dev(), metadata.ino());
            #[cfg(windows)]
            let actual = coordination_directory_identity(directory)?;
            if actual != self.directory_identity {
                return Ok(false);
            }
            self.check_directory_identity(self.directory_identity)?;
            Ok(true)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Ok(directory.canonicalize()? == self.directory.canonicalize()?)
        }
    }
    pub fn publish(self) -> Result<()> {
        self.activate(false)
    }
    /// Select an already-verified bootstrap in a fresh unpublished environment.
    pub fn publish_bootstrap(self) -> Result<()> {
        self.activate(true)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn check_directory_identity(
        &self,
        identity: crate::coordination::DirectoryIdentity,
    ) -> Result<()> {
        let actual = coordination_directory_identity(&self.directory)?;
        if self.directory_identity != identity || actual != self.directory_identity {
            return Err(Error::Corrupt("prepared publication directory binding"));
        }
        Ok(())
    }
    /// Crate-private proof bridge for coordination: only a token produced after
    /// immutable descriptor preparation can admit a pin without cold decoding.
    /// This certifies descriptor bytes/realm, not logical graph correctness.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) fn validated_pin_checkpoint(
        &self,
        directory_identity: crate::coordination::DirectoryIdentity,
        uuid: [u8; 16],
        manifest: &Manifest,
    ) -> Result<[u8; 64]> {
        self.check_directory_identity(directory_identity)?;
        if self.uuid != uuid
            || manifest.uuid != uuid
            || manifest.previous != self.previous
            || manifest.sequence != self.sequence
            || checkpoint_bytes(manifest)? != self.pointer
        {
            return Err(Error::Corrupt("prepared publication manifest binding"));
        }
        Ok(self.pointer)
    }
    fn activate(self, bootstrap: bool) -> Result<()> {
        let ready = self.prepare_ready(None)?;
        if bootstrap {
            ready.publish_bootstrap(crate::Durability::Full).map(|_| ())
        } else {
            ready.publish(crate::Durability::Full).map(|_| ())
        }
    }
}

/// A fresh immutable rollback root whose selection precondition is a different
/// current checkpoint from its deliberately absent logical predecessor.
/// Caller retains the writer fence through preparation, pinning and activation.
pub struct PreparedRollback {
    prepared: PreparedPublication,
    expected_current: [u8; 64],
}
impl PreparedRollback {
    /// Immutable descriptor proof for physical-pin admission before selection.
    pub fn prepared(&self) -> &PreparedPublication {
        &self.prepared
    }
    /// Select only CURRENT. Legacy PREVIOUS may remain stale; the selected
    /// descriptor's previous=0 explicitly makes that ancillary file irrelevant.
    /// Failures after invocation require reopening.
    pub fn publish(self) -> Result<()> {
        self.prepare_ready(None)?
            .publish(crate::Durability::Full)
            .map(|_| ())
    }
}

/// Prepare a new descriptor with no predecessor, bound to the exact selected
/// checkpoint being undone. Logical eligibility and graph validation belong to
/// the native caller; this token never rotates an unrelated current into history.
pub fn prepare_rollback(
    directory: &Path,
    manifest: &Manifest,
    expected_current: &[u8; 64],
) -> Result<PreparedRollback> {
    if manifest.previous != 0 || manifest.sequence == 0 {
        return Err(Error::InvalidInput(
            "rollback requires nonzero root without predecessor",
        ));
    }
    let state = current(directory)?;
    let selected = read_checkpoint_with_transform(
        directory,
        &state.current,
        manifest.transform_context().cloned(),
    )?;
    check_predecessor(&state, &selected)?;
    if state.current != *expected_current
        || manifest.uuid != selected.uuid
        || manifest.epoch != selected.epoch
        || manifest.format_minor != selected.format_minor
        || manifest.page_size != selected.page_size
        || manifest.generation == selected.generation
    {
        return Err(Error::Corrupt("rollback preparation base mismatch"));
    }
    Ok(PreparedRollback {
        prepared: prepare_publication(directory, manifest)?,
        expected_current: *expected_current,
    })
}
/// A fenced, authenticated selected B and logical predecessor A. This proves
/// descriptors and their exact pair binding, not either root's logical graph.
/// The caller must retain native writer ownership until publication or drop.
pub struct PreviousSelection {
    directory: PathBuf,
    selected: Manifest,
    previous: Manifest,
    expected_current: [u8; 64],
    previous_checkpoint: [u8; 64],
    selected_durability: FileDurabilityState,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    directory_identity: crate::coordination::DirectoryIdentity,
}
impl PreviousSelection {
    pub fn selected(&self) -> &Manifest {
        &self.selected
    }
    pub fn previous(&self) -> &Manifest {
        &self.previous
    }
    /// Exact authenticated typed records captured together; no IO or re-encoding.
    pub fn selected_checkpoint(&self) -> &[u8; 64] {
        &self.expected_current
    }
    pub fn previous_checkpoint(&self) -> &[u8; 64] {
        &self.previous_checkpoint
    }
    pub fn selected_durability(&self) -> &FileDurabilityState {
        &self.selected_durability
    }
    /// Directory identity only, without rediscovering CURRENT or predecessor.
    pub fn matches_directory(&self, directory: impl AsRef<Path>) -> Result<bool> {
        let directory = directory.as_ref();
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || directory.canonicalize()? != self.directory
        {
            return Ok(false);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            use std::os::unix::fs::MetadataExt;
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let actual = (metadata.dev(), metadata.ino());
            #[cfg(windows)]
            let actual = coordination_directory_identity(directory)?;
            if actual != self.directory_identity {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Check the complete durable B checkpoint, without consulting ancillary
    /// PREVIOUS. A caller can preserve this error's precedence over other IO.
    pub fn check_current(&self) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            if coordination_directory_identity(&self.directory)? != self.directory_identity {
                return Err(Error::Corrupt("previous selection directory changed"));
            }
        }
        if current(&self.directory)?.current != self.expected_current {
            return Err(Error::Corrupt("previous selection durable head changed"));
        }
        Ok(())
    }
}
/// Capture one exact pair under caller-owned writer and capture fencing.
/// Missing nonzero history returns None. Only genuinely empty genesis may use B
/// itself as A; a native caller must additionally validate/admit A's full graph.
pub fn capture_previous_selection(directory: &Path) -> Result<Option<PreviousSelection>> {
    let history = crate::history::HistoryGate::shared(directory)?;
    capture_previous_selection_guarded(directory, &history)
}
pub(crate) fn capture_previous_selection_guarded(
    directory: &Path,
    history: &crate::history::HistoryGate,
) -> Result<Option<PreviousSelection>> {
    capture_previous_selection_expected(directory, history, None)
}
pub(crate) fn capture_previous_selection_guarded_with_transform(
    directory: &Path,
    history: &crate::history::HistoryGate,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<Option<PreviousSelection>> {
    capture_previous_selection_expected_with_transform(directory, history, None, transform)
}
pub(crate) fn capture_previous_selection_expected(
    directory: &Path,
    history: &crate::history::HistoryGate,
    expected: Option<&[u8; 64]>,
) -> Result<Option<PreviousSelection>> {
    capture_previous_selection_expected_with_transform(directory, history, expected, None)
}
pub(crate) fn capture_previous_selection_expected_with_transform(
    directory: &Path,
    history: &crate::history::HistoryGate,
    expected: Option<&[u8; 64]>,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<Option<PreviousSelection>> {
    capture_previous_selection_external_inner(directory, expected, transform, || {
        history.check(directory)
    })
}
pub(crate) fn capture_previous_selection_external_inner(
    directory: &Path,
    expected: Option<&[u8; 64]>,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    check: impl Fn() -> Result<()>,
) -> Result<Option<PreviousSelection>> {
    check()?;
    let state = current(directory)?;
    let selected = read_checkpoint_with_transform(directory, &state.current, transform)?;
    check_predecessor(&state, &selected)?;
    if expected.is_some_and(|expected| state.current != *expected) {
        return Err(Error::Busy);
    }
    let mut genesis = false;
    let previous = match previous_from_state(directory, &state, &selected)? {
        Some(previous) => previous,
        None if selected.sequence == 0 && selected.previous == 0 => {
            if !selected.catalog.is_null()
                || selected.parent != 0
                || selected.next_database != 1
                || selected.next_segment != 1
                || !selected.segments.is_empty()
            {
                return Err(Error::Corrupt("nonempty previous genesis"));
            }
            genesis = true;
            selected.clone()
        }
        None => return Ok(None),
    };
    if selected.uuid != previous.uuid
        || selected.epoch != previous.epoch
        || selected.page_size != previous.page_size
    {
        return Err(Error::Corrupt("previous selection realm mismatch"));
    }
    if selected.format_minor != previous.format_minor || !(2..=4).contains(&selected.format_minor) {
        return Err(Error::InvalidInput(
            "mixed or unsupported previous selection format",
        ));
    }
    if !genesis
        && (selected.generation == previous.generation || selected.previous != previous.generation)
    {
        return Err(Error::Corrupt("previous selection predecessor mismatch"));
    }
    let business = previous.sequence.checked_add(1) == Some(selected.sequence)
        && selected.parent == previous.commit
        && selected.commit != previous.commit;
    let maintenance = selected.sequence == previous.sequence
        && selected.commit == previous.commit
        && selected.parent == previous.parent;
    if !genesis && !(business || maintenance) {
        return Err(Error::Corrupt("previous selection logical lineage"));
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    let directory_identity = coordination_directory_identity(directory)?;
    check()?;
    Ok(Some(PreviousSelection {
        directory: directory.canonicalize()?,
        previous_checkpoint: checkpoint_bytes(&previous)?,
        selected,
        previous,
        expected_current: state.current,
        selected_durability: FileDurabilityState::captured(directory, &state)?,
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        directory_identity,
    }))
}
/// A prepared C based on A while durable CURRENT is still B. This authority is
/// distinct from ordinary publication and never rotates B into logical history.
pub struct PreparedPreviousPublication {
    prepared: PreparedPublication,
    expected_current: [u8; 64],
    previous_checkpoint: [u8; 64],
}
impl PreparedPreviousPublication {
    pub fn prepared(&self) -> &PreparedPublication {
        &self.prepared
    }
    pub fn publish(self) -> Result<()> {
        self.prepare_ready(None)?
            .publish(crate::Durability::Full)
            .map(|_| ())
    }
}
/// Bind a descriptor already prepared by the normal immutable-file path to a
/// captured previous selection. No ordinary publication precondition changes.
pub fn prepare_previous_publication(
    selection: &PreviousSelection,
    manifest: &Manifest,
    prepared: PreparedPublication,
) -> Result<PreparedPreviousPublication> {
    selection.check_current()?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    prepared.check_directory_identity(selection.directory_identity)?;
    let a = &selection.previous;
    let b = &selection.selected;
    if prepared.directory.canonicalize()? != selection.directory
        || prepared.pointer != checkpoint_bytes(manifest)?
        || manifest.uuid != a.uuid
        || manifest.epoch != a.epoch
        || manifest.page_size != a.page_size
        || manifest.format_minor != a.format_minor
        || manifest.previous != a.generation
        || prepared.previous != a.generation
        || manifest.parent != a.commit
        || a.sequence.checked_add(1) != Some(manifest.sequence)
        || prepared.sequence != manifest.sequence
        || manifest.next_database < a.next_database.max(b.next_database)
        || manifest.generation == a.generation
        || manifest.generation == b.generation
        || manifest.commit == a.commit
        || manifest.commit == b.commit
    {
        return Err(Error::Corrupt("previous publication candidate binding"));
    }
    Ok(PreparedPreviousPublication {
        prepared,
        expected_current: selection.expected_current,
        previous_checkpoint: selection.previous_checkpoint,
    })
}
fn prepared(directory: &Path, manifest: &Manifest, hash: [u8; 32]) -> Result<PreparedPublication> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    let directory_identity = coordination_directory_identity(directory)?;
    Ok(PreparedPublication {
        transform: manifest.transform_context().cloned(),
        directory: directory.to_path_buf(),
        pointer: record(manifest, hash),
        previous: manifest.previous,
        sequence: manifest.sequence,
        data_policy: crate::Durability::Full,
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        epoch: manifest.epoch,
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        directory_identity,
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        uuid: manifest.uuid,
    })
}
pub(crate) fn prepare_cohort(
    directory: &Path,
    manifest: &Manifest,
    bytes: Option<&[u8]>,
) -> Result<PreparedPublication> {
    prepare_cohort_with_durability(directory, manifest, bytes, crate::Durability::Full)
}
pub(crate) fn prepare_cohort_with_durability(
    directory: &Path,
    manifest: &Manifest,
    bytes: Option<&[u8]>,
    policy: crate::Durability,
) -> Result<PreparedPublication> {
    let hash = cohort::write_with_durability(directory, manifest, bytes, policy)?;
    let mut result = prepared(directory, manifest, hash)?;
    result.data_policy = policy;
    Ok(result)
}

/// Arena-only preparation retains the sealed inode through map construction.
pub(crate) fn prepare_mapped_cohort_with_durability(
    directory: &Path,
    manifest: &Manifest,
    bytes: Option<&[u8]>,
    policy: crate::Durability,
    read_ahead: crate::ReadAhead,
) -> Result<(PreparedPublication, Option<crate::mapping::Mapping>)> {
    prepare_mapped_cohort_with_transform(directory, manifest, bytes, policy, read_ahead, None)
}
pub(crate) fn prepare_mapped_cohort_with_transform(
    directory: &Path,
    manifest: &Manifest,
    bytes: Option<&[u8]>,
    policy: crate::Durability,
    read_ahead: crate::ReadAhead,
    context: Option<&std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(PreparedPublication, Option<crate::mapping::Mapping>)> {
    let sealed = cohort::write_open_with_transform(directory, manifest, bytes, policy, context)?;
    let mapping = match (sealed.segment, bytes) {
        (Some(entry), Some(bytes)) => Some(match context {
            Some(context) => crate::mapping::Mapping::from_transformed_file(
                &sealed.file,
                entry,
                Some(bytes),
                read_ahead,
                context,
            )?,
            None => crate::mapping::Mapping::from_written_file_with_read_ahead(
                &sealed.file,
                entry,
                bytes,
                sealed.physical,
                read_ahead,
            )?,
        }),
        (None, None) => None,
        _ => return Err(Error::Corrupt("sealed cohort mapping/payload mismatch")),
    };
    let mut publication = prepared(directory, manifest, sealed.hash)?;
    publication.data_policy = policy;
    Ok((publication, mapping))
}
pub fn install_snapshot(directory: &Path, manifest: &Manifest) -> Result<()> {
    if directory.join("CURRENT").try_exists()?
        || directory.join("PREVIOUS").try_exists()?
        || fs::read_dir(directory.join("commits"))?.next().is_some()
    {
        return Err(Error::InvalidInput(
            "snapshot target is already initialized",
        ));
    }
    // Indexed descriptors can refer to immutable nodes in copied cohorts.
    // Their resolver must use the destination, not an in-memory-only decoder.
    if manifest.format_minor < 4 {
        Manifest::decode(&manifest.encode()?)?;
    }
    let hash = if manifest.format_minor >= 3 {
        cohort::install(directory, manifest)?
    } else {
        write_descriptor(directory, manifest)?
    };
    let checkpoint = record(manifest, hash);
    read_checkpoint_with_transform(
        directory,
        &checkpoint,
        manifest.transform_context().cloned(),
    )?;
    prepared(directory, manifest, hash)?.activate(true)
}
/// Prepare an existing selected descriptor after verified immutable-file import.
/// Caller owns the writer fence and must durably fence every referenced data
/// file before publishing this token. This validates/persists the selected root;
/// it neither authenticates a peer nor validates the complete object graph.
pub fn prepare_existing(directory: &Path, checkpoint: &[u8; 64]) -> Result<PreparedPublication> {
    prepare_existing_with_transform(directory, checkpoint, None)
}
pub fn prepare_existing_with_transform(
    directory: &Path,
    checkpoint: &[u8; 64],
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<PreparedPublication> {
    let manifest = read_checkpoint_with_transform(directory, checkpoint, transform)?;
    let path = crate::descriptor_path(directory, &manifest);
    ulmdb_platform::sync_file(&File::open(&path)?)?;
    ulmdb_platform::sync_directory(
        path.parent()
            .ok_or(Error::Corrupt("descriptor has no parent"))?,
    )?;
    let (_, hash) = decode_pointer(checkpoint)?;
    prepared(directory, &manifest, hash)
}
/// A checked imported D/P pair selected against an exact existing destination.
/// This token does not certify logical graphs or an external publisher.
pub struct PreparedSnapshotImport {
    prepared: PreparedPublication,
    expected: [u8; 64],
    predecessor: Option<[u8; 64]>,
}
impl PreparedSnapshotImport {
    pub fn prepared(&self) -> &PreparedPublication {
        &self.prepared
    }
    pub fn publish(self) -> Result<()> {
        self.prepared
            .prepare_import_ready(self.expected, self.predecessor)?
            .publish(crate::Durability::Full)
            .map(|_| ())
    }
}
pub fn prepare_imported_snapshot(
    directory: &Path,
    checkpoint: &[u8; 64],
    expected: &[u8; 64],
    predecessor: Option<&[u8; 64]>,
) -> Result<PreparedSnapshotImport> {
    prepare_imported_snapshot_with_transform(directory, checkpoint, expected, predecessor, None)
}
pub fn prepare_imported_snapshot_with_transform(
    directory: &Path,
    checkpoint: &[u8; 64],
    expected: &[u8; 64],
    predecessor: Option<&[u8; 64]>,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<PreparedSnapshotImport> {
    let state = current(directory)?;
    if state.current != *expected {
        return Err(Error::Busy);
    }
    let base = read_checkpoint_with_transform(directory, expected, transform.clone())?;
    let target = read_checkpoint_with_transform(directory, checkpoint, transform.clone())?;
    if target.uuid != base.uuid
        || target.epoch != base.epoch
        || target.page_size != base.page_size
        || target.sequence <= base.sequence
        || target.next_database < base.next_database
        || target.next_segment < base.next_segment
    {
        return Err(Error::Corrupt("imported snapshot destination lineage"));
    }
    match predecessor {
        Some(cp) => {
            let p = read_checkpoint_with_transform(directory, cp, transform.clone())?;
            if target.previous != p.generation
                || p.uuid != target.uuid
                || p.epoch != target.epoch
                || !(p.sequence == target.sequence
                    || p.sequence.checked_add(1) == Some(target.sequence))
                || (p.sequence < target.sequence && target.parent != p.commit)
                || (p.sequence == target.sequence
                    && (target.commit != p.commit || target.parent != p.parent))
            {
                return Err(Error::Corrupt("imported snapshot predecessor"));
            }
        }
        None if target.previous == 0 => {}
        None => return Err(Error::Corrupt("missing imported predecessor")),
    }
    Ok(PreparedSnapshotImport {
        prepared: prepare_existing_with_transform(directory, checkpoint, transform.clone())?,
        expected: *expected,
        predecessor: predecessor.copied(),
    })
}
pub fn publish(directory: &Path, manifest: &Manifest) -> Result<()> {
    prepare_publication(directory, manifest)?.publish()
}
/// Persist an immutable descriptor without selecting it as CURRENT.
/// The caller retains the writer fence until this token is published or dropped.
/// This has exactly the descriptor/seal behavior of `publish`; publication and
/// its uncertainty boundary remain the caller's responsibility.
pub fn prepare_publication(directory: &Path, manifest: &Manifest) -> Result<PreparedPublication> {
    if manifest.format_minor >= 3 {
        prepare_cohort(directory, manifest, None)
    } else {
        prepared(directory, manifest, write_descriptor(directory, manifest)?)
    }
}
pub fn checkpoint_bytes(manifest: &Manifest) -> Result<[u8; 64]> {
    Ok(record(manifest, crate::work::digest(&manifest.encode()?)?))
}
/// Decode a retained immutable generation. Callers must retain capture/writer
/// exclusion and check its lineage against their authenticated selected root.
pub fn read_generation(directory: &Path, generation: u128, minor: u16) -> Result<Manifest> {
    if generation == 0 || !(1..=4).contains(&minor) {
        return Err(Error::InvalidInput("invalid retained generation"));
    }
    let path = if minor >= 3 {
        crate::cohort::path(directory, generation)
    } else {
        directory
            .join("commits")
            .join(format!("{generation:032x}.root"))
    };
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if minor >= 3 && e.kind() == std::io::ErrorKind::NotFound => {
            let checkpoint = crate::cohort_history::checkpoint(directory, generation, minor)?;
            return read_checkpoint(directory, &checkpoint);
        }
        Err(e) => return Err(e.into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::Corrupt("nonregular retained descriptor"));
    }
    let bytes = if minor >= 3 {
        crate::cohort::read(directory, generation)?.0
    } else {
        read_bounded(&path, 67_109_056)?
    };
    let hash = crate::work::digest(&bytes)?;
    let mut checkpoint = pointer(generation, hash);
    if minor >= 3 {
        checkpoint[..8].copy_from_slice(if minor == 4 { b"ULMDBT04" } else { b"ULMDBC03" });
        let checksum = crc(&checkpoint[..60]);
        checkpoint[60..64].copy_from_slice(&checksum.to_le_bytes());
    }
    let manifest = read_checkpoint(directory, &checkpoint)?;
    if manifest.generation != generation || manifest.format_minor != minor {
        return Err(Error::Corrupt("retained generation mismatch"));
    }
    Ok(manifest)
}
pub fn read_current_with_transform(
    directory: &Path,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<Manifest> {
    read_current_checkpoint_with_transform(directory, transform).map(|(m, _)| m)
}
pub fn read_roots_with_transform(
    directory: &Path,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, Option<Manifest>)> {
    read_roots_checked_with_transform(directory, |_| Ok(()), transform)
}
pub fn read_roots_checked_with_transform<F: FnOnce(&Manifest) -> Result<()>>(
    directory: &Path,
    selected_check: F,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, Option<Manifest>)> {
    let state = current(directory)?;
    let selected = read_checkpoint_with_transform(directory, &state.current, transform)?;
    check_predecessor(&state, &selected)?;
    selected_check(&selected)?;
    let previous = previous_from_state(directory, &state, &selected)?;
    Ok((selected, previous))
}
pub fn read_checkpoint_with_transform(
    directory: &Path,
    bytes: &[u8],
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<Manifest> {
    read_checkpoint_inner(
        directory,
        None,
        bytes,
        crate::read_mode::ReadMode::Ordinary.with_transform(transform),
    )
}
pub fn read_generation_with_transform(
    directory: &Path,
    generation: u128,
    minor: u16,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<Manifest> {
    let path = if minor >= 3 {
        cohort::path(directory, generation)
    } else {
        directory
            .join("commits")
            .join(format!("{generation:032x}.root"))
    };
    let mode = crate::read_mode::ReadMode::Ordinary.with_transform(transform.clone());
    let bytes = if minor >= 3 {
        let mut file = match mode.open(&path) {
            Ok(file) => file,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                let checkpoint = crate::cohort_history::checkpoint(directory, generation, minor)?;
                return read_checkpoint_with_transform(directory, &checkpoint, transform);
            }
            Err(e) => return Err(e),
        };
        cohort::read_descriptor(&mut file, generation, &mode)?.0
    } else {
        mode.read_legacy(&path, 67_109_056)?
    };
    let hash = crate::work::digest(&bytes)?;
    let mut pointer = pointer(generation, hash);
    if minor >= 3 {
        pointer[..8].copy_from_slice(if minor == 4 { b"ULMDBT04" } else { b"ULMDBC03" });
        let sum = crc(&pointer[..60]);
        pointer[60..].copy_from_slice(&sum.to_le_bytes());
    }
    read_checkpoint_with_transform(directory, &pointer, transform)
}
pub fn read_current(directory: &Path) -> Result<Manifest> {
    read_current_checkpoint(directory).map(|(manifest, _)| manifest)
}
pub(crate) fn read_current_checkpoint(directory: &Path) -> Result<(Manifest, [u8; 64])> {
    read_current_checkpoint_with_transform(directory, None)
}
pub(crate) fn read_current_checkpoint_with_transform(
    directory: &Path,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, [u8; 64])> {
    let state = current(directory)?;
    let manifest = read_checkpoint_with_transform(directory, &state.current, transform)?;
    check_predecessor(&state, &manifest)?;
    Ok((manifest, state.current))
}
pub(crate) fn read_current_checkpoint_with_durability(
    directory: &Path,
) -> Result<(Manifest, [u8; 64], FileDurabilityState)> {
    read_current_checkpoint_with_durability_and_transform(directory, None)
}
pub(crate) fn read_current_checkpoint_with_durability_and_transform(
    directory: &Path,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, [u8; 64], FileDurabilityState)> {
    let state = current(directory)?;
    let manifest = read_checkpoint_with_transform(directory, &state.current, transform.clone())?;
    check_predecessor(&state, &manifest)?;
    let durability = FileDurabilityState::captured(directory, &state)?.with_transform(transform);
    Ok((manifest, state.current, durability))
}
pub fn read_previous(directory: &Path) -> Result<Option<Manifest>> {
    let state = current(directory)?;
    if state.bundled {
        let manifest = read_checkpoint(directory, &state.current)?;
        check_predecessor(&state, &manifest)?;
        previous_from_state(directory, &state, &manifest)
    } else if read_checkpoint(directory, &state.current).is_ok_and(|m| m.previous == 0) {
        // Suppression requires a positively authenticated selected manifest.
        // Otherwise preserve Legacy's historical standalone ancillary-reader
        // contract. Strict captures use read_roots[_checked], which never fall
        // back from an invalid selected descriptor.
        Ok(None)
    } else {
        legacy_previous(directory)
    }
}
/// Read one captured CURRENT and its predecessor, decoding each selected root
/// once. Both roots retain full typed/hash/metadata validation. A caller needing
/// a stable pair while legacy writers run must hold the environment writer fence.
/// The standalone readers retain their narrower validation behavior.
pub fn read_roots(directory: &Path) -> Result<(Manifest, Option<Manifest>)> {
    read_roots_checked(directory, |_| Ok(()))
}
/// Check the fully validated selected root before decoding its predecessor.
/// This boundary lets a fenced caller reject/poison a changed durable head with
/// its original error precedence, even when the predecessor is also corrupt.
/// The callback runs once per operation, never per metadata node; an error is
/// returned unchanged and prevents predecessor IO. The caller owns writer fencing.
pub fn read_roots_checked<F>(
    directory: &Path,
    selected_check: F,
) -> Result<(Manifest, Option<Manifest>)>
where
    F: FnOnce(&Manifest) -> Result<()>,
{
    let state = current(directory)?;
    let selected = read_checkpoint(directory, &state.current)?;
    check_predecessor(&state, &selected)?;
    selected_check(&selected)?;
    let previous = previous_from_state(directory, &state, &selected)?;
    Ok((selected, previous))
}
fn previous_from_state(
    directory: &Path,
    state: &Current,
    selected: &Manifest,
) -> Result<Option<Manifest>> {
    if state.bundled {
        state
            .previous
            .map(|p| {
                read_checkpoint_with_transform(directory, &p, selected.transform_context().cloned())
            })
            .transpose()
    } else if selected.previous == 0 {
        Ok(None)
    } else {
        match read_bounded(&directory.join("PREVIOUS"), 64) {
            Ok(bytes) => read_checkpoint_with_transform(
                directory,
                &bytes,
                selected.transform_context().cloned(),
            )
            .map(Some),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}
fn legacy_previous(directory: &Path) -> Result<Option<Manifest>> {
    match read_bounded(&directory.join("PREVIOUS"), 64) {
        Ok(bytes) => read_checkpoint(directory, &bytes).map(Some),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
pub fn read_checkpoint(directory: &Path, bytes: &[u8]) -> Result<Manifest> {
    read_checkpoint_inner(directory, None, bytes, crate::read_mode::ReadMode::Ordinary)
}
/// Resolve an incremental staged root using existing immutable metadata nodes.
/// The selected descriptor is always read from `directory`; only old node
/// references may use `fallback`. Callers must fence/authenticate both inputs.
pub fn read_checkpoint_with_fallback(
    directory: &Path,
    fallback: &Path,
    bytes: &[u8],
) -> Result<Manifest> {
    read_checkpoint_inner(
        directory,
        Some(fallback),
        bytes,
        crate::read_mode::ReadMode::Ordinary,
    )
}
fn read_checkpoint_inner(
    directory: &Path,
    fallback: Option<&Path>,
    bytes: &[u8],
    mode: crate::read_mode::ReadMode,
) -> Result<Manifest> {
    let (generation, hash) = decode_pointer(bytes)?;
    if is_cohort(bytes) {
        let manifest =
            cohort::decode_with_mode(directory, fallback, generation, hash, mode.clone())?;
        let expected = if &bytes[..8] == b"ULMDBT04" { 4 } else { 3 };
        if manifest.format_minor != expected {
            return Err(Error::Corrupt("root generation/storage type mismatch"));
        }
        return Ok(manifest.with_transform_context(mode.context().cloned()));
    }
    let path = directory
        .join("commits")
        .join(format!("{generation:032x}.root"));
    let bytes = mode.read_legacy(&path, 67_109_056)?;
    if crate::work::digest(&bytes)? != hash {
        return Err(Error::Corrupt("root descriptor hash mismatch"));
    }
    let manifest = Manifest::decode(&bytes)?;
    if manifest.generation != generation || manifest.format_minor >= 3 {
        return Err(Error::Corrupt("root generation/storage type mismatch"));
    }
    Ok(manifest.with_transform_context(mode.context().cloned()))
}
#[cfg(test)]
mod tests;

pub fn read_checkpoint_with_fallback_and_transform(
    directory: &Path,
    fallback: &Path,
    bytes: &[u8],
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<Manifest> {
    read_checkpoint_inner(
        directory,
        Some(fallback),
        bytes,
        crate::read_mode::ReadMode::Ordinary.with_transform(transform),
    )
}

/// Validated exact imported D/P under explicit caller-wide exclusion.
pub struct ExternalSnapshotImport {
    prepared: PreparedSnapshotImport,
    authority: crate::external::ExternalSynchronization,
}
impl ExternalSnapshotImport {
    pub fn prepared(&self) -> &PreparedPublication {
        self.prepared.prepared()
    }
    pub fn publish(self) -> Result<()> {
        self.authority.check_prepared(&self.prepared.prepared)?;
        self.prepared
            .prepared
            .prepare_import_ready_external(
                self.prepared.expected,
                self.prepared.predecessor,
                &self.authority,
            )?
            .publish(crate::Durability::Full)
            .map(|_| ())
    }
}
impl PreparedSnapshotImport {
    pub(crate) fn externally_synchronized(
        self,
        authority: &crate::external::ExternalSynchronization,
    ) -> Result<ExternalSnapshotImport> {
        authority.check_prepared(&self.prepared)?;
        Ok(ExternalSnapshotImport {
            prepared: self,
            authority: authority.clone(),
        })
    }
}

/// Arena has sealed the exact canonical cohort at its genuine generation name.
/// This wraps its hash/namespace as the same ordinary publication authority.
pub(crate) fn prepare_promoted_cohort(
    directory: &Path,
    manifest: &Manifest,
    hash: [u8; 32],
    policy: crate::Durability,
) -> Result<PreparedPublication> {
    let mut publication = prepared(directory, manifest, hash)?;
    publication.data_policy = policy;
    Ok(publication)
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn coordination_directory_identity(
    directory: &Path,
) -> Result<crate::coordination::DirectoryIdentity> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt;
        let m = fs::metadata(directory)?;
        if !m.is_dir() {
            return Err(Error::Corrupt("publication directory"));
        }
        Ok((m.dev(), m.ino()))
    }
    #[cfg(windows)]
    {
        let m = ulmdb_platform::path_identity(directory)?;
        if !m.is_dir() {
            return Err(Error::Corrupt("publication directory"));
        }
        Ok((m.volume, m.file_id))
    }
}
