//! RAII process coordination for the compatibility adapter. Ordinary readers
//! map immutable commits; only writers acquire the separate writer lease.
use super::*;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

fn writable_file(path: &Path) -> FfiResult<File> {
    let access = ulmdb_storage::access::context_for_entry(path).map_err(code)?;
    access.validate_parent(path).map_err(code)?;
    let file = access.options()
        .map_err(code)?
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| code(e.into()))?;
    access.revalidate_file(&file).map_err(code)?;
    Ok(file)
}

#[cfg(unix)]
fn flock(file: &File, operation: c_int) -> FfiResult<()> {
    use std::os::fd::AsRawFd;
    loop {
        // SAFETY: fd stays owned by File; flock does not retain pointers.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.raw_os_error().unwrap_or(libc::EIO));
        }
    }
}
#[cfg(unix)]
const LOCK_EX: c_int = libc::LOCK_EX;
#[cfg(unix)]
const LOCK_SH: c_int = libc::LOCK_SH;
#[cfg(unix)]
const LOCK_UN: c_int = libc::LOCK_UN;
#[cfg(unix)]
const LOCK_NB: c_int = libc::LOCK_NB;
#[cfg(windows)]
const LOCK_EX: c_int = 2;
#[cfg(windows)]
const LOCK_SH: c_int = 1;
#[cfg(windows)]
const LOCK_UN: c_int = 8;
#[cfg(windows)]
const LOCK_NB: c_int = 4;
#[cfg(windows)]
fn flock(file: &File, operation: c_int) -> FfiResult<()> {
    // Unlike advisory flock, Windows locks are mandatory. This reserved byte
    // never overlaps the reader registry/slot contents we must inspect.
    const OFFSET: u64 = 1u64 << 62;
    if operation & !(LOCK_EX | LOCK_SH | LOCK_UN | LOCK_NB) != 0 {
        return Err(libc::EINVAL);
    }
    // Explicit conversion matches flock's non-atomic conversion semantics.
    // ENV conversions are serialized by the already-required ENV-GATE.
    if let Err(e) = ulmdb_platform::unlock_file_range(file, OFFSET, 1) {
        if e.raw_os_error() != Some(158) {
            return Err(code(e.into()));
        }
    }
    if operation == LOCK_UN {
        return Ok(());
    }
    let mode = match operation & !LOCK_NB {
        LOCK_EX => ulmdb_platform::LockMode::Exclusive,
        LOCK_SH => ulmdb_platform::LockMode::Shared,
        _ => return Err(libc::EINVAL),
    };
    ulmdb_platform::lock_file_range(file, mode, OFFSET, 1, operation & LOCK_NB == 0).map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            libc::EWOULDBLOCK
        } else {
            code(e.into())
        }
    })
}
fn would_block(error: c_int) -> bool {
    error == libc::EWOULDBLOCK || error == libc::EAGAIN
}

pub(super) struct WriterLease {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    _lease: ulmdb_storage::coordination::WriterLease,
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    _file: File,
}
impl WriterLease {
    pub(super) fn try_acquire(directory: &Path) -> FfiResult<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            Ok(Self {
                _lease: ulmdb_storage::coordination::WriterLease::try_acquire(directory)
                    .map_err(code)?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let file = writable_file(&directory.join("LMDB-WRITER"))?;
            flock(&file, LOCK_EX | LOCK_NB).map_err(|error| {
                if would_block(error) {
                    libc::EBUSY
                } else {
                    error
                }
            })?;
            Ok(Self { _file: file })
        }
    }
    pub(super) fn acquire(directory: &Path) -> FfiResult<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            Ok(Self {
                _lease: ulmdb_storage::coordination::WriterLease::acquire(directory)
                    .map_err(code)?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let file = writable_file(&directory.join("LMDB-WRITER"))?;
            flock(&file, LOCK_EX)?;
            Ok(Self { _file: file })
        }
    }
}

