//! Consistent native snapshots behind LMDB's copy entry points.
mod archive;
mod empty;
use super::*;
pub(super) use archive::{
    restore_archive, restore_archive_externally_synchronized_with_transform,
    restore_archive_with_cache_options, restore_archive_with_transform,
};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};
pub(super) const ARCHIVE_MAGIC: &[u8; 8] = b"ULMDBCP\0";

/// Remove only the uniquely created staging directory owned by this operation.
pub(super) struct Staging {
    path: PathBuf,
    owner: PathBuf,
}
impl Staging {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn absent(parent: &Path) -> FfiResult<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        for _ in 0..1024 {
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let owner = parent.join(format!(".ulmdb-copy-{}-{id}", std::process::id()));
            match fs::create_dir(&owner) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&owner, fs::Permissions::from_mode(0o700))
                            .map_err(|e| code(e.into()))?;
                    }
                    return Ok(Self {
                        path: owner.join("snapshot"),
                        owner,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(code(e.into())),
            }
        }
        Err(libc::EEXIST)
    }
    pub(super) fn publish(self, target: &Path) -> FfiResult<()> {
        check_target(target)?;
        ulmdb_platform::rename_replace(&self.path, target).map_err(|error| {
            if ulmdb_platform::rename_outcome_unknown(&error) {
                code(ulmdb::Error::OutcomeUnknown)
            } else {
                code(error.into())
            }
        })?;
        let parent = parent(target);
        ulmdb_platform::sync_path(parent).map_err(|e| code(e.into()))
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.owner);
    }
}
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn check_target(path: &Path) -> FfiResult<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(code(e.into())),
        Ok(metadata) if metadata.file_type().is_dir() => {
            if fs::read_dir(path)
                .map_err(|e| code(e.into()))?
                .next()
                .is_some()
            {
                Err(libc::EEXIST)
            } else {
                Ok(())
            }
        }
        Ok(_) => Err(libc::EEXIST),
    }
}
fn adapter_metadata(source: &Path, target: &Path) -> FfiResult<()> {
    use std::io::{Read, Write};
    for (name, limit) in [("LMDB-ABI", 64usize), ("LMDB-MAPSIZE", 8usize)] {
        let path = source.join(name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(code(error.into())),
            Ok(metadata) if !metadata.is_file() || metadata.len() > limit as u64 => {
                return Err(-30796);
            }
            Ok(_) => {}
        }
        if name == "LMDB-MAPSIZE" {
            map_size::read(source)?;
        }
        let mut bytes = Vec::with_capacity(limit + 1);
        File::open(&path)
            .map_err(|e| code(e.into()))?
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| code(e.into()))?;
        if bytes.len() > limit {
            return Err(-30796);
        }
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target.join(name))
            .map_err(|e| code(e.into()))?;
        output.write_all(&bytes).map_err(|e| code(e.into()))?;
        ulmdb_platform::sync_file(&output).map_err(|e| code(e.into()))?;
    }
    ulmdb_platform::sync_path(target).map_err(|e| code(e.into()))
}

