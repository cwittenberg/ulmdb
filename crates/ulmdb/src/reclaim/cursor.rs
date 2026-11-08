//! Resumable name discovery only. Names never confer deletion authority: every
//! invocation builds fresh root marks under the ordinary maintenance fences.
use super::{
    Candidate, Kind, Marks,
    names::{Selection, temporary},
};
use crate::{Error, Result};
use std::{
    fs,
    mem::ManuallyDrop,
    path::{Path, PathBuf},
};

pub(crate) struct ReclaimCursor {
    creator: u32,
    directory: usize,
    entries: ManuallyDrop<Option<fs::ReadDir>>,
    identity: Option<(u64, u128)>,
}
impl Default for ReclaimCursor {
    fn default() -> Self {
        Self {
            creator: std::process::id(),
            directory: 0,
            entries: ManuallyDrop::new(None),
            identity: None,
        }
    }
}
impl Drop for ReclaimCursor {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            // Do not enter an inherited libc directory stream after another
            // thread forked the process. Move the creator's stream out safely.
            let entries = std::mem::replace(&mut self.entries, ManuallyDrop::new(None));
            drop(ManuallyDrop::into_inner(entries));
        }
    }
}
impl ReclaimCursor {
    fn path(&self, root: &Path) -> PathBuf {
        match self.directory {
            0 => root.join("segments"),
            1 => root.join("commits"),
            2 => root.join("cohorts"),
            3 => root.join("transactions"),
            4 => root.to_owned(),
            5 => root.join("retained"),
            _ => unreachable!(),
        }
    }
    fn advance(&mut self) -> bool {
        *self.entries = None;
        self.identity = None;
        self.directory += 1;
        if self.directory == 6 {
            self.directory = 0;
            true
        } else {
            false
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn collect(
        &mut self,
        root: &Path,
        marks: &Marks,
        output: &mut Vec<Candidate>,
        files: usize,
        names: usize,
        scanned: &mut usize,
    ) -> Result<bool> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited reclaim cursor"));
        }
        while *scanned < names && output.len() < files {
            let path = self.path(root);
            let metadata = match ulmdb_platform::path_identity(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && self.entries.is_none() => {
                    if self.advance() {
                        return Ok(true);
                    }
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            if !metadata.is_dir() {
                return Err(Error::Corrupt("invalid reclaim directory"));
            }
            let identity = (metadata.volume, metadata.file_id);
            if self.identity.is_some_and(|old| old != identity) {
                return Err(Error::Corrupt("reclaim directory replaced"));
            }
            if self.entries.is_none() {
                *self.entries = Some(fs::read_dir(&path)?);
                let after = ulmdb_platform::path_identity(&path)?;
                if !after.is_dir() || (after.volume, after.file_id) != identity {
                    return Err(Error::Corrupt("reclaim directory changed during open"));
                }
                self.identity = Some(identity);
            }
            let Some(entry) = self.entries.as_mut().unwrap().next() else {
                if self.advance() {
                    return Ok(true);
                }
                continue;
            };
            *scanned += 1;
            let entry = entry?;
            let name = entry.file_name();
            let kind = if self.directory >= 4 {
                name.to_str()
                    .filter(|name| temporary(name))
                    .map(|_| Kind::Temporary)
            } else {
                let name = name
                    .to_str()
                    .ok_or(Error::Corrupt("non-UTF8 engine file"))?;
                match self.directory {
                    0 => Selection::Segments(&marks.segments).candidate(name),
                    1 => Selection::Descriptors(&marks.roots).candidate(name),
                    2 => Selection::Cohorts(&marks.cohorts).candidate(name),
                    3 => Selection::Temporary.candidate(name),
                    _ => unreachable!(),
                }
            };
            if let Some(kind) = kind {
                let metadata = fs::symlink_metadata(entry.path())?;
                if !metadata.is_file() {
                    return Err(Error::Corrupt("nonregular reclaim candidate"));
                }
                output.push(Candidate {
                    path: entry.path(),
                    bytes: metadata.len(),
                    kind,
                });
            }
        }
        Ok(false)
    }
}
