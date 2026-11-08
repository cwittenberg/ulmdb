//! Bounded, read-only directory discovery for the explorer's file picker.
//!
//! A database badge is a layout hint, not validation. Opening is always left
//! to the read-only reader; discovering a legacy LMDB file never imports it.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Parent,
    Directory,
    Database,
    File,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
}

pub struct Browser {
    pub directory: PathBuf,
    pub entries: Vec<Entry>,
    /// The directory has more entries than this view can retain.
    pub truncated: bool,
}

impl Browser {
    /// Start at a directory, or at the parent of a selected file.
    pub fn new(path: &Path) -> io::Result<Self> {
        let path = fs::canonicalize(path)?;
        let directory = if path.is_dir() {
            path
        } else {
            path.parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no parent"))?
                .to_path_buf()
        };
        let mut browser = Self {
            directory,
            entries: Vec::new(),
            truncated: false,
        };
        browser.reload()?;
        Ok(browser)
    }

    /// Refresh only this directory. A failed read leaves the old view intact.
    pub fn reload(&mut self) -> io::Result<()> {
        let (mut entries, truncated) = directory_entries(&self.directory, MAX_ENTRIES)?;
        entries.sort_by_cached_key(|entry| {
            (
                matches!(entry.kind, EntryKind::File),
                entry.name.to_lowercase(),
                entry.name.clone(),
            )
        });
        if let Some(parent) = self.directory.parent() {
            entries.insert(
                0,
                Entry {
                    path: parent.to_path_buf(),
                    name: "..".to_owned(),
                    kind: EntryKind::Parent,
                },
            );
        }
        self.entries = entries;
        self.truncated = truncated;
        Ok(())
    }
}

fn directory_entries(directory: &Path, limit: usize) -> io::Result<(Vec<Entry>, bool)> {
    let mut entries = Vec::new();
    let mut children = fs::read_dir(directory)?;
    for child in children.by_ref().take(limit) {
        let child = child?;
        let path = child.path();
        // Follow directory symlinks for navigation, but never descend into them.
        let kind = if path.is_dir() {
            if database_directory(&path) {
                EntryKind::Database
            } else {
                EntryKind::Directory
            }
        } else {
            EntryKind::File
        };
        entries.push(Entry {
            path,
            name: child.file_name().to_string_lossy().into_owned(),
            kind,
        });
    }
    let truncated = children.next().transpose()?.is_some();
    Ok((entries, truncated))
}

fn native_directory(path: &Path) -> bool {
    path.join("CURRENT").is_file()
        || path.join("RESIDENT.wal").is_file()
        || path.join("BACKEND").is_file()
}

fn database_directory(path: &Path) -> bool {
    native_directory(path) || native_directory(&path.join(".ulmdb"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ulmdb-explorer-picker-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn directories_precede_files_and_native_layouts_are_discoverable() {
        let directory = Directory::new();
        for name in ["Zoo", "alpha", "native", "resident", "range", "adapter"] {
            fs::create_dir(directory.0.join(name)).unwrap();
        }
        fs::write(directory.0.join("native/CURRENT"), b"fixture").unwrap();
        fs::write(directory.0.join("resident/RESIDENT.wal"), b"fixture").unwrap();
        fs::write(directory.0.join("range/BACKEND"), b"fixture").unwrap();
        fs::create_dir(directory.0.join("adapter/.ulmdb")).unwrap();
        fs::write(directory.0.join("adapter/.ulmdb/CURRENT"), b"fixture").unwrap();
        fs::write(directory.0.join("B.bin"), b"binary").unwrap();
        fs::write(directory.0.join("a.bin"), b"binary").unwrap();

        let browser = Browser::new(&directory.0).unwrap();
        assert_eq!(browser.entries[0].kind, EntryKind::Parent);
        assert_eq!(browser.entries[0].name, "..");
        let names: Vec<_> = browser
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "..", "adapter", "alpha", "native", "range", "resident", "Zoo", "a.bin", "B.bin"
            ]
        );
        for name in ["adapter", "native", "range", "resident"] {
            assert_eq!(
                browser
                    .entries
                    .iter()
                    .find(|entry| entry.name == name)
                    .unwrap()
                    .kind,
                EntryKind::Database
            );
        }
        assert!(!browser.truncated);
    }

    #[test]
    fn a_file_starts_in_its_parent_and_reload_discovers_changes() {
        let directory = Directory::new();
        let file = directory.0.join("data.mdb");
        fs::write(&file, b"legacy contents").unwrap();
        let mut browser = Browser::new(&file).unwrap();
        assert_eq!(browser.directory, fs::canonicalize(&directory.0).unwrap());
        assert_eq!(browser.entries[1].kind, EntryKind::File);
        assert!(!directory.0.join(".ulmdb").exists());

        fs::create_dir(directory.0.join("new-directory")).unwrap();
        browser.reload().unwrap();
        assert_eq!(browser.entries[1].name, "new-directory");
        assert_eq!(fs::read(&file).unwrap(), b"legacy contents");

        fs::remove_dir_all(&directory.0).unwrap();
        let old_names: Vec<_> = browser
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        assert!(browser.reload().is_err());
        assert_eq!(
            browser
                .entries
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>(),
            old_names
        );
    }

    #[test]
    fn discovery_limits_entries_and_reports_truncation() {
        let directory = Directory::new();
        for name in ["a", "b", "c"] {
            fs::write(directory.0.join(name), b"").unwrap();
        }
        let (entries, truncated) = directory_entries(&directory.0, 2).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(truncated);
        assert!(!directory_entries(&directory.0, 3).unwrap().1);
        assert!(!directory_entries(&directory.0, 4).unwrap().1);
    }

    #[test]
    fn missing_path_reports_an_error_without_creating_it() {
        let directory = Directory::new();
        let missing = directory.0.join("missing");
        assert!(Browser::new(&missing).is_err());
        assert!(!missing.exists());
    }
}
