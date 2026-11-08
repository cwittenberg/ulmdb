//! Replay caches carry no authority. Reopen unlinks stale names under the fence;
//! escaped mapped readers retain their original inode and immutable bytes.
use crate::Result;
use std::path::Path;
pub(super) fn remove_old_names(directory: &Path) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(body) = name
            .strip_prefix("image-")
            .and_then(|s| s.strip_suffix(".cache"))
        else {
            continue;
        };
        let Some((incarnation, id)) = body.split_once('-') else {
            continue;
        };
        if incarnation.len() == 32
            && incarnation.bytes().all(|b| b.is_ascii_hexdigit())
            && id.parse::<u64>().is_ok()
        {
            // remove_file unlinks a symlink itself; it never follows its target.
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
