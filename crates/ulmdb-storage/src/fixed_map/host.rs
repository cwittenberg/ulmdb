//! Platform file-object observations for placement metadata. These checks do
//! not replace the caller's creator, namespace, writer or publication gates.
use crate::{Error, Result};
use std::{fs::File, fs::OpenOptions, path::Path};
use ulmdb_platform::FileIdentity;

pub(super) type FileToken = (u64, u128, u64);

pub(super) fn token(file: &File) -> Result<FileToken> {
    let identity = ulmdb_platform::file_identity(file)?;
    Ok((identity.volume, identity.file_id, identity.length))
}

pub(super) fn identity(file: &File) -> Result<(u64, u128)> {
    let identity = ulmdb_platform::file_identity(file)?;
    Ok((identity.volume, identity.file_id))
}

pub(super) fn named(path: &Path) -> Result<FileIdentity> {
    Ok(ulmdb_platform::path_identity(path)?)
}

pub(super) fn check_directory(path: &Path, file: &File) -> Result<()> {
    let named = named(path)?;
    let actual = ulmdb_platform::file_identity(file)?;
    if !named.is_dir() || !actual.is_dir() || !named.same_object(&actual) {
        return Err(Error::Corrupt("fixed directory identity changed"));
    }
    Ok(())
}

fn regular_identity(path: &Path, file: &File) -> Result<FileIdentity> {
    let named = named(path)?;
    let actual = ulmdb_platform::file_identity(file)?;
    if !named.is_file()
        || !actual.is_file()
        || !named.same_object(&actual)
        || named.length != actual.length
    {
        return Err(Error::Corrupt("fixed layout inode changed"));
    }
    Ok(actual)
}

pub(super) fn check_regular(path: &Path, file: &File) -> Result<()> {
    regular_identity(path, file).map(|_| ())
}

pub(super) fn directory(path: &Path) -> Result<File> {
    // Windows opens a real directory handle with BACKUP semantics; Unix keeps
    // the existing O_DIRECTORY/O_NOFOLLOW/CLOEXEC open order.
    let file = ulmdb_platform::open_directory(path)?;
    check_directory(path, &file)?;
    Ok(file)
}

pub(super) fn readonly(path: &Path) -> Result<File> {
    open_regular(path, false)
}

pub(super) fn open_regular(path: &Path, write: bool) -> Result<File> {
    // Windows has no O_NONBLOCK for a pipe-data open. Inspect through the
    // metadata-only no-reparse boundary before asking for file data access.
    #[cfg(windows)]
    let before = {
        let before = named(path)?;
        if !before.is_file() {
            return Err(Error::Corrupt("fixed layout object kind"));
        }
        before
    };
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS);
    }
    let file = options.open(path)?;
    let actual = regular_identity(path, &file)?;
    #[cfg(windows)]
    if !before.same_object(&actual) || before.length != actual.length {
        return Err(Error::Corrupt("fixed layout object replaced during open"));
    }
    #[cfg(not(windows))]
    let _ = actual;
    Ok(file)
}
