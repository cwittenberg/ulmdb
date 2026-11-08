//! Bounded namespace-relative descriptors for cache-owned Native snapshots.
//! Direct AuthenticatedFile(File) callers retain their owning descriptor API.
use super::{Error, File, Result, check_process};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    collections::BTreeMap,
    fs,
    mem::ManuallyDrop,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) struct Pool {
    creator: u32,
    limit: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
    directories: ManuallyDrop<Mutex<BTreeMap<PathBuf, Weak<Directory>>>>,
}
struct Permit {
    pool: Arc<Pool>,
}
impl Pool {
    pub(super) fn new(limit: usize) -> Result<Arc<Self>> {
        if !(3..=65536).contains(&limit) {
            return Err(Error::InvalidInput("chunk source descriptor limit"));
        }
        Ok(Arc::new(Self {
            creator: std::process::id(),
            limit,
            used: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            directories: ManuallyDrop::new(Mutex::new(BTreeMap::new())),
        }))
    }
    fn reserve(self: &Arc<Self>) -> Result<Permit> {
        check_process(self.creator)?;
        let previous = self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(1).filter(|n| *n <= self.limit)
            })
            .map_err(|_| Error::LimitExceeded)?;
        self.peak.fetch_max(previous + 1, Ordering::Relaxed);
        Ok(Permit { pool: self.clone() })
    }
    pub(super) fn stats(&self) -> Result<(usize, usize, usize)> {
        check_process(self.creator)?;
        Ok((
            self.used.load(Ordering::Acquire),
            self.peak.load(Ordering::Relaxed),
            self.limit,
        ))
    }
    pub(super) fn directory(self: &Arc<Self>, path: &Path) -> Result<Arc<Directory>> {
        check_process(self.creator)?;
        let named = fs::symlink_metadata(path)?;
        if !named.is_dir() || named.file_type().is_symlink() {
            return Err(Error::Corrupt("chunk source directory type"));
        }
        let path = path.canonicalize()?;
        let prior = {
            self.directories
                .lock()
                .map_err(|_| Error::Busy)?
                .get(&path)
                .and_then(Weak::upgrade)
        };
        if let Some(prior) = prior {
            prior.check()?;
            return Ok(prior);
        }
        let permit = self.reserve()?;
        let file = ulmdb_platform::open_directory(&path)?;
        let meta = ulmdb_platform::file_identity(&file)?;
        let directory = Arc::new(Directory {
            creator: self.creator,
            path: path.clone(),
            file,
            _permit: permit,
            device: meta.volume,
            inode: meta.file_id,
            pool: self.clone(),
            access: crate::access::FileAccessContext::open(&path)?,
        });
        directory.check()?;
        let mut registry = self.directories.lock().map_err(|_| Error::Busy)?;
        registry.retain(|_, entry| entry.strong_count() != 0);
        if let Some(prior) = registry.get(&path).and_then(Weak::upgrade) {
            drop(registry);
            prior.check()?;
            return Ok(prior);
        }
        registry.insert(path, Arc::downgrade(&directory));
        drop(registry);
        Ok(directory)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if self.pool.creator == std::process::id() {
            self.pool.used.fetch_sub(1, Ordering::AcqRel);
        }
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            // SAFETY: the creator exclusively owns final destruction. A fork child
            // must not walk a registry another vanished parent thread was mutating.
            #[allow(unsafe_code)]
            unsafe {
                ManuallyDrop::drop(&mut self.directories)
            }
        }
    }
}