fn snapshot(
    env: &MDB_env,
    parent: &Path,
    flags: u32,
    consume: impl FnOnce(Staging) -> FfiResult<()>,
) -> FfiResult<()> {
    if flags & !1 != 0 {
        return Err(libc::EINVAL);
    }
    let shared = env.shared.as_ref().ok_or(libc::EINVAL)?;
    let capture = previous::copy_capture::capture(shared, env.flags(), flags & 1 != 0)?;
    // Capture/Visibility/History choose only authenticated metadata/pins. Both
    // complete graph admission and all building happen after Visibility drops.
    let source = capture.admit(
        shared.read_ahead(),
        shared.transform.clone(),
        shared.cache.clone(),
    )?;
    let sequence = match &source {
        previous::copy_capture::AdmittedCopy::Pair(pair) => pair.selected().sequence(),
        previous::copy_capture::AdmittedCopy::View(view) => view.read().sequence(),
    };
    // Copies are internal readonly transactions for C reader accounting too.
    // Keep the existing TLS/NOTLS guard through final publication/FD output;
    // every Result/unwind exit releases active authority, but not high water.
    let mut reader_lease = None;
    let _reader_slot = reader_slots::setup(
        shared,
        env.flags() & 0x200000 != 0,
        env.flags() & 0x400000 != 0,
        sequence,
        &mut reader_lease,
    )?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    let source_realm = {
        let read = match &source {
            previous::copy_capture::AdmittedCopy::Pair(pair) => pair.selected(),
            previous::copy_capture::AdmittedCopy::View(view) => view.read(),
        };
        let m = read.file_manifest().map_err(code)?;
        (m.uuid, m.epoch)
    };
    let empty = match &source {
        previous::copy_capture::AdmittedCopy::View(view) => empty::logical_empty(
            view.read(),
            names::external_name_limit(shared),
            shared.catalog_terminator,
        )?,
        previous::copy_capture::AdmittedCopy::Pair(_) => false,
    };
    let staging = Staging::absent(parent)?;
    match source {
        previous::copy_capture::AdmittedCopy::Pair(pair) => {
            ulmdb::backup::export_file_pair(&pair, &staging.path).map_err(code)?;
        }
        previous::copy_capture::AdmittedCopy::View(view) if empty => {
            let mode = match view.read().file_manifest().map_err(code)?.format_minor {
                2 => ulmdb::PublicationMode::Legacy,
                3 => ulmdb::PublicationMode::Cohort,
                4 => ulmdb::PublicationMode::Indexed,
                _ => return Err(libc::ENOTSUP),
            };
            if shared.external.is_some() {
                ulmdb::backup::create_empty_file_copy_externally_synchronized(
                    &staging.path,
                    mode,
                    view.read().file_manifest().map_err(code)?.page_size,
                    shared.transform.clone(),
                    ulmdb_storage::access::FileAccessContext::open(view.directory())
                        .map_err(code)?
                        .policy(),
                )
                .map_err(code)?;
            } else if shared.transform.is_some() {
                drop(
                    Environment::create_with_file_access(
                        &staging.path,
                        mode,
                        view.read().file_manifest().map_err(code)?.page_size,
                        Default::default(),
                        shared.read_ahead(),
                        shared.transform.clone(),
                        ulmdb_storage::access::FileAccessContext::open(view.directory())
                            .map_err(code)?
                            .policy(),
                    )
                    .map_err(code)?,
                );
            } else {
                ulmdb::backup::create_empty_file_copy_with_access(
                    &staging.path,
                    mode,
                    view.read().file_manifest().map_err(code)?.page_size,
                    None,
                    ulmdb_storage::access::FileAccessContext::open(view.directory())
                        .map_err(code)?
                        .policy(),
                )
                .map_err(code)?;
            }
        }
        previous::copy_capture::AdmittedCopy::View(view) => {
            ulmdb::backup::compact_file_copy(&view, &staging.path).map_err(code)?;
        }
    }
    adapter_metadata(&shared.directory, &staging.path)?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let selected =
            ulmdb_storage::read_current_with_transform(&staging.path, shared.transform.clone())
                .map_err(code)?;
        ulmdb::FixedMap::export_layout(
            &shared.directory,
            &staging.path,
            source_realm.0,
            source_realm.1,
            &selected,
            flags & 1 != 0,
        )
        .map_err(code)?;
    }
    consume(staging)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_copy(env: *mut MDB_env, path: *const c_char) -> c_int {
    unsafe { mdb_env_copy2(env, path, 0) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_copy2(
    env: *mut MDB_env,
    path: *const c_char,
    flags: u32,
) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        let target = unsafe { filesystem_path(path)? };
        let target = target.as_ref();
        check_target(target)?;
        snapshot(env, parent(target), flags, |staging| {
            staging.publish(target)
        })
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_copyfd(env: *mut MDB_env, fd: FileHandle) -> c_int {
    unsafe { mdb_env_copyfd2(env, fd, 0) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_copyfd2(env: *mut MDB_env, fd: FileHandle, flags: u32) -> c_int {
    boundary(|| {
        let env = unsafe { get_ref(env)? };
        // Duplicate the native descriptor/HANDLE and share stream position,
        // retaining caller ownership on every success/error path.
        let mut output = ulmdb_platform::duplicate_file_handle(fd).map_err(|e| code(e.into()))?;
        snapshot(env, &std::env::temp_dir(), flags, |staging| {
            let shared = env.shared.as_ref().ok_or(libc::EINVAL)?;
            archive::write_archive_v2_with_cache_options(
                &staging.path,
                &mut output,
                shared.transform.clone(),
                shared.external.is_some(),
                shared.cache.clone(),
            )
        })
    })
}
