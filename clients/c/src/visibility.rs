//! Bounded C-only transaction visibility. Data always comes from native pins.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

#[cfg(all(test, target_os = "linux"))]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use ulmdb_platform::FileExt;
#[cfg(windows)]
type FileNumber = u128;
#[cfg(not(windows))]
type FileNumber = u64;
#[cfg(windows)]
const MAGIC: &[u8; 8] = b"ULMCVW03";
#[cfg(not(windows))]
const MAGIC: &[u8; 8] = b"ULMCVW02";
#[cfg(windows)]
const VERSION: u16 = 3;
#[cfg(not(windows))]
const VERSION: u16 = 2;
const UUID: usize = 46 + std::mem::size_of::<FileNumber>();
const EPOCH: usize = UUID + 16;
const MINOR: usize = EPOCH + 8;
const GATE_ID: usize = MINOR + 2;
const RECORD_ID: usize = GATE_ID + std::mem::size_of::<FileNumber>();
const DURABLE: usize = RECORD_ID + std::mem::size_of::<FileNumber>();
const VIEW: usize = DURABLE + 64;
const HASH: usize = BYTES - 32;
fn file_number(info: &ulmdb_platform::FileIdentity) -> FfiResult<FileNumber> {
    info.file_id.try_into().map_err(|_| CORRUPT)
}
fn file_key(info: &ulmdb_platform::FileIdentity) -> FfiResult<(u64, FileNumber)> {
    Ok((info.volume, file_number(info)?))
}
pub(super) const PREVIOUS: u32 = 0x2000000;
const GATE: &str = "LMDB-VIEW-GATE";
const RECORD: &str = "LMDB-VIEW";
#[cfg(not(windows))]
const BYTES: usize = 256;
#[cfg(windows)]
const BYTES: usize = 288;
const CORRUPT: c_int = -30796;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Canonical,
    Offset,
}

/// Metadata copied only from a native authenticated/pinned manifest.
#[derive(Clone, Copy, Debug)]
pub(super) struct Meta {
    pub checkpoint: [u8; 64],
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub minor: u16,
    pub previous: u128,
    pub empty_genesis: bool,
}
impl Meta {
    pub(super) fn new(manifest: &ulmdb_storage::Manifest, checkpoint: [u8; 64]) -> FfiResult<Self> {
        if ulmdb_storage::checkpoint_identity(&checkpoint)
            .map_err(code)?
            .0
            != manifest.generation
        {
            return Err(CORRUPT);
        }
        let meta = Self {
            checkpoint,
            uuid: manifest.uuid,
            epoch: manifest.epoch,
            minor: manifest.format_minor,
            previous: manifest.previous,
            empty_genesis: manifest.sequence == 0
                && manifest.previous == 0
                && manifest.catalog.is_null()
                && manifest.parent == 0
                && manifest.next_database == 1
                && manifest.next_segment == 1
                && manifest.segments.is_empty(),
        };
        check_checkpoint(&meta.checkpoint, meta.minor)?;
        Ok(meta)
    }
}

