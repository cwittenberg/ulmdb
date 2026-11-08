//! Schedule actual selected backing objects: MS_ASYNC on Linux and
//! FlushViewOfFile on Windows. Neither is a file/namespace durability fence or
//! a certificate that existing weak-selection/data debt has been repaired.
use super::*;
#[derive(Clone, Debug)]
pub struct FileAsyncFlushReport {
    pub selected_checkpoint: [u8; 64],
    pub previous_checkpoint: Option<[u8; 64]>,
    pub files_scheduled: usize,
    pub file_bytes_covered: u64,
}
pub fn flush_async_file_environment(
    path: impl AsRef<Path>,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<FileAsyncFlushReport> {
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (path, transform);
        Err(Error::InvalidInput(
            "asynchronous mapped sync requires Linux, macOS or Windows",
        ))
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let directory = path.as_ref().canonicalize()?;
        if crate::range::is_selected(&directory)? {
            return Err(Error::InvalidInput(
                "async file flush does not support Range",
            ));
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
        async_capture(capture)
    }
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn async_capture(
    capture: crate::coordination::CaptureLease,
) -> Result<FileAsyncFlushReport> {
    let (directory, uuid) = capture.sync_namespace()?;
    if directory.join("REPLICA").try_exists()? {
        return Err(Error::ReadOnly);
    }
    let history = crate::history::HistoryGate::exclusive(directory)?;
    capture.sync_namespace()?;
    let report = async_checked(directory, uuid, capture.transform_context(), || {
        capture.sync_namespace().map(|_| ())
    })?;
    history.check(directory)?;
    capture.sync_namespace()?;
    Ok(report)
}
pub(crate) fn async_external(
    authority: &crate::external::ExternalSynchronization,
    directory: &Path,
    uuid: [u8; 16],
    epoch: u64,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<FileAsyncFlushReport> {
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (authority, directory, uuid, epoch, transform);
        Err(Error::InvalidInput(
            "async mapped sync requires Linux, macOS or Windows",
        ))
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        authority.check(directory, uuid, epoch)?;
        if directory.join("REPLICA").try_exists()? {
            return Err(Error::ReadOnly);
        }
        async_checked(directory, uuid, transform, || {
            authority.check(directory, uuid, epoch)
        })
    }
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn async_checked(
    directory: &Path,
    uuid: [u8; 16],
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    check: impl Fn() -> Result<()>,
) -> Result<FileAsyncFlushReport> {
    let (selected, previous, ancillary) =
        crate::publication::read_sync_roots_checked_with_transform(
            directory,
            |m| {
                check()?;
                if m.uuid != uuid {
                    return Err(Error::Corrupt("async durability realm changed"));
                }
                Ok(())
            },
            transform,
        )?;
    if let Some(p) = &previous {
        if p.generation != selected.previous || p.uuid != selected.uuid || p.epoch != selected.epoch
        {
            return Err(Error::Corrupt("async durability predecessor lineage"));
        }
    } else if ancillary && selected.previous != 0 {
        match fs::symlink_metadata(directory.join("PREVIOUS")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
            Ok(_) => return Err(Error::Corrupt("async durability unresolvable predecessor")),
        }
    }
    let mut files = BTreeSet::new();
    assets(directory, &selected, &mut files)?;
    if let Some(p) = &previous {
        assets(directory, p, &mut files)?;
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let mut roots = vec![&selected];
        if let Some(previous) = &previous {
            roots.push(previous);
        }
        if crate::fixed_map::FixedMap::validate_layout(directory, &roots)?.is_some() {
            files.insert(directory.join("FIXEDMAP"));
        }
    }
    files.insert(directory.join("CURRENT"));
    if ancillary && previous.is_some() {
        files.insert(directory.join("PREVIOUS"));
    }
    let mut bytes = 0u64;
    for path in &files {
        check()?;
        let file = checked_open(path, false)?;
        let length = file.metadata()?.len();
        if length == 0 || length > crate::MAX_COHORT_BYTES {
            return Err(Error::Corrupt("async mapped file extent"));
        }
        // SAFETY: checked immutable selected assets or a selector protected by
        // History/caller-owned exclusion. Capture protects names from reclaim;
        // the complete regular-file extent is fixed while mapped. The actual
        // platform operation schedules backing pages without writing through a
        // Rust slice or reopening sealed files writable. Windows invokes
        // FlushViewOfFile exactly as pinned LMDB's MDB_MSYNC macro; it does not
        // append FlushFileBuffers/NtFlushBuffersFileEx or any directory fence.
        // Metadata callbacks decode only the captured descriptors above; these
        // mappings cover the physical encoded files, never decoded scratch.
        #[allow(unsafe_code)]
        let mapped = unsafe {
            memmap2::MmapOptions::new()
                .len(usize::try_from(length).map_err(|_| Error::LimitExceeded)?)
                .map(&file)?
        };
        crate::fault_point!("async_sync.before_msync");
        #[allow(unsafe_code)]
        unsafe {
            ulmdb_platform::flush_view(mapped.as_ptr().cast_mut(), mapped.len())?
        };
        crate::fault_point!("async_sync.after_msync");
        check_inode(path, &file, false)?;
        bytes = bytes.checked_add(length).ok_or(Error::LimitExceeded)?;
    }
    check()?;
    Ok(FileAsyncFlushReport {
        selected_checkpoint: crate::checkpoint_bytes(&selected)?,
        previous_checkpoint: previous.as_ref().map(crate::checkpoint_bytes).transpose()?,
        files_scheduled: files.len(),
        file_bytes_covered: bytes,
    })
}
