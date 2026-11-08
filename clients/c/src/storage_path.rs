//! Logical LMDB paths map to native ULMDB environments. Existing LMDB files
//! are imported by an isolated tool; this module never loads a foreign engine.
use super::*;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MARKER: &[u8] = b"ULMDB-LMDB-ADAPTER\n";
pub(super) fn location(path: &Path, flags: u32) -> PathBuf {
    let no_subdir = flags & 0x4000 != 0;
    if no_subdir {
        let mut value = path.as_os_str().to_owned();
        value.push(".ulmdb");
        PathBuf::from(value)
    } else if path.join(".ulmdb/CURRENT").is_file() || path.join("data.mdb").is_file() {
        path.join(".ulmdb")
    } else {
        path.to_path_buf()
    }
}
pub(super) fn native_path(path: &Path, flags: u32) -> FfiResult<PathBuf> {
    native_path_with_transform(path, flags, None)
}
pub(super) fn native_path_with_transform(
    path: &Path,
    flags: u32,
    transform: Transform,
) -> FfiResult<PathBuf> {
    native_path_with_cache_options(path, flags, transform, None)
}
pub(super) fn native_path_with_cache_options(
    path: &Path,
    flags: u32,
    transform: Transform,
    cache: chunks::Options,
) -> FfiResult<PathBuf> {
    let no_subdir = flags & 0x4000 != 0;
    let native = location(path, flags);
    if native.join("CURRENT").is_file() {
        return Ok(native);
    }
    if no_subdir && path.is_file() {
        let mut magic = [0u8; 8];
        if File::open(path)
            .and_then(|mut f| f.read_exact(&mut magic))
            .is_ok()
            && &magic == copy::ARCHIVE_MAGIC
        {
            copy::restore_archive_with_cache_options(
                path,
                &native,
                transform,
                flags & 0x400000 != 0 && cfg!(target_os = "linux"),
                cache,
            )?;
            return Ok(native);
        }
    }
    let legacy = if no_subdir && path.is_file() {
        let mut header = Vec::with_capacity(MARKER.len());
        File::open(path)
            .and_then(|f| f.take(MARKER.len() as u64).read_to_end(&mut header))
            .map_err(|e| code(e.into()))?;
        if header == MARKER
            || (!header.is_empty() && path.metadata().map_err(|e| code(e.into()))?.len() < 32)
        {
            // A marker without its native sidecar, or a truncated database,
            // must never be turned into an apparently empty environment.
            return Err(-30793);
        }
        !header.is_empty() && header != MARKER
    } else {
        !no_subdir
            && path.join("data.mdb").is_file()
            && path
                .join("data.mdb")
                .metadata()
                .map_err(|e| code(e.into()))?
                .len()
                != 0
    };
    if legacy {
        let migrator = migrator_path().ok_or(libc::ENOENT)?;
        let status = std::process::Command::new(migrator)
            .arg(path)
            .arg(&native)
            .arg("--quiet")
            .status()
            .map_err(|e| code(e.into()))?;
        if !status.success() && !native.join("CURRENT").is_file() {
            return Err(-30793);
        }
    }
    Ok(native)
}
fn migrator_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ULMDB_MIGRATOR") {
        return Some(PathBuf::from(path));
    }
    // Resolve the actual loaded module on POSIX/Windows, independently of cwd.
    // SAFETY: the adapter must remain loaded while this C call is executing.
    if let Ok(module) = unsafe {
        ulmdb_platform::module_path(super::environment::mdb_env_open as *const () as *const c_void)
    } {
        let binary = if cfg!(windows) {
            "ulmdb-migrate.exe"
        } else {
            "ulmdb-migrate"
        };
        let candidate = module.parent()?.join("../bin").join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let candidate =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/lmdb-ulmdb/bin/ulmdb-migrate");
    candidate.is_file().then_some(candidate)
}
pub(super) fn publish_nosubdir_marker(path: &Path, flags: u32, mode: FileMode) -> FfiResult<()> {
    if flags & 0x4000 == 0
        || (path.exists() && path.metadata().map_err(|e| code(e.into()))?.len() != 0)
    {
        return Ok(());
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    options.mode(requested_file_mode(mode));
    #[cfg(windows)]
    {
        let _ = mode;
        options.custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS);
    }
    let mut file = options.open(path).map_err(|e| code(e.into()))?;
    #[cfg(windows)]
    {
        if !ulmdb_platform::file_identity(&file)
            .map_err(|e| code(e.into()))?
            .is_file()
        {
            return Err(libc::EINVAL);
        }
    }
    file.write_all(MARKER).map_err(|e| code(e.into()))?;
    ulmdb_platform::sync_file(&file).map_err(|e| code(e.into()))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ulmdb_platform::open_directory(parent)
        .and_then(|f| ulmdb_platform::sync_file(&f))
        .map_err(|e| code(e.into()))
}
