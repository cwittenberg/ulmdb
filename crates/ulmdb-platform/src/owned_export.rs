//! Private, descriptor-rooted output for an independently published directory.
//! No write or cleanup operation resolves a staging pathname after creation.
use crate::{FileIdentity, RelativeKind};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    path::{Path, PathBuf},
};
#[cfg(unix)]
mod unix;
#[cfg(windows)]
use crate::windows::owned_export as sys;
#[cfg(unix)]
use unix as sys;

struct Entry {
    name: OsString,
    file: File,
    directory: bool,
}
/// A newly created owner-only outer stage containing one private `image`.
/// Writes are exclusive, bounded and relative to its genuine held directory.
/// The object is not a serving snapshot, lock, graph certificate or source pin.
pub struct OwnedExport {
    parent: File,
    parent_path: PathBuf,
    requested_parent: PathBuf,
    target: OsString,
    stage_name: OsString,
    stage: Option<File>,
    image: Option<File>,
    entries: Vec<Entry>,
    creator: u32,
    published: bool,
}
fn identity(file: &File) -> io::Result<FileIdentity> {
    crate::file_identity(file)
}
fn same_named(parent: &File, name: &OsStr, file: &File, directory: bool) -> io::Result<()> {
    let named = crate::open_relative(
        parent,
        name,
        if directory {
            RelativeKind::Directory
        } else {
            RelativeKind::File
        },
    )?;
    let held = identity(file)?;
    let selected = identity(&named)?;
    if !held.same_object(&selected)
        || (directory && !held.is_dir())
        || (!directory && (!held.is_file() || held.links != 1 || selected.links != 1))
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(())
}
fn check_name(name: &OsStr) -> io::Result<()> {
    if !matches!(
        Path::new(name).components().next(),
        Some(std::path::Component::Normal(_))
    ) || Path::new(name).components().count() != 1
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(())
}
impl OwnedExport {
    pub fn create(target: &Path, nonce: u128) -> io::Result<Self> {
        let target_name = target
            .file_name()
            .ok_or(io::ErrorKind::InvalidInput)?
            .to_owned();
        check_name(&target_name)?;
        let requested_parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_owned();
        let parent_path = std::fs::canonicalize(&requested_parent)?;
        let parent = crate::open_directory(&parent_path)?;
        if !identity(&parent)?.same_object(&crate::path_identity(&parent_path)?) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        match crate::open_relative(&parent, &target_name, RelativeKind::Directory) {
            Ok(_) => return Err(io::ErrorKind::AlreadyExists.into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        let stage_name: OsString = format!(".resident-export-{nonce:032x}").into();
        let stage = sys::create(&parent, &stage_name, true)?;
        let image = match sys::create(&stage, OsStr::new("image"), true) {
            Ok(f) => f,
            Err(e) => {
                let _ = sys::remove(&parent, &stage_name, &stage, true);
                return Err(e);
            }
        };
        Ok(Self {
            parent,
            parent_path,
            requested_parent,
            target: target_name,
            stage_name,
            stage: Some(stage),
            image: Some(image),
            entries: Vec::new(),
            creator: std::process::id(),
            published: false,
        })
    }
    fn check_process(&self) -> io::Result<()> {
        if self.creator != std::process::id() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(())
    }
    fn check(&self) -> io::Result<()> {
        self.check_process()?;
        if std::fs::canonicalize(&self.requested_parent)? != self.parent_path
            || !identity(&self.parent)?.same_object(&crate::path_identity(&self.parent_path)?)
        {
            return Err(io::ErrorKind::InvalidData.into());
        }
        same_named(
            &self.parent,
            &self.stage_name,
            self.stage.as_ref().unwrap(),
            true,
        )?;
        if !self.published {
            same_named(
                self.stage.as_ref().unwrap(),
                OsStr::new("image"),
                self.image.as_ref().unwrap(),
                true,
            )?;
        }
        Ok(())
    }
    fn create_entry(&mut self, name: &OsStr, directory: bool) -> io::Result<File> {
        self.check()?;
        check_name(name)?;
        if self.published || self.entries.len() >= 64 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| io::ErrorKind::OutOfMemory)?;
        let file = sys::create(self.image.as_ref().unwrap(), name, directory)?;
        let output = match file.try_clone() {
            Ok(f) => f,
            Err(e) => {
                let _ = sys::remove(self.image.as_ref().unwrap(), name, &file, directory);
                return Err(e);
            }
        };
        self.entries.push(Entry {
            name: name.to_owned(),
            file,
            directory,
        });
        Ok(output)
    }
    pub fn create_file(&mut self, name: &OsStr) -> io::Result<File> {
        self.create_entry(name, false)
    }
    pub fn create_directory(&mut self, name: &OsStr) -> io::Result<()> {
        self.create_entry(name, true).map(drop)
    }
    /// All data is already fenced by its writer. This also fences the exact
    /// layout/namespace objects; no pathname file is substituted at a fence.
    pub fn sync(&self) -> io::Result<()> {
        self.check()?;
        for e in &self.entries {
            same_named(self.image.as_ref().unwrap(), &e.name, &e.file, e.directory)?;
            if e.directory {
                crate::sync_directory_file(&e.file)?;
            } else {
                crate::sync_file(&e.file)?;
            }
        }
        crate::sync_directory_file(self.image.as_ref().unwrap())?;
        crate::sync_directory_file(self.stage.as_ref().unwrap())?;
        self.check()
    }
    /// Consumes this output. Errors after selection are tagged uncertain and
    /// never cause deletion of the published image or its files.
    pub fn publish(mut self) -> io::Result<()> {
        self.sync()?;
        if let Err(e) = install(
            self.stage.as_ref().unwrap(),
            self.image.as_ref().unwrap(),
            &self.parent,
            &self.target,
        ) {
            if crate::rename_outcome_unknown(&e) {
                self.published = true;
            }
            return Err(e);
        }
        self.published = true;
        let result = (|| {
            self.check()?;
            same_named(
                &self.parent,
                &self.target,
                self.image.as_ref().unwrap(),
                true,
            )?;
            crate::sync_directory_file(&self.parent)?;
            crate::sync_directory_file(self.stage.as_ref().unwrap())?;
            self.check()?;
            same_named(
                &self.parent,
                &self.target,
                self.image.as_ref().unwrap(),
                true,
            )
        })();
        result.map_err(crate::uncertain_rename)
    }
}
/// Install only the `image` child of a caller-owned private directory. A failure
/// after the kernel rename is tagged uncertain. Never replaces any target.
pub fn install(source: &File, image: &File, parent: &File, name: &OsStr) -> io::Result<()> {
    check_name(name)?;
    same_named(source, OsStr::new("image"), image, true)?;
    sys::rename(source, image, parent, name)?;
    (|| {
        same_named(parent, name, image, true)?;
        crate::sync_directory_file(parent)?;
        crate::sync_directory_file(source)
    })()
    .map_err(crate::uncertain_rename)
}
impl Drop for OwnedExport {
    fn drop(&mut self) {
        if self.creator != std::process::id() {
            return;
        }
        let stage = self.stage.take().unwrap();
        let image = self.image.take().unwrap();
        if !self.published {
            while let Some(e) = self.entries.pop() {
                if same_named(&image, &e.name, &e.file, e.directory).is_ok() {
                    let _ = sys::remove(&image, &e.name, &e.file, e.directory);
                }
            }
            if same_named(&stage, OsStr::new("image"), &image, true).is_ok() {
                let _ = sys::remove(&stage, OsStr::new("image"), &image, true);
            }
        }
        drop(image);
        if same_named(&self.parent, &self.stage_name, &stage, true).is_ok() {
            let _ = sys::remove(&self.parent, &self.stage_name, &stage, true);
        }
    }
}

#[cfg(test)]
mod tests;