/// A shared environment-lifetime lease. Exclusive first-open initialization
/// chooses the shared reader capacity; later opens retain that capacity until
/// every environment/transaction capability in every process has closed.
pub(super) struct EnvironmentLease {
    _file: File,
    creator: u32,
    directory: PathBuf,
    exclusive: std::sync::atomic::AtomicBool,
    fresh: bool,
}
impl EnvironmentLease {
    /// Return retained EX for a genuine first opener. A SH waiter must never
    /// retry EX when it wakes: its preexisting visibility session remains live.
    pub(super) fn begin(
        directory: &Path,
        requested: u32,
        writable_previous: bool,
    ) -> FfiResult<(Self, u32)> {
        if requested == 0 {
            return Err(libc::EINVAL);
        }
        // Serialize the EX-to-SH conversion: flock lock conversions need not
        // be atomic, so another initializer must not enter during downgrade.
        let gate = EnvironmentGate::acquire(directory)?;
        let file = writable_file(&directory.join("LMDB-ENV"))?;
        let config = directory.join("LMDB-READERS");
        let (limit, fresh) = match flock(&file, LOCK_EX | LOCK_NB) {
            Ok(()) => {
                // Match LMDB: an exclusive opener can grow existing reader
                // capacity, but closing all environments does not shrink it.
                let existing = read_limit(&config)?;
                let limit = existing.unwrap_or(requested).max(requested);
                if existing != Some(limit) {
                    publish_limit(directory, &config, limit)?;
                }
                // LMDB initializes its reader table on the first exclusive
                // open, even when the lock-file capacity is retained. No
                // environment/transaction can own a slot while we hold EX.
                reset_reader_high_water(directory)?;
                (limit, true)
            }
            Err(error) if would_block(error) => {
                if writable_previous {
                    return Err(-30771);
                }
                // Never hold GATE while waiting behind retained previous EX.
                // The publisher needs GATE for its non-atomic EX->SH downgrade.
                drop(gate);
                flock(&file, LOCK_SH)?;
                let _gate = EnvironmentGate::acquire(directory)?;
                (read_limit(&config)?.ok_or(-30796)?, false)
            }
            Err(error) => return Err(error),
        };
        Ok((
            Self {
                _file: file,
                creator: std::process::id(),
                directory: directory.to_path_buf(),
                exclusive: std::sync::atomic::AtomicBool::new(fresh),
                fresh,
            },
            limit,
        ))
    }
    pub(super) fn fresh(&self) -> bool {
        self.fresh
    }
    pub(super) fn bound_to(&self, directory: &Path) -> FfiResult<()> {
        if self.creator != std::process::id() {
            return Err(libc::EINVAL);
        }
        if self.directory.canonicalize().map_err(|e| code(e.into()))?
            != directory.canonicalize().map_err(|e| code(e.into()))?
        {
            return Err(-30796);
        }
        let named = ulmdb_platform::path_identity(&directory.join("LMDB-ENV"))
            .map_err(|e| code(e.into()))?;
        let owned = ulmdb_platform::file_identity(&self._file).map_err(|e| code(e.into()))?;
        if !named.is_file()
            || named.reparse
            || !owned.is_file()
            || owned.links != 1
            || !named.same_object(&owned)
        {
            return Err(-30796);
        }
        Ok(())
    }
    pub(super) fn can_initialize(&self, directory: &Path) -> FfiResult<bool> {
        self.bound_to(directory)?;
        Ok(self.fresh && self.exclusive.load(std::sync::atomic::Ordering::Acquire))
    }
    pub(super) fn downgrade(&self) -> FfiResult<()> {
        if self.creator != std::process::id() {
            return Err(libc::EINVAL);
        }
        if self.exclusive.load(std::sync::atomic::Ordering::Acquire) {
            let _gate = EnvironmentGate::acquire(&self.directory)?;
            self.bound_to(&self.directory)?;
            flock(&self._file, LOCK_SH)?;
            self.exclusive
                .store(false, std::sync::atomic::Ordering::Release);
        }
        Ok(())
    }
}
impl Drop for EnvironmentLease {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let _ = flock(&self._file, LOCK_UN);
        }
    }
}
struct EnvironmentGate {
    file: File,
    creator: u32,
}
impl EnvironmentGate {
    fn acquire(directory: &Path) -> FfiResult<Self> {
        let file = writable_file(&directory.join("LMDB-ENV-GATE"))?;
        Self::check(directory, &file)?;
        flock(&file, LOCK_EX)?;
        let gate = Self {
            file,
            creator: std::process::id(),
        };
        Self::check(directory, &gate.file)?;
        Ok(gate)
    }
    fn check(directory: &Path, file: &File) -> FfiResult<()> {
        let named = ulmdb_platform::path_identity(&directory.join("LMDB-ENV-GATE"))
            .map_err(|e| code(e.into()))?;
        let owned = ulmdb_platform::file_identity(file).map_err(|e| code(e.into()))?;
        if !named.is_file()
            || named.reparse
            || !owned.is_file()
            || owned.links != 1
            || owned.len() != 0
            || !named.same_object(&owned)
        {
            return Err(-30796);
        }
        Ok(())
    }
}
impl Drop for EnvironmentGate {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let _ = flock(&self.file, LOCK_UN);
        }
    }
}
fn publish_limit(directory: &Path, config: &Path, limit: u32) -> FfiResult<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let (temporary, mut file) = loop {
        let serial = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |serial| {
                serial.checked_add(1)
            })
            .map_err(|_| -30792)?;
        let path = directory.join(format!(".lmdb-readers-{}-{serial}", std::process::id()));
        match ulmdb_storage::access::options_for_entry(&path)
            .map_err(code)?
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(code(error.into())),
        }
    };
    let published = (|| -> std::io::Result<()> {
        file.write_all(&limit.to_le_bytes())?;
        ulmdb_platform::sync_file(&file)?;
        ulmdb_platform::rename_replace(&temporary, &config)
            .map_err(ulmdb_platform::rename_original_error)?;
        ulmdb_platform::sync_path(directory)
    })();
    if published.is_err() {
        // The private file belongs to this attempt. After rename it no longer
        // exists; never remove the selected config on a directory-sync error.
        let _ = std::fs::remove_file(&temporary);
    }
    published.map_err(|error| code(error.into()))
}
fn read_limit(path: &Path) -> FfiResult<Option<u32>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(code(error.into())),
    };
    let mut bytes = Vec::with_capacity(5);
    file.take(5)
        .read_to_end(&mut bytes)
        .map_err(|error| code(error.into()))?;
    let bytes: [u8; 4] = bytes.try_into().map_err(|_| -30796)?;
    let limit = u32::from_le_bytes(bytes);
    if limit == 0 {
        return Err(-30796);
    }
    Ok(Some(limit))
}