pub(crate) struct Directory {
    creator: u32,
    path: PathBuf,
    file: File,
    _permit: Permit,
    device: u64,
    inode: u128,
    pool: Arc<Pool>,
    access: crate::access::FileAccessContext,
}
pub(super) struct Opened {
    pub(super) file: File,
    _permit: Permit,
}
impl Directory {
    pub(super) fn check(&self) -> Result<()> {
        check_process(self.creator)?;
        #[cfg(unix)]
        {
            let named = fs::symlink_metadata(&self.path)?;
            let actual = self.file.metadata()?;
            if !named.is_dir() || named.file_type().is_symlink() || !actual.is_dir()
                || (named.dev(), u128::from(named.ino())) != (self.device, self.inode)
                || (actual.dev(), u128::from(actual.ino())) != (self.device, self.inode)
            {
                return Err(Error::Corrupt("chunk source directory replaced"));
            }
        }
        #[cfg(windows)]
        {
            // A bounded transient descriptor authenticates the named directory;
            // Windows std Metadata has no stable full-width FileId API.
            let _permit = self.pool.reserve()?;
            let named = ulmdb_platform::open_directory(&self.path)?;
            let named = ulmdb_platform::file_identity(&named)?;
            let actual = ulmdb_platform::file_identity(&self.file)?;
            if !named.directory || named.reparse || !actual.directory || actual.reparse
                || (named.volume, named.file_id) != (self.device, self.inode)
                || (actual.volume, actual.file_id) != (self.device, self.inode)
            {
                return Err(Error::Corrupt("chunk source directory replaced"));
            }
        }
        Ok(())
    }
    pub(super) fn open(&self, relative: &Path) -> Result<Opened> {
        self.check()?;
        let mut components = relative.components();
        let Some(Component::Normal(parent)) = components.next() else {
            return Err(Error::Corrupt("chunk source relative parent"));
        };
        let Some(Component::Normal(name)) = components.next() else {
            return Err(Error::Corrupt("chunk source relative file"));
        };
        if components.next().is_some() || !(parent == "segments" || parent == "cohorts") {
            return Err(Error::Corrupt("chunk source path escape"));
        }
        let parent_permit = self.pool.reserve()?;
        let parent = ulmdb_platform::open_relative(
            &self.file, parent, ulmdb_platform::RelativeKind::Directory,
        )?;
        let file_permit = self.pool.reserve()?;
        let file = ulmdb_platform::open_relative(
            &parent, name, ulmdb_platform::RelativeKind::File,
        )?;
        self.access.validate_file(&file)?;
        self.check()?;
        drop(parent);
        drop(parent_permit);
        Ok(Opened {
            file,
            _permit: file_permit,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "chunk-file-pool-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&p).unwrap();
        fs::write(p.join("segments/a.seg"), [1; 4096]).unwrap();
        fs::write(p.join("segments/b.seg"), [2; 4096]).unwrap();
        p
    }
    #[test]
    fn permit_exhaustion_preserves_live_file_and_recovers_without_leaks() {
        assert!(
            super::super::ChunkCache::new_with_file_limit(super::super::Limits::default(), 3)
                .is_err()
        );
        assert!(
            super::super::ChunkCache::new_with_file_limit(super::super::Limits::default(), 4)
                .is_ok()
        );
        let path = directory();
        let pool = Pool::new(3).unwrap();
        let dir = pool.directory(&path).unwrap();
        let same = pool.directory(&path).unwrap();
        assert!(Arc::ptr_eq(&dir, &same));
        assert_eq!(pool.stats().unwrap().0, 1);
        let a = dir.open(Path::new("segments/a.seg")).unwrap();
        assert_eq!(pool.stats().unwrap().0, 2);
        assert!(matches!(
            dir.open(Path::new("segments/b.seg")),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(pool.stats().unwrap().0, 2);
        assert_eq!(a.file.metadata().unwrap().len(), 4096);
        drop(a);
        let b = dir.open(Path::new("segments/b.seg")).unwrap();
        drop(b);
        drop(same);
        drop(dir);
        assert_eq!(pool.stats().unwrap().0, 0);
        assert_eq!(pool.stats().unwrap().1, 3);
        fs::remove_dir_all(path).unwrap();
    }
    #[cfg(unix)]
    #[test]
    #[allow(unsafe_code)]
    fn fork_rejects_before_inherited_registry_mutex_and_drop_does_not_walk_it() {
        #[cfg(target_os = "linux")]
        const NAME: &str = "chunk_cache::files::tests::fork_rejects_before_inherited_registry_mutex_and_drop_does_not_walk_it";
        #[cfg(target_os = "linux")]
        if crate::history::tests::run_isolated(NAME) {
            return;
        }
        let path = directory();
        let pool = Pool::new(3).unwrap();
        let dir = pool.directory(&path).unwrap();
        let lock = pool.directories.lock().unwrap();
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            unsafe { libc::alarm(3) };
            let rejected = matches!(pool.directory(&path), Err(Error::InvalidInput(_)))
                && matches!(
                    dir.open(Path::new("segments/a.seg")),
                    Err(Error::InvalidInput(_))
                );
            std::mem::forget(lock);
            drop(dir);
            drop(pool);
            unsafe { libc::_exit(if rejected { 0 } else { 1 }) }
        }
        drop(lock);
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
        drop(dir);
        assert_eq!(pool.stats().unwrap().0, 0);
        fs::remove_dir_all(path).unwrap();
        #[cfg(target_os = "linux")]
        crate::history::tests::worker_completed(NAME);
    }
}