#[derive(Clone, Debug)]
pub(super) struct Record {
    pub kind: Kind,
    pub session: [u8; 16],
    pub revision: u64,
    pub identity: (u64, FileNumber),
    pub gate_inode: FileNumber,
    pub record_inode: FileNumber,
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub minor: u16,
    pub durable: [u8; 64],
    pub view: [u8; 64],
}
impl Record {
    fn canonical(
        identity: (u64, FileNumber),
        gate_inode: FileNumber,
        record_inode: FileNumber,
        meta: Meta,
    ) -> FfiResult<Self> {
        Ok(Self {
            kind: Kind::Canonical,
            session: ulmdb_storage::new_identity().map_err(code)?.to_le_bytes(),
            revision: 1,
            identity,
            gate_inode,
            record_inode,
            uuid: meta.uuid,
            epoch: meta.epoch,
            minor: meta.minor,
            durable: meta.checkpoint,
            view: meta.checkpoint,
        })
    }
    pub(super) fn updated(&self, durable: Meta, view: Meta, kind: Kind) -> FfiResult<Self> {
        if durable.uuid != self.uuid
            || durable.epoch != self.epoch
            || view.uuid != durable.uuid
            || view.epoch != durable.epoch
            || view.minor != durable.minor
        {
            return Err(CORRUPT);
        }
        match kind {
            Kind::Canonical if durable.checkpoint != view.checkpoint => return Err(CORRUPT),
            Kind::Offset if durable.checkpoint == view.checkpoint => {
                if !durable.empty_genesis || !view.empty_genesis {
                    return Err(CORRUPT);
                }
            }
            Kind::Offset => {
                let previous = ulmdb_storage::checkpoint_identity(&view.checkpoint)
                    .map_err(code)?
                    .0;
                if durable.previous != previous {
                    return Err(CORRUPT);
                }
            }
            _ => {}
        }
        Ok(Self {
            kind,
            session: self.session,
            revision: self.revision.checked_add(1).ok_or(-30792)?,
            identity: self.identity,
            gate_inode: self.gate_inode,
            record_inode: self.record_inode,
            uuid: durable.uuid,
            epoch: durable.epoch,
            minor: durable.minor,
            durable: durable.checkpoint,
            view: view.checkpoint,
        })
    }
    pub(super) fn matches(&self, durable: Meta) -> FfiResult<bool> {
        // A format transition can invalidate a valid old binding; a live realm
        // transition cannot silently retarget this environment's capabilities.
        if self.uuid != durable.uuid || self.epoch != durable.epoch {
            return Err(CORRUPT);
        }
        if self.durable != durable.checkpoint {
            return Ok(false);
        }
        if self.minor != durable.minor {
            return Err(CORRUPT);
        }
        Ok(true)
    }
    fn encode(&self) -> FfiResult<[u8; BYTES]> {
        let mut bytes = [0; BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&VERSION.to_le_bytes());
        bytes[10..12].copy_from_slice(&(BYTES as u16).to_le_bytes());
        bytes[12..14]
            .copy_from_slice(&(if self.kind == Kind::Offset { 1u16 } else { 0 }).to_le_bytes());
        bytes[14..30].copy_from_slice(&self.session);
        bytes[30..38].copy_from_slice(&self.revision.to_le_bytes());
        bytes[38..46].copy_from_slice(&self.identity.0.to_le_bytes());
        bytes[46..UUID].copy_from_slice(&self.identity.1.to_le_bytes());
        bytes[UUID..EPOCH].copy_from_slice(&self.uuid);
        bytes[EPOCH..MINOR].copy_from_slice(&self.epoch.to_le_bytes());
        bytes[MINOR..GATE_ID].copy_from_slice(&self.minor.to_le_bytes());
        bytes[GATE_ID..RECORD_ID].copy_from_slice(&self.gate_inode.to_le_bytes());
        bytes[RECORD_ID..DURABLE].copy_from_slice(&self.record_inode.to_le_bytes());
        bytes[DURABLE..VIEW].copy_from_slice(&self.durable);
        bytes[VIEW..VIEW + 64].copy_from_slice(&self.view);
        let hash: [u8; 32] = Sha256::digest(&bytes[..HASH]).into();
        bytes[HASH..].copy_from_slice(&hash);
        Self::decode(&bytes)?;
        Ok(bytes)
    }
    fn decode(bytes: &[u8; BYTES]) -> FfiResult<Self> {
        let hash: [u8; 32] = Sha256::digest(&bytes[..HASH]).into();
        if &bytes[..8] != MAGIC
            || bytes[8..10] != VERSION.to_le_bytes()
            || bytes[10..12] != (BYTES as u16).to_le_bytes()
            || bytes[HASH..] != hash
            || bytes[VIEW + 64..HASH].iter().any(|b| *b != 0)
        {
            return Err(CORRUPT);
        }
        let kind = match u16::from_le_bytes(bytes[12..14].try_into().unwrap()) {
            0 => Kind::Canonical,
            1 => Kind::Offset,
            _ => return Err(CORRUPT),
        };
        let result = Self {
            kind,
            session: bytes[14..30].try_into().unwrap(),
            revision: u64::from_le_bytes(bytes[30..38].try_into().unwrap()),
            identity: (
                u64::from_le_bytes(bytes[38..46].try_into().unwrap()),
                FileNumber::from_le_bytes(bytes[46..UUID].try_into().unwrap()),
            ),
            gate_inode: FileNumber::from_le_bytes(bytes[GATE_ID..RECORD_ID].try_into().unwrap()),
            record_inode: FileNumber::from_le_bytes(bytes[RECORD_ID..DURABLE].try_into().unwrap()),
            uuid: bytes[UUID..EPOCH].try_into().unwrap(),
            epoch: u64::from_le_bytes(bytes[EPOCH..MINOR].try_into().unwrap()),
            minor: u16::from_le_bytes(bytes[MINOR..GATE_ID].try_into().unwrap()),
            durable: bytes[DURABLE..VIEW].try_into().unwrap(),
            view: bytes[VIEW..VIEW + 64].try_into().unwrap(),
        };
        if result.session == [0; 16]
            || result.revision == 0
            || result.uuid == [0; 16]
            || result.epoch == 0
            || (result.kind == Kind::Canonical && result.durable != result.view)
        {
            return Err(CORRUPT);
        }
        check_checkpoint(&result.durable, result.minor)?;
        check_checkpoint(&result.view, result.minor)?;
        Ok(result)
    }
}

