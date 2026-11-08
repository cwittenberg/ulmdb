//! Explicit file durability operations. Flushing does not certify user graphs.
use crate::{Error, Manifest, Result};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

/// Business-publication policy. Existing entry points retain Full.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Durability {
    #[default]
    Full,
    NoMetaSync,
    NoSync,
}
impl Durability {
    pub const fn sync_data(self) -> bool {
        !matches!(self, Self::NoSync)
    }
    pub const fn sync_pointer_file(self) -> bool {
        !matches!(self, Self::NoSync)
    }
    pub const fn sync_pointer_directory(self) -> bool {
        matches!(self, Self::Full)
    }
    /// A transaction override may weaken the environment's current policy.
    pub const fn weaken(self, local: Self) -> Self {
        match (self, local) {
            (Self::NoSync, _) | (_, Self::NoSync) => Self::NoSync,
            (Self::NoMetaSync, _) | (_, Self::NoMetaSync) => Self::NoMetaSync,
            _ => Self::Full,
        }
    }
}

/// Actual policy of newly sealed Arena bytes. Only storage sealers mint this
/// receipt; it does not certify inherited files or logical graph integrity.
pub struct SealedAssets {
    directory: PathBuf,
    identity: Option<(u64, u128)>,
    creator: u32,
    entry: Option<crate::SegmentEntry>,
    policy: Durability,
}
impl SealedAssets {
    pub fn durability(&self) -> Durability {
        self.policy
    }
    pub(crate) fn new(
        directory: &Path,
        entry: Option<crate::SegmentEntry>,
        policy: Durability,
    ) -> Result<Self> {
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::InvalidInput("sealed assets directory"));
        }
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            Some((metadata.dev(), metadata.ino() as u128))
        };
        #[cfg(windows)]
        let identity = {
            let physical = ulmdb_platform::path_identity(directory)?;
            if !physical.directory || physical.reparse {
                return Err(Error::InvalidInput("sealed assets directory"));
            }
            Some((physical.volume, physical.file_id))
        };
        #[cfg(not(any(unix, windows)))]
        let identity = None;
        Ok(Self {
            directory: directory.canonicalize()?,
            identity,
            creator: std::process::id(),
            entry,
            policy,
        })
    }
    pub(crate) fn check(&self, directory: &Path, manifest: &Manifest) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited sealed assets"));
        }
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::InvalidInput("sealed assets directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if self.identity != Some((metadata.dev(), metadata.ino() as u128)) {
                return Err(Error::InvalidInput("sealed assets directory identity"));
            }
        }
        #[cfg(windows)]
        {
            let physical = ulmdb_platform::path_identity(directory)?;
            if physical.reparse
                || !physical.directory
                || self.identity != Some((physical.volume, physical.file_id))
            {
                return Err(Error::InvalidInput("sealed assets directory identity"));
            }
        }
        if directory != self.directory && directory.canonicalize()? != self.directory {
            return Err(Error::InvalidInput("sealed assets namespace"));
        }
        if let Some(entry) = &self.entry {
            if !manifest.segments.iter().any(|candidate| {
                candidate.id == entry.id
                    && candidate.hash == entry.hash
                    && candidate.length == entry.length
                    && candidate.cohort == entry.cohort
            }) {
                return Err(Error::Corrupt("sealed assets inventory mismatch"));
            }
        }
        Ok(())
    }
}
pub(crate) fn check_policy(policy: Durability) -> Result<()> {
    if policy != Durability::Full && !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        return Err(Error::InvalidInput(
            "weak file durability requires Linux, macOS or Windows",
        ));
    }
    Ok(())
}

/// Counts files covered by actual successful fences, not physical write bytes.
#[derive(Clone, Debug)]
pub struct FileSyncReport {
    pub selected_checkpoint: [u8; 64],
    pub previous_checkpoint: Option<[u8; 64]>,
    pub files_synced: usize,
    pub directories_synced: usize,
    pub file_bytes_covered: u64,
}

fn assets(directory: &Path, manifest: &Manifest, files: &mut BTreeSet<PathBuf>) -> Result<()> {
    if manifest.format_minor > 4 {
        return Err(Error::InvalidInput("file sync does not support Range"));
    }
    files.insert(crate::descriptor_path(directory, manifest));
    for entry in &manifest.segments {
        files.insert(crate::segment_path(directory, entry));
    }
    for origin in manifest.metadata_origins_ref() {
        files.insert(crate::cohort::path(directory, *origin));
    }
    Ok(())
}