fn reader_registry(directory: &Path) -> FfiResult<File> {
    let directory = directory.join("lmdb-readers");
    match ulmdb_storage::access::builder_for_entry(&directory)
        .map_err(code)?
        .create(&directory)
    {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(code(e.into())),
    }
    // This inode is stable: replacing it would split the allocator's lock.
    writable_file(&directory.join("registry.lock"))
}
fn read_high_water(file: &mut File, limit: u32) -> FfiResult<u32> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| code(error.into()))?;
    let mut bytes = Vec::with_capacity(5);
    file.take(5)
        .read_to_end(&mut bytes)
        .map_err(|error| code(error.into()))?;
    let bytes: [u8; 4] = bytes.try_into().map_err(|_| -30796)?;
    let high_water = u32::from_le_bytes(bytes);
    if high_water > limit {
        return Err(-30796);
    }
    Ok(high_water)
}
fn write_high_water(file: &mut File, high_water: u32) -> FfiResult<()> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| code(error.into()))?;
    file.write_all(&high_water.to_le_bytes())
        .map_err(|error| code(error.into()))?;
    file.set_len(4).map_err(|error| code(error.into()))
}
fn reset_reader_high_water(directory: &Path) -> FfiResult<()> {
    let mut registry = reader_registry(directory)?;
    flock(&registry, LOCK_EX)?;
    write_high_water(&mut registry, 0)
}
pub(super) fn reader_high_water(shared: &Shared) -> FfiResult<u32> {
    let mut registry = reader_registry(&shared.directory)?;
    flock(&registry, LOCK_SH)?;
    read_high_water(&mut registry, shared.maxreaders)
}

pub(super) struct ReaderLease {
    file: File,
    _slot: PathBuf,
    creator: u32,
}
impl ReaderLease {
    pub(super) fn acquire(shared: &Shared, sequence: u64) -> FfiResult<Self> {
        let directory = shared.directory.join("lmdb-readers");
        match ulmdb_storage::access::builder_for_entry(&directory)
            .map_err(code)?
            .create(&directory)
        {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(code(e.into())),
        }
        let mut guard = reader_registry(&shared.directory)?;
        flock(&guard, LOCK_EX)?;
        let high_water = read_high_water(&mut guard, shared.maxreaders)?;
        for slot in 0..shared.maxreaders {
            let path = directory.join(format!("{slot:08}.slot"));
            let file = writable_file(&path)?;
            match flock(&file, LOCK_EX | LOCK_NB) {
                Ok(()) => {
                    let mut lease = Self {
                        file,
                        _slot: path,
                        creator: std::process::id(),
                    };
                    lease.update(Some(sequence))?;
                    // Free/reused slots never lower the table high water.
                    // Publish only after the slot has been initialized.
                    if slot >= high_water {
                        write_high_water(&mut guard, slot + 1)?;
                    }
                    return Ok(lease);
                }
                Err(error) if would_block(error) => {}
                Err(error) => return Err(error),
            }
        }
        Err(-30790)
    }
    pub(super) fn update(&mut self, sequence: Option<u64>) -> FfiResult<()> {
        self.file.set_len(0).map_err(|error| code(error.into()))?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|error| code(error.into()))?;
        let process = std::process::id();
        let thread = std::thread::current().id();
        let record = match sequence {
            Some(sequence) => format!("{process} {thread:?} {sequence}\n"),
            None => format!("{process} {thread:?} -\n"),
        };
        self.file
            .write_all(record.as_bytes())
            .map_err(|error| code(error.into()))
    }
}
impl Drop for ReaderLease {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let _ = self.file.set_len(0);
        }
        // Close only this fd: LOCK_UN could unlock the parent's shared open
        // file description if a child inherited it across fork.
    }
}

