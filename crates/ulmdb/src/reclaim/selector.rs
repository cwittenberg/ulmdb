//! Bounded selector observation shared by platform maintenance schedulers.
//! Opening a selector does not itself grant namespace or deletion authority.
use crate::{Error, Result};
use std::{fs::OpenOptions, io::Read, path::Path};

pub(crate) fn selector(directory: &Path, name: &str, optional: bool) -> Result<Option<Vec<u8>>> {
    let path = directory.join(name);
    let before = match ulmdb_platform::path_identity(&path) {
        Ok(x) => x,
        Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !before.is_file() || before.length > 176 {
        return Err(Error::Corrupt("invalid reclaim selector"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS);
    }
    let mut file = options.open(&path)?;
    ulmdb_storage::access::validate_entry(&path, &file)?;
    let mut bytes = Vec::with_capacity(177);
    (&mut file).take(177).read_to_end(&mut bytes)?;
    let held = ulmdb_platform::file_identity(&file)?;
    let after = ulmdb_platform::path_identity(&path)?;
    if bytes.len() > 176
        || !after.is_file()
        || !before.same_object(&held)
        || !after.same_object(&held)
    {
        return Err(Error::Corrupt("reclaim selector replaced"));
    }
    Ok(Some(bytes))
}
