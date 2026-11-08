//! Negotiated live read capture. Lease inode identity is authenticated by the
//! WAL header; pre-protocol writers cannot decode it. No process-global state.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
#[cfg(not(windows))]
use std::fs::TryLockError;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb_platform::FileExt;
const NAME: &str = "RESIDENT-CAPTURE";
#[cfg(windows)]
const LOCK_BYTE: u64 = 1 << 62;

/// Wire-family identity, not an address or a narrowed platform file index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CaptureIdentity {
    Unix { device: u64, inode: u64 },
    Windows { volume: u64, file_id: u128 },
}
#[derive(Clone, Copy, Debug)]
pub struct CaptureLimits {
    /// Selected logical tail bytes; fixed196608-byte control area is additional.
    pub max_replay_bytes: u64,
    /// Encoded checkpoint file bytes, before hashing/decoding its contents.
    pub max_checkpoint_bytes: u64,
}
impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_replay_bytes: 256 << 20,
            max_checkpoint_bytes: 384 << 20,
        }
    }
}
impl CaptureLimits {
    pub(super) fn validate(self) -> Result<()> {
        if self.max_replay_bytes > 256 << 20 || self.max_checkpoint_bytes > 512 << 20 {
            return Err(Error::InvalidInput("Resident capture byte limit"));
        }
        Ok(())
    }
}
struct Inner {
    directory: PathBuf,
    _directory: File,
    file: File,
    device: u64,
    inode: u128,
    dir_device: u64,
    dir_inode: u128,
    uuid: [u8; 16],
    creator: u32,
}
#[derive(Clone)]
pub(super) struct CaptureGate {
    inner: Arc<Inner>,
}
pub struct ReadCapture {
    guard: GateGuard,
}
pub(super) struct GateGuard {
    #[cfg(not(windows))]
    file: File,
    #[cfg(windows)]
    lease: ulmdb_platform::RangeLock,
    gate: CaptureGate,
    creator: u32,
}
fn header(uuid: [u8; 16]) -> [u8; 64] {
    let mut b = [0; 64];
    b[..8].copy_from_slice(b"ULMDBRG1");
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[12..16].copy_from_slice(&64u32.to_le_bytes());
    b[16..32].copy_from_slice(&uuid);
    let mut h = Sha256::new();
    h.update(b"ULMDB/resident/capture-gate/v1\0");
    h.update(&b[..32]);
    b[32..].copy_from_slice(&h.finalize());
    b
}
fn regular(file: &File) -> Result<ulmdb_platform::FileIdentity> {
    let m = ulmdb_platform::file_identity(file)?;
    if !m.is_file() || m.links != 1 || m.file_id == 0 {
        return Err(Error::Corrupt("Resident capture gate file kind/link"));
    }
    Ok(m)
}
#[cfg(not(windows))]
fn busy(error: TryLockError) -> Error {
    match error {
        TryLockError::WouldBlock => Error::Busy,
        TryLockError::Error(e) => Error::Io(e),
    }
}
impl CaptureGate {
    fn open(directory: &Path, uuid: Option<[u8; 16]>, create: bool) -> Result<Self> {
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        return Err(Error::InvalidInput(
            "Resident coordinated capture requires Linux, macOS or Windows",
        ));
        let named = ulmdb_platform::path_identity(directory)?;
        if !named.is_dir() || named.reparse {
            return Err(Error::Corrupt("Resident capture directory kind"));
        }
        let directory = directory.canonicalize()?;
        let dir = ulmdb_platform::open_directory(&directory)?;
        let dm = ulmdb_platform::file_identity(&dir)?;
        let path = directory.join(NAME);
        let file = if create {
            crate::access::FileAccessContext::open(&directory)?
                .options()?
                .read(true)
                .write(true)
                .create(true)
                .open(&path)?
        } else {
            match OpenOptions::new()
                .read(true)
                .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
                .open(&path)
            {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(Error::InvalidInput("Resident capture protocol not enabled"));
                }
                r => r?,
            }
        };
        crate::access::validate_entry(&path, &file)?;
        let m = regular(&file)?;
        let actual = if create {
            uuid.ok_or(Error::InvalidInput("capture gate UUID"))?
        } else {
            let mut b = [0; 64];
            if m.len() != 64 {
                return Err(Error::Corrupt("Resident capture gate width"));
            }
            file.read_exact_at(&mut b, 0)?;
            let id: [u8; 16] = b[16..32].try_into().unwrap();
            if b != header(id) || id == [0; 16] {
                return Err(Error::Corrupt("Resident capture gate identity/hash"));
            }
            id
        };
        let gate = Self {
            inner: Arc::new(Inner {
                directory,
                _directory: dir,
                file,
                device: m.volume,
                inode: m.file_id,
                dir_device: dm.volume,
                dir_inode: dm.file_id,
                uuid: actual,
                creator: std::process::id(),
            }),
        };
        gate.check()?;
        Ok(gate)
    }
    pub(super) fn prepare(directory: &Path, uuid: [u8; 16]) -> Result<(Self, GateGuard)> {
        let gate = Self::open(directory, Some(uuid), true)?;
        let guard = gate.lock(false)?;
        let m = gate.inner.file.metadata()?;
        let expected = header(uuid);
        if m.len() > 64 {
            return Err(Error::Corrupt("Resident capture gate width"));
        }
        let mut prior = vec![0; m.len() as usize];
        gate.inner.file.read_exact_at(&mut prior, 0)?;
        if !expected.starts_with(&prior) {
            return Err(Error::Corrupt("Resident capture gate prior identity"));
        }
        if m.len() != 64 {
            guard.file().write_all_at(&expected, 0)?;
            guard.file().set_len(64)?;
            ulmdb_platform::sync_file(guard.file())?;
            ulmdb_platform::sync_directory_file(&gate.inner._directory)?;
        }
        guard.check()?;
        Ok((gate, guard))
    }
    pub(super) fn writer(
        directory: &Path,
        uuid: [u8; 16],
        identity: CaptureIdentity,
    ) -> Result<Self> {
        let gate = Self::open(directory, None, false)?;
        if gate.inner.uuid != uuid || gate.identity() != identity {
            return Err(Error::Corrupt("Resident capture negotiated gate mismatch"));
        }
        Ok(gate)
    }
    pub(super) fn identity(&self) -> CaptureIdentity {
        #[cfg(windows)]
        return CaptureIdentity::Windows {
            volume: self.inner.device,
            file_id: self.inner.inode,
        };
        #[cfg(not(windows))]
        CaptureIdentity::Unix {
            device: self.inner.device,
            // Unix FileIdentity is constructed from its native u64 inode.
            inode: self.inner.inode as u64,
        }
    }
    pub(super) fn check(&self) -> Result<()> {
        if self.inner.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident capture gate"));
        }
        let dm = ulmdb_platform::path_identity(&self.inner.directory)?;
        let fm = ulmdb_platform::path_identity(&self.inner.directory.join(NAME))?;
        if !dm.is_dir()
            || dm.reparse
            || dm.volume != self.inner.dir_device
            || dm.file_id != self.inner.dir_inode
            || !fm.is_file()
            || fm.reparse
            || fm.links != 1
            || fm.volume != self.inner.device
            || fm.file_id != self.inner.inode
        {
            return Err(Error::Corrupt("Resident capture namespace changed"));
        }
        Ok(())
    }
    fn lock(&self, shared: bool) -> Result<GateGuard> {
        self.check()?;
        let file = OpenOptions::new()
            .read(true)
            .write(!shared)
            .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
            .open(self.inner.directory.join(NAME))?;
        let m = regular(&file)?;
        if (m.volume, m.file_id) != (self.inner.device, self.inner.inode) {
            return Err(Error::Corrupt("Resident capture gate replaced"));
        }
        #[cfg(not(windows))]
        if shared {
            file.try_lock_shared().map_err(busy)?
        } else {
            file.try_lock().map_err(busy)?
        }
        // Windows byte locks are mandatory. Locking the whole64-byte header
        // would prevent another handle from reading it before SH admission.
        #[cfg(windows)]
        let lease = ulmdb_platform::RangeLock::acquire(
            file,
            if shared {
                ulmdb_platform::LockMode::Shared
            } else {
                ulmdb_platform::LockMode::Exclusive
            },
            LOCK_BYTE,
            1,
            false,
        )
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                Error::Busy
            } else {
                e.into()
            }
        })?;
        let guard = GateGuard {
            #[cfg(not(windows))]
            file,
            #[cfg(windows)]
            lease,
            gate: self.clone(),
            creator: std::process::id(),
        };
        guard.check()?;
        Ok(guard)
    }
    pub(super) fn exclusive(&self) -> Result<GateGuard> {
        self.lock(false)
    }
}
impl ReadCapture {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let gate = CaptureGate::open(directory.as_ref(), None, false)?;
        Ok(Self {
            guard: gate.lock(true)?,
        })
    }
    pub fn directory(&self) -> &Path {
        &self.guard.gate.inner.directory
    }
    pub(super) fn uuid(&self) -> [u8; 16] {
        self.guard.gate.inner.uuid
    }
    pub(super) fn check(&self) -> Result<()> {
        self.guard.check()
    }
    pub(super) fn validate_header(&self, b: &[u8]) -> Result<()> {
        let id =
            protocol(b)?.ok_or(Error::InvalidInput("Resident capture protocol not enabled"))?;
        if id != self.guard.gate.identity() {
            return Err(Error::Corrupt("Resident capture WAL gate binding"));
        }
        Ok(())
    }
    pub(super) fn validate_wal(&self, file: &File) -> Result<()> {
        self.check()?;
        let held = regular(file)?;
        let named = ulmdb_platform::path_identity(&self.directory().join("RESIDENT.wal"))?;
        if !named.is_file()
            || named.reparse
            || named.links != 1
            || named.volume != held.volume
            || named.file_id != held.file_id
        {
            return Err(Error::Corrupt("Resident captured WAL replaced"));
        }
        Ok(())
    }
    pub(super) fn preflight(
        &self,
        selected_end: u64,
        checkpoint: Option<&Path>,
        limits: CaptureLimits,
    ) -> Result<()> {
        limits.validate()?;
        if selected_end
            .checked_sub(196608)
            .is_none_or(|n| n > limits.max_replay_bytes)
        {
            return Err(Error::LimitExceeded);
        }
        if let Some(p) = checkpoint {
            let m = ulmdb_platform::path_identity(p)?;
            if !m.is_file() || m.reparse || m.links != 1 {
                return Err(Error::Corrupt("Resident capture checkpoint kind/link"));
            }
            if m.len() > limits.max_checkpoint_bytes {
                return Err(Error::LimitExceeded);
            }
        }
        Ok(())
    }
}
impl GateGuard {
    fn file(&self) -> &File {
        #[cfg(windows)]
        return self.lease.file();
        #[cfg(not(windows))]
        &self.file
    }
    pub(super) fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident capture lease"));
        }
        self.gate.check()
    }
}
impl Drop for GateGuard {
    fn drop(&mut self) {
        // Windows RangeLock owns its exact byte range and creator-safe Drop.
        #[cfg(not(windows))]
        if self.creator == std::process::id() {
            let _ = self.file.unlock();
        }
    }
}
pub(super) fn protocol(header: &[u8]) -> Result<Option<CaptureIdentity>> {
    if header.len() != 256 {
        return Err(Error::Corrupt("Resident capture header width"));
    }
    let flag = u64::from_le_bytes(header[184..192].try_into().unwrap());
    match flag {
        0 => {
            if header[192..224] != [0; 32] {
                return Err(Error::Corrupt("Resident capture unnegotiated identity"));
            }
            Ok(None)
        }
        1 => {
            let dev = u64::from_le_bytes(header[192..200].try_into().unwrap());
            let ino = u64::from_le_bytes(header[200..208].try_into().unwrap());
            if ino == 0 || header[208..224] != [0; 16] {
                return Err(Error::Corrupt("Resident capture zero gate inode"));
            }
            Ok(Some(CaptureIdentity::Unix {
                device: dev,
                inode: ino,
            }))
        }
        2 => {
            let volume = u64::from_le_bytes(header[192..200].try_into().unwrap());
            let file_id = u128::from_le_bytes(header[200..216].try_into().unwrap());
            if file_id == 0 || header[216..224] != [0; 8] {
                return Err(Error::Corrupt("Resident capture Windows gate identity"));
            }
            Ok(Some(CaptureIdentity::Windows { volume, file_id }))
        }
        _ => Err(Error::Corrupt("Resident capture protocol version")),
    }
}
pub(super) fn negotiated(mut h: [u8; 256], identity: CaptureIdentity) -> [u8; 256] {
    h[192..224].fill(0);
    let domain: &[u8] = match identity {
        CaptureIdentity::Unix { device, inode } => {
            h[184..192].copy_from_slice(&1u64.to_le_bytes());
            h[192..200].copy_from_slice(&device.to_le_bytes());
            h[200..208].copy_from_slice(&inode.to_le_bytes());
            b"ULMDB/resident/capture-WAL-header/v1\0"
        }
        CaptureIdentity::Windows { volume, file_id } => {
            h[184..192].copy_from_slice(&2u64.to_le_bytes());
            h[192..200].copy_from_slice(&volume.to_le_bytes());
            h[200..216].copy_from_slice(&file_id.to_le_bytes());
            b"ULMDB/resident/capture-WAL-header/v2\0"
        }
    };
    let mut sum = Sha256::new();
    sum.update(domain);
    sum.update(&h[..224]);
    h[224..].copy_from_slice(&sum.finalize());
    h
}
pub(super) fn finish<T>(
    result: Result<T>,
    guard: Option<&GateGuard>,
    poisoned: &mut bool,
) -> Result<T> {
    if let Some(g) = guard {
        if let Err(error) = g.check() {
            if result.is_ok() || *poisoned {
                *poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            return Err(error);
        }
    }
    result
}

#[cfg(test)]
pub(super) fn check_full_identity_codec(
    header: fn(crate::resident::wal_codec::Boot, u64) -> Result<[u8; 256]>,
    decode: fn(&[u8]) -> Result<(crate::resident::wal_codec::Boot, u64)>,
) {
    let boot = crate::resident::wal_codec::Boot {
        uuid: [7; 16],
        epoch: 2,
        commit: 100,
        kv_pages: 16,
        tiny_pages: 4,
    };
    for id in [
        CaptureIdentity::Unix {
            device: 9,
            inode: 0x1234,
        },
        CaptureIdentity::Windows {
            volume: 9,
            file_id: (0xfedcba9876543210u128 << 64) | 0x1234,
        },
    ] {
        let source = negotiated(header(boot, 1 << 20).unwrap(), id);
        assert_eq!(protocol(&source).unwrap(), Some(id));
        assert_eq!(decode(&source).unwrap(), (boot, 1 << 20));
        for at in [184, 192, 200, 208, 215, 216, 224, 255] {
            let mut bad = source;
            bad[at] ^= 1;
            assert!(decode(&bad).is_err(), "unauthenticated source byte {at}");
        }
    }
    let id = CaptureIdentity::Windows {
        volume: 9,
        file_id: (1 << 100) | 0x1234,
    };
    let mut wrong_domain = negotiated(header(boot, 1 << 20).unwrap(), id);
    let mut sum = Sha256::new();
    sum.update(b"ULMDB/resident/capture-WAL-header/v1\0");
    sum.update(&wrong_domain[..224]);
    wrong_domain[224..].copy_from_slice(&sum.finalize());
    assert!(decode(&wrong_domain).is_err());
    assert!(
        decode(&negotiated(
            header(boot, 1 << 20).unwrap(),
            CaptureIdentity::Windows {
                volume: 9,
                file_id: 0
            }
        ))
        .is_err()
    );
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod gate_tests {
    use super::*;
    #[test]
    fn capture_gate_conflicts_leave_header_io_and_shared_owners_live() {
        let path =
            std::env::temp_dir().join(format!("resident-gate-{}", crate::new_identity().unwrap()));
        crate::create_layout(&path).unwrap();
        let uuid = crate::new_identity().unwrap().to_le_bytes();
        let (gate, writer) = CaptureGate::prepare(&path, uuid).unwrap();
        let independent = File::open(path.join(NAME)).unwrap();
        let mut actual = [0; 64];
        independent.read_exact_at(&mut actual, 0).unwrap();
        assert_eq!(actual, header(uuid));
        assert!(matches!(ReadCapture::open(&path), Err(Error::Busy)));
        drop(writer);
        let a = ReadCapture::open(&path).unwrap();
        let b = ReadCapture::open(&path).unwrap();
        assert!(matches!(gate.exclusive(), Err(Error::Busy)));
        drop(a);
        assert!(matches!(gate.exclusive(), Err(Error::Busy)));
        let mut wal = [0; 256];
        wal = negotiated(wal, gate.identity());
        b.validate_header(&wal).unwrap();
        let wrong = match gate.identity() {
            CaptureIdentity::Windows { volume, file_id } => CaptureIdentity::Windows {
                volume,
                file_id: file_id ^ (1 << 100),
            },
            CaptureIdentity::Unix { device, inode } => CaptureIdentity::Unix {
                device,
                inode: inode ^ (1 << 50),
            },
        };
        assert!(b.validate_header(&negotiated(wal, wrong)).is_err());
        drop(b);
        drop(gate.exclusive().unwrap());
        drop((independent, gate));
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn unix_flag_one_bytes_are_unchanged_and_windows_high_bits_do_not_alias() {
        let mut h = [0; 256];
        for (n, byte) in h[..184].iter_mut().enumerate() {
            *byte = n as u8;
        }
        let old = negotiated(
            h,
            CaptureIdentity::Unix {
                device: 0x1234567890abcdef,
                inode: 0xfedcba9876543210,
            },
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(old)),
            "cf4ee948df125afc26b2b252dbeab528d8d609af12074092832573c38cf336e9"
        );
        let a = CaptureIdentity::Windows {
            volume: 9,
            file_id: (1 << 100) | 3,
        };
        let b = CaptureIdentity::Windows {
            volume: 9,
            file_id: (1 << 101) | 3,
        };
        assert_ne!(a, b);
        assert_eq!(protocol(&negotiated(h, a)).unwrap(), Some(a));
        assert_eq!(protocol(&negotiated(h, b)).unwrap(), Some(b));
        let mut unknown = negotiated(h, a);
        unknown[184..192].copy_from_slice(&3u64.to_le_bytes());
        assert!(protocol(&unknown).is_err());
    }
}
#[cfg(all(test, target_os = "linux"))]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    #[test]
    fn foreign_fork_drop_keeps_parent_capture_lease() {
        const NAME: &str = "resident::capture::tests::foreign_fork_drop_keeps_parent_capture_lease";
        if crate::history::tests::run_isolated(NAME) {
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "resident-capture-fork-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        let (gate, exclusive) =
            CaptureGate::prepare(&path, crate::new_identity().unwrap().to_le_bytes()).unwrap();
        drop(exclusive);
        let lease = ReadCapture::open(&path).unwrap();
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            let valid = matches!(
                lease.check(),
                Err(Error::InvalidInput("inherited Resident capture lease"))
            );
            drop(lease);
            unsafe { libc::_exit(if valid { 0 } else { 7 }) }
        }
        let start = std::time::Instant::now();
        let mut status = 0;
        loop {
            let rc = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if rc == pid {
                break;
            }
            assert!(rc >= 0);
            if start.elapsed() > std::time::Duration::from_secs(3) {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                    libc::waitpid(pid, &mut status, 0);
                }
                panic!("capture fork control deadline")
            };
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(status, 0);
        assert!(
            matches!(gate.exclusive(), Err(Error::Busy)),
            "foreign child must not unlock parent's OFD"
        );
        drop(lease);
        drop(gate.exclusive().unwrap());
        drop(gate);
        std::fs::remove_dir_all(path).unwrap();
        crate::history::tests::worker_completed(NAME);
    }
}