pub(super) fn readers(shared: &Shared, clean: bool) -> FfiResult<(Vec<String>, c_int)> {
    if shared._environment_lease.is_none() {
        return Ok((Vec::new(), 0));
    }
    let directory = shared.directory.join("lmdb-readers");
    // Inspection must not transiently consume a free slot while an allocator
    // is searching for one, especially when the shared limit is very small.
    let mut guard = reader_registry(&shared.directory)?;
    flock(&guard, LOCK_EX)?;
    let high_water = read_high_water(&mut guard, shared.maxreaders)?;
    let mut rows = Vec::new();
    let mut dead = 0;
    // Old files beyond this session's high water are not reader-table rows.
    // An exclusive reopen reset the table, as LMDB resets mti_numreaders.
    for slot in 0..high_water {
        let path = directory.join(format!("{slot:08}.slot"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| code(error.into()))?;
        match flock(&file, LOCK_EX | LOCK_NB) {
            Ok(()) => {
                if clean && file.metadata().map_err(|error| code(error.into()))?.len() != 0 {
                    file.set_len(0).map_err(|error| code(error.into()))?;
                    dead += 1;
                }
            }
            Err(error) if would_block(error) => {
                let mut row = String::new();
                file.take(1024)
                    .read_to_string(&mut row)
                    .map_err(|error| code(error.into()))?;
                rows.push(row);
            }
            Err(error) => return Err(error),
        }
    }
    Ok((rows, dead))
}

/// An exclusive environment session for rollback. The gate remains locked
/// through downgrade because flock conversions may temporarily release SH.
enum RollbackFile<'a> {
    Environment(&'a File),
    Manual(File),
}
impl RollbackFile<'_> {
    fn file(&self) -> &File {
        match self {
            Self::Environment(file) => file,
            Self::Manual(file) => file,
        }
    }
}
pub(super) struct RollbackLease<'a> {
    file: RollbackFile<'a>,
    _gate: File,
    creator: u32,
}
impl<'a> RollbackLease<'a> {
    pub(super) fn acquire(
        directory: &Path,
        environment: Option<&'a EnvironmentLease>,
    ) -> FfiResult<Self> {
        if environment.is_some_and(|lease| lease.creator != std::process::id()) {
            return Err(libc::EINVAL);
        }
        let gate = writable_file(&directory.join("LMDB-ENV-GATE"))?;
        flock(&gate, LOCK_EX | LOCK_NB).map_err(|error| {
            if would_block(error) {
                libc::EBUSY
            } else {
                error
            }
        })?;
        // NOLOCK users already promise manual exclusive use. Still reject a
        // known lockful peer, without claiming visibility of other NOLOCK users.
        let file = match environment {
            Some(environment) => RollbackFile::Environment(&environment._file),
            None => RollbackFile::Manual(writable_file(&directory.join("LMDB-ENV"))?),
        };
        match flock(file.file(), LOCK_EX | LOCK_NB) {
            Ok(()) => Ok(Self {
                file,
                _gate: gate,
                creator: std::process::id(),
            }),
            Err(error) => {
                if matches!(file, RollbackFile::Environment(_)) {
                    // flock conversions may have dropped our shared lease.
                    flock(file.file(), LOCK_SH)?;
                }
                Err(if would_block(error) {
                    libc::EBUSY
                } else {
                    error
                })
            }
        }
    }
}
impl Drop for RollbackLease<'_> {
    fn drop(&mut self) {
        if self.creator == std::process::id()
            && let RollbackFile::Environment(file) = &self.file
        {
            // SH cannot block while this open description owns EX. The gate
            // excludes all cooperating competing conversions during downgrade.
            let _ = flock(file, LOCK_SH);
        }
        // Manual leases release by close only; no inherited LOCK_UN after fork.
    }
}