fn check_checkpoint(checkpoint: &[u8; 64], minor: u16) -> FfiResult<()> {
    ulmdb_storage::checkpoint_identity(checkpoint).map_err(code)?;
    let expected: &[u8] = match minor {
        1 | 2 => b"ULMDBCUR",
        3 => b"ULMDBC03",
        4 => b"ULMDBT04",
        _ => return Err(CORRUPT),
    };
    if &checkpoint[..8] != expected {
        return Err(CORRUPT);
    }
    Ok(())
}

pub(super) struct Visibility {
    directory: PathBuf,
    directory_file: File,
    record: File,
    identity: (u64, FileNumber),
    gate_inode: FileNumber,
    record_inode: FileNumber,
    session: [u8; 16],
    creator: u32,
}
pub(super) struct Lease<'a> {
    owner: &'a Visibility,
    file: File,
    exclusive: bool,
    creator: u32,
}
impl Visibility {
    /// `initial` is supplied only after genuine ENV EX and native authentication.
    /// Joining SH never creates, truncates or repairs a missing/damaged record.
    pub(super) fn open(
        directory: &Path,
        initial: Option<Meta>,
        environment: &process::EnvironmentLease,
    ) -> FfiResult<Self> {
        environment.bound_to(directory)?;
        if initial.is_some() && !environment.can_initialize(directory)? {
            return Err(libc::EINVAL);
        }
        let directory = directory.canonicalize().map_err(io)?;
        let directory_file = ulmdb_platform::open_directory(&directory).map_err(io)?;
        let metadata = ulmdb_platform::file_identity(&directory_file).map_err(io)?;
        let identity = file_key(&metadata)?;
        let gate = open_file(&directory.join(GATE), initial.is_some())?;
        check_file(&directory.join(GATE), &gate, Some(0))?;
        let record = open_file(&directory.join(RECORD), initial.is_some())?;
        let gate_metadata = ulmdb_platform::file_identity(&gate).map_err(io)?;
        let record_metadata = ulmdb_platform::file_identity(&record).map_err(io)?;
        if gate_metadata.volume != identity.0 || record_metadata.volume != identity.0 {
            return Err(CORRUPT);
        }
        if initial.is_some() {
            record.set_len(BYTES as u64).map_err(io)?;
        }
        let mut owner = Self {
            directory,
            directory_file,
            record,
            identity,
            gate_inode: file_number(&gate_metadata)?,
            record_inode: file_number(&record_metadata)?,
            session: [0; 16],
            creator: std::process::id(),
        };
        let parsed = {
            let lease = owner.lease(initial.is_some())?;
            if let Some(meta) = initial {
                let parsed =
                    Record::canonical(identity, owner.gate_inode, owner.record_inode, meta)?;
                owner.store(&lease, &parsed)?;
                ulmdb_platform::open_directory(&owner.directory)
                    .and_then(|f| ulmdb_platform::sync_file(&f))
                    .map_err(io)?;
                parsed
            } else {
                owner.read(&lease)?
            }
        };
        owner.session = parsed.session;
        Ok(owner)
    }
    pub(super) fn check_process(&self) -> FfiResult<()> {
        if self.creator != std::process::id() {
            return Err(libc::EINVAL);
        }
        Ok(())
    }
    fn check(&self) -> FfiResult<()> {
        self.check_process()?;
        let owned = ulmdb_platform::file_identity(&self.directory_file).map_err(io)?;
        let named = ulmdb_platform::path_identity(&self.directory).map_err(io)?;
        if !owned.is_dir()
            || !named.is_dir()
            || named.reparse
            || file_key(&owned)? != self.identity
            || file_key(&named)? != self.identity
        {
            return Err(CORRUPT);
        }
        let named_gate = ulmdb_platform::path_identity(&self.directory.join(GATE)).map_err(io)?;
        if !named_gate.is_file()
            || named_gate.reparse
            || named_gate.links != 1
            || named_gate.len() != 0
            || file_key(&named_gate)? != (self.identity.0, self.gate_inode)
        {
            return Err(CORRUPT);
        }
        check_file(
            &self.directory.join(RECORD),
            &self.record,
            Some(BYTES as u64),
        )?;
        let record = ulmdb_platform::file_identity(&self.record).map_err(io)?;
        if file_key(&record)? != (self.identity.0, self.record_inode) {
            return Err(CORRUPT);
        }
        Ok(())
    }
    pub(super) fn lease(&self, exclusive: bool) -> FfiResult<Lease<'_>> {
        self.check()?;
        let file = open_file(&self.directory.join(GATE), false)?;
        check_file(&self.directory.join(GATE), &file, Some(0))?;
        loop {
            let result = if exclusive {
                file.lock()
            } else {
                file.lock_shared()
            };
            match result {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(io(error)),
            }
        }
        let lease = Lease {
            owner: self,
            file,
            exclusive,
            creator: std::process::id(),
        };
        // Recheck after waiting: replacement must not split authority across OFDs.
        self.check()?;
        check_file(&self.directory.join(GATE), &lease.file, Some(0))?;
        Ok(lease)
    }
    pub(super) fn read(&self, lease: &Lease<'_>) -> FfiResult<Record> {
        self.check_lease(lease, false)?;
        let mut bytes = [0; BYTES];
        self.record.read_exact_at(&mut bytes, 0).map_err(io)?;
        let record = Record::decode(&bytes)?;
        if record.identity != self.identity
            || record.gate_inode != self.gate_inode
            || record.record_inode != self.record_inode
            || (self.session != [0; 16] && record.session != self.session)
        {
            return Err(CORRUPT);
        }
        Ok(record)
    }
    pub(super) fn store(&self, lease: &Lease<'_>, record: &Record) -> FfiResult<()> {
        self.check_lease(lease, true)?;
        if record.identity != self.identity
            || record.gate_inode != self.gate_inode
            || record.record_inode != self.record_inode
            || (self.session != [0; 16] && record.session != self.session)
        {
            return Err(CORRUPT);
        }
        let bytes = record.encode()?;
        #[cfg(test)]
        point("view.before_write")?;
        self.record.write_all_at(&bytes, 0).map_err(io)?;
        #[cfg(test)]
        point("view.after_write")?;
        #[cfg(test)]
        point("view.before_file_sync")?;
        ulmdb_platform::sync_file(&self.record).map_err(io)?;
        #[cfg(test)]
        point("view.after_file_sync")?;
        Ok(())
    }
    fn check_lease(&self, lease: &Lease<'_>, exclusive: bool) -> FfiResult<()> {
        self.check()?;
        if !std::ptr::eq(self, lease.owner)
            || lease.creator != std::process::id()
            || (exclusive && !lease.exclusive)
        {
            return Err(libc::EINVAL);
        }
        check_file(&self.directory.join(GATE), &lease.file, Some(0))
    }
}
#[cfg(test)]
fn point(name: &str) -> FfiResult<()> {
    if let Some(path) = std::env::var_os("ULMDB_C_VIEW_TRACE") {
        use std::io::Write;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io)?;
        writeln!(file, "{name}").map_err(io)?;
    }
    if std::env::var("ULMDB_C_VIEW_FAIL_AT").is_ok_and(|point| point == name) {
        return Err(libc::EIO);
    }
    Ok(())
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let _ = self.file.unlock();
        }
    }
}
fn io(error: std::io::Error) -> c_int {
    error.raw_os_error().unwrap_or(libc::EIO)
}
fn open_file(path: &Path, create: bool) -> FfiResult<File> {
    let access = ulmdb_storage::access::context_for_entry(path).map_err(code)?;
    access.validate_parent(path).map_err(code)?;
    let file = access.options()
        .map_err(code)?
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
        .open(path)
        .map_err(|error| {
            if !create && error.kind() == std::io::ErrorKind::NotFound {
                CORRUPT
            } else {
                io(error)
            }
        })?;
    access.revalidate_file(&file).map_err(code)?;
    check_file(path, &file, None)?;
    Ok(file)
}
fn check_file(path: &Path, file: &File, length: Option<u64>) -> FfiResult<()> {
    let named = ulmdb_platform::path_identity(path).map_err(io)?;
    let owned = ulmdb_platform::file_identity(file).map_err(io)?;
    if !named.is_file()
        || named.reparse
        || !owned.is_file()
        || owned.links != 1
        || !named.same_object(&owned)
        || length.is_some_and(|length| owned.len() != length)
    {
        return Err(CORRUPT);
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, mpsc},
        time::{Duration, Instant},
    };

    struct Fixture {
        path: PathBuf,
        environment: process::EnvironmentLease,
        view: Arc<Visibility>,
        meta: Meta,
    }
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("c-view-{}", ulmdb_storage::new_identity().unwrap()));
            drop(Environment::create(&path).unwrap());
            let manifest = ulmdb_storage::read_current(&path).unwrap();
            let meta = Meta::new(
                &manifest,
                ulmdb_storage::checkpoint_bytes(&manifest).unwrap(),
            )
            .unwrap();
            let (environment, _) = process::EnvironmentLease::begin(&path, 8, false).unwrap();
            let view = Arc::new(Visibility::open(&path, Some(meta), &environment).unwrap());
            environment.downgrade().unwrap();
            Self {
                path,
                environment,
                view,
                meta,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn replacement_is_rejected_by_existing_and_joining_visibility_authority() {
        for target in [GATE, RECORD] {
            let fixture = Fixture::new();
            let held = fixture.view.lease(false).unwrap();
            let original = fixture.path.join(target);
            let bytes = fs::read(&original).unwrap();
            fs::rename(&original, fixture.path.join("replaced-inode")).unwrap();
            fs::write(&original, bytes).unwrap();
            let (join, _) = process::EnvironmentLease::begin(&fixture.path, 8, false).unwrap();
            assert!(!join.fresh());
            assert!(
                matches!(Visibility::open(&fixture.path, None, &join), Err(CORRUPT)),
                "a joining process accepted replacement of {target}"
            );
            assert!(
                matches!(fixture.view.lease(false), Err(CORRUPT)),
                "an existing process accepted replacement of {target}"
            );
            drop(held);
        }
    }

    fn wait_for_kernel_waiter(path: &Path) {
        let inode = fs::metadata(path).unwrap().ino().to_string();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let text = fs::read_to_string("/proc/locks").unwrap();
            if text.lines().any(|line| {
                line.contains("->")
                    && line
                        .split_whitespace()
                        .any(|word| word.rsplit(':').next() == Some(inode.as_str()))
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "no actual blocked kernel lease for {inode}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn independent_shared_leases_keep_writer_excluded_until_last_reader_ends() {
        let fixture = Fixture::new();
        let first = fixture.view.lease(false).unwrap();
        let (ready, received) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let view = fixture.view.clone();
        let second = std::thread::spawn(move || {
            let lease = view.lease(false).unwrap();
            ready.send(()).unwrap();
            released.recv().unwrap();
            drop(lease);
        });
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        let (ready, writer_ready) = mpsc::channel();
        let view = fixture.view.clone();
        let writer = std::thread::spawn(move || {
            let lease = view.lease(true).unwrap();
            ready.send(()).unwrap();
            drop(lease);
        });
        wait_for_kernel_waiter(&fixture.path.join(GATE));
        drop(first);
        assert!(
            writer_ready
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "ending one reader released another reader's OFD authority"
        );
        wait_for_kernel_waiter(&fixture.path.join(GATE));
        release.send(()).unwrap();
        second.join().unwrap();
        writer_ready.recv_timeout(Duration::from_secs(2)).unwrap();
        writer.join().unwrap();
    }

    #[test]
    fn damaged_record_is_not_repaired_by_live_shared_join_or_stale_fresh_flag() {
        let fixture = Fixture::new();
        let mut bytes = fs::read(fixture.path.join(RECORD)).unwrap();
        bytes[128] ^= 1;
        fs::write(fixture.path.join(RECORD), &bytes).unwrap();
        let lease = fixture.view.lease(false).unwrap();
        assert!(matches!(fixture.view.read(&lease), Err(CORRUPT)));
        drop(lease);
        let (join, _) = process::EnvironmentLease::begin(&fixture.path, 8, false).unwrap();
        assert!(!join.fresh());
        assert!(matches!(
            Visibility::open(&fixture.path, None, &join),
            Err(CORRUPT)
        ));
        assert!(matches!(
            Visibility::open(&fixture.path, Some(fixture.meta), &join),
            Err(libc::EINVAL)
        ));
        // This lease once held fresh EX but has already downgraded to SH.
        assert!(fixture.environment.fresh());
        assert!(matches!(
            Visibility::open(&fixture.path, Some(fixture.meta), &fixture.environment),
            Err(libc::EINVAL)
        ));
        assert_eq!(fs::read(fixture.path.join(RECORD)).unwrap(), bytes);
    }

    #[test]
    fn replacement_while_kernel_blocked_is_rejected_after_wakeup() {
        for target in [GATE, RECORD] {
            let fixture = Fixture::new();
            let held = fixture.view.lease(false).unwrap();
            let view = fixture.view.clone();
            let waiter = std::thread::spawn(move || view.lease(true).map(drop).err());
            wait_for_kernel_waiter(&fixture.path.join(GATE));
            let original = fixture.path.join(target);
            let bytes = fs::read(&original).unwrap();
            fs::rename(&original, fixture.path.join("replaced-during-wait")).unwrap();
            fs::write(&original, bytes).unwrap();
            drop(held);
            assert_eq!(
                waiter.join().unwrap(),
                Some(CORRUPT),
                "{target} changed while blocked"
            );
        }
    }

    #[test]
    fn damaged_and_hash_valid_invalid_records_fail_without_repair() {
        let cases = [
            "truncated",
            "extended",
            "bad-hash",
            "unknown-mode",
            "zero-session",
            "zero-revision",
            "wrong-directory",
            "wrong-gate",
            "wrong-record",
            "unknown-format",
        ];
        for case in cases {
            let fixture = Fixture::new();
            let record_path = fixture.path.join(RECORD);
            let mut bytes = fs::read(&record_path).unwrap();
            match case {
                "truncated" => {
                    bytes.pop();
                }
                "extended" => bytes.push(0),
                "bad-hash" => bytes[140] ^= 1,
                "unknown-mode" => bytes[12..14].copy_from_slice(&VERSION.to_le_bytes()),
                "zero-session" => bytes[14..30].fill(0),
                "zero-revision" => bytes[30..38].fill(0),
                "wrong-directory" => bytes[46] ^= 1,
                "wrong-gate" => bytes[80] ^= 1,
                "wrong-record" => bytes[88] ^= 1,
                "unknown-format" => bytes[MINOR..GATE_ID].copy_from_slice(&99u16.to_le_bytes()),
                _ => unreachable!(),
            }
            if !matches!(case, "truncated" | "extended" | "bad-hash") {
                let hash: [u8; 32] = Sha256::digest(&bytes[..HASH]).into();
                bytes[HASH..].copy_from_slice(&hash);
            }
            fs::write(&record_path, &bytes).unwrap();
            let owned_result = fixture
                .view
                .lease(false)
                .and_then(|lease| fixture.view.read(&lease));
            assert!(
                matches!(owned_result, Err(CORRUPT)),
                "existing authority accepted {case}"
            );
            let (join, _) = process::EnvironmentLease::begin(&fixture.path, 8, false).unwrap();
            assert!(
                matches!(Visibility::open(&fixture.path, None, &join), Err(CORRUPT)),
                "joining authority accepted {case}"
            );
            assert_eq!(
                fs::read(&record_path).unwrap(),
                bytes,
                "live SH repaired {case}"
            );
        }
    }
}