fn checked_open(path: &Path, directory: bool) -> Result<File> {
    if !directory {
        return crate::read_mode::ReadMode::Sync.open(path);
    }
    #[cfg(windows)]
    {
        let file = ulmdb_platform::open_directory(path)?;
        check_inode(path, &file, true)?;
        return Ok(file);
    }
    #[cfg(not(windows))]
    {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(
                libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CLOEXEC
                    | if directory { libc::O_DIRECTORY } else { 0 },
            );
        }
        let file = options.open(path)?;
        check_inode(path, &file, directory)?;
        Ok(file)
    }
}

fn check_inode(path: &Path, file: &File, directory: bool) -> Result<()> {
    let named = fs::symlink_metadata(path)?;
    let owned = file.metadata()?;
    if named.file_type().is_symlink()
        || if directory {
            !named.is_dir() || !owned.is_dir()
        } else {
            !named.is_file() || !owned.is_file()
        }
    {
        return Err(Error::Corrupt("invalid durability file type"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (named.dev(), named.ino()) != (owned.dev(), owned.ino()) {
            return Err(Error::Corrupt("durability inode changed"));
        }
    }
    #[cfg(windows)]
    {
        let named = ulmdb_platform::path_identity(path)?;
        let held = ulmdb_platform::file_identity(file)?;
        if !named.same_object(&held) {
            return Err(Error::Corrupt("durability file identity changed"));
        }
    }
    Ok(())
}

fn sync_files(files: BTreeSet<PathBuf>) -> Result<(usize, usize, u64)> {
    let mut directories = BTreeSet::new();
    let mut bytes = 0u64;
    for path in &files {
        let file = checked_open(path, false)?;
        let length = file.metadata()?.len();
        crate::fault_point!("sync.before_file_sync");
        ulmdb_platform::sync_file(&file)?;
        crate::fault_point!("sync.after_file_sync");
        check_inode(path, &file, false)?;
        bytes = bytes.checked_add(length).ok_or(Error::LimitExceeded)?;
        directories.insert(
            path.parent()
                .ok_or(Error::Corrupt("sync file parent"))?
                .to_path_buf(),
        );
    }
    for path in &directories {
        let file = checked_open(path, true)?;
        crate::fault_point!("sync.before_assets_directory_sync");
        ulmdb_platform::sync_directory_file(&file)?;
        crate::fault_point!("sync.after_assets_directory_sync");
        check_inode(path, &file, true)?;
    }
    Ok((files.len(), directories.len(), bytes))
}

/// Synchronize selected immutable assets while the caller excludes publication
/// and reclamation. Neither metadata nor selection files are rewritten.
pub(crate) fn sync_manifests(
    directory: &Path,
    manifests: &[&Manifest],
) -> Result<(usize, usize, u64)> {
    let mut files = BTreeSet::new();
    for manifest in manifests {
        assets(directory, manifest, &mut files)?;
    }
    sync_files(files)
}

/// Flush the selected file roots without acquiring native LOCK or a writer
/// mutex. A caller may therefore retain a clean, dirty or prepared writer.
///
/// Capture SH protects file names from reclaim; History EX prevents a newer
/// unsynchronized head from being selected while this call fences CURRENT.
/// Ordinary current readers remain compatible. Previous capture/selection waits
/// for this explicit flush. This operation is not a full logical integrity audit.
pub fn sync_file_environment(path: impl AsRef<Path>) -> Result<FileSyncReport> {
    sync_file_environment_with_transform(path, None)
}
pub fn sync_file_environment_with_transform(
    path: impl AsRef<Path>,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<FileSyncReport> {
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = path;
        return Err(Error::InvalidInput("coordinated file sync is Linux-only"));
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let directory = path.as_ref().canonicalize()?;
        if crate::range::is_selected(&directory)? {
            return Err(Error::InvalidInput("file sync does not support Range"));
        }
        if directory.join("REPLICA").try_exists()? {
            return Err(Error::ReadOnly);
        }
        let unbound = crate::coordination::Coordination::capture_directory_with_transform(
            &directory,
            Default::default(),
            transform.clone(),
        )?;
        unbound.directory_identity()?;
        let selected = crate::publication::read_sync_current_with_transform(&directory, transform)?;
        let (_, capture) = unbound.bind(selected.uuid)?;
        sync_capture(capture)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn sync_capture(capture: crate::coordination::CaptureLease) -> Result<FileSyncReport> {
    let (directory, uuid) = capture.sync_namespace()?;
    if directory.join("REPLICA").try_exists()? {
        return Err(Error::ReadOnly);
    }
    let history = crate::history::HistoryGate::exclusive(directory)?;
    // Recheck the originally bound owner after waiting: a fresh path lookup
    // must never certify a replacement environment.
    capture.sync_namespace()?;
    let report = sync_checked(directory, uuid, capture.transform_context(), || {
        capture.sync_namespace().map(|_| ())
    })?;
    history.check(directory)?;
    capture.sync_namespace()?;
    Ok(report)
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn sync_checked(
    directory: &Path,
    uuid: [u8; 16],
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    check: impl Fn() -> Result<()>,
) -> Result<FileSyncReport> {
    let (selected, previous, external_previous) =
        crate::publication::read_sync_roots_checked_with_transform(
            directory,
            |selected| {
                check()?;
                if selected.uuid != uuid {
                    return Err(Error::Corrupt("durability namespace realm changed"));
                }
                Ok(())
            },
            transform,
        )?;
    // Old snapshot/export protocols preserve commit metadata while deliberately
    // omitting historical roots. Absence is valid; any present predecessor must
    // still authenticate the exact selected generation/realm/epoch relation.
    if let Some(previous) = &previous {
        if previous.generation != selected.previous
            || previous.uuid != selected.uuid
            || previous.epoch != selected.epoch
        {
            return Err(Error::Corrupt("durability predecessor lineage"));
        }
    } else if external_previous && selected.previous != 0 {
        // Legacy's generic optional reader maps ENOENT to absence. A dangling
        // symlink is nevertheless present metadata, not an exported no-history
        // root, and must not be silently ignored by the sync operation.
        match fs::symlink_metadata(directory.join("PREVIOUS")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {
                return Err(Error::Corrupt(
                    "durability unresolvable predecessor pointer",
                ));
            }
        }
    }
    let checkpoint = crate::checkpoint_bytes(&selected)?;
    let mut roots = vec![&selected];
    if let Some(previous) = &previous {
        roots.push(previous);
    }
    // Capture protects all selected files without consuming another pin slot.
    let (mut files_synced, directories_synced, mut file_bytes_covered) =
        sync_manifests(directory, &roots)?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if crate::fixed_map::FixedMap::validate_layout(directory, &roots)?.is_some() {
        let path = directory.join("FIXEDMAP");
        let file = checked_open(&path, false)?;
        crate::fault_point!("sync.before_fixed_layout_sync");
        ulmdb_platform::sync_file(&file)?;
        crate::fault_point!("sync.after_fixed_layout_sync");
        check_inode(&path, &file, false)?;
        files_synced = files_synced.checked_add(1).ok_or(Error::LimitExceeded)?;
        file_bytes_covered = file_bytes_covered
            .checked_add(file.metadata()?.len())
            .ok_or(Error::LimitExceeded)?;
    }
    let current = checked_open(&directory.join("CURRENT"), false)?;
    crate::fault_point!("sync.before_current_file_sync");
    ulmdb_platform::sync_file(&current)?;
    crate::fault_point!("sync.after_current_file_sync");
    check_inode(&directory.join("CURRENT"), &current, false)?;
    files_synced = files_synced.checked_add(1).ok_or(Error::LimitExceeded)?;
    file_bytes_covered = file_bytes_covered
        .checked_add(current.metadata()?.len())
        .ok_or(Error::LimitExceeded)?;
    if external_previous && previous.is_some() {
        let previous_file = checked_open(&directory.join("PREVIOUS"), false)?;
        crate::fault_point!("sync.before_previous_file_sync");
        ulmdb_platform::sync_file(&previous_file)?;
        crate::fault_point!("sync.after_previous_file_sync");
        check_inode(&directory.join("PREVIOUS"), &previous_file, false)?;
        files_synced = files_synced.checked_add(1).ok_or(Error::LimitExceeded)?;
        file_bytes_covered = file_bytes_covered
            .checked_add(previous_file.metadata()?.len())
            .ok_or(Error::LimitExceeded)?;
    }
    let parent = checked_open(directory, true)?;
    crate::fault_point!("sync.before_selection_directory_sync");
    ulmdb_platform::sync_directory_file(&parent)?;
    crate::fault_point!("sync.after_selection_directory_sync");
    check_inode(directory, &parent, true)?;
    check()?;
    Ok(FileSyncReport {
        selected_checkpoint: checkpoint,
        previous_checkpoint: previous.as_ref().map(crate::checkpoint_bytes).transpose()?,
        files_synced,
        directories_synced: directories_synced
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?,
        file_bytes_covered,
    })
}

pub(crate) fn sync_external(
    authority: &crate::external::ExternalSynchronization,
    directory: &Path,
    uuid: [u8; 16],
    epoch: u64,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<FileSyncReport> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        authority.check(directory, uuid, epoch)?;
        if directory.join("REPLICA").try_exists()? {
            return Err(Error::ReadOnly);
        }
        sync_checked(directory, uuid, transform, || {
            authority.check(directory, uuid, epoch)
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (authority, directory, uuid, epoch, transform);
        Err(Error::InvalidInput("external sync requires Linux"))
    }
}

mod asynchronous;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) use asynchronous::async_capture;
pub(crate) use asynchronous::async_external;
pub use asynchronous::{FileAsyncFlushReport, flush_async_file_environment};
