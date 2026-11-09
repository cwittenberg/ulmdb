use crate::{
    Error, Manifest, PreparedPublication, Result, checkpoint_bytes, checkpoint_identity,
    new_identity,
};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

#[path = "history_floor.rs"]
mod history_floor;
pub use history_floor::HistoryFloorBuilder;

#[path = "generations.rs"]
mod generations;
pub use generations::{PendingPhysicalPin, PinDiscovery, PinDiscoveryBudget, PinDiscoveryProgress};

const AREA: &str = ".ulmdb-coordination";
const RECORD_BYTES: usize = 160;
const MAX_LIMIT: usize = 1_048_576;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Bounds admission and each registry walk, including stale records.
    pub max_pin_records: usize,
    /// Sum of inventory entries retained by a scan. One candidate descriptor
    /// may be decoded before this charge is checked; existing codec limits apply.
    pub max_inventory_entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pin_records: 4096,
            max_inventory_entries: 1_048_575,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}
impl Identity {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}
struct Inner {
    transform: Option<Arc<crate::transform::TransformContext>>,
    directory: PathBuf,
    area: PathBuf,
    pins: PathBuf,
    identity: Identity,
    area_identity: Identity,
    pins_identity: Identity,
    uuid: [u8; 16],
    limits: Limits,
    creator: u32,
    _directory: File,
    _area: File,
    _pins: File,
}
#[derive(Clone)]
pub struct Coordination {
    inner: Arc<Inner>,
}
struct Lease {
    file: File,
    creator: u32,
}
impl Lease {
    fn acquire(file: File, shared: bool) -> Result<Self> {
        let result = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        match result {
            Ok(()) => Ok(Self {
                file,
                creator: std::process::id(),
            }),
            Err(TryLockError::WouldBlock) => Err(Error::Busy),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let _ = self.file.unlock();
        }
    }
}
/// A directory-only shared gate acquired before any realm/head lookup.
/// It cannot register pins or grant maintenance authority until consumed by bind.
pub struct DirectoryCapture {
    inner: Arc<Inner>,
    gate: Lease,
}
pub struct CaptureLease {
    inner: Arc<Inner>,
    _gate: Lease,
}
/// Opaque same-capture D/P physical authority. Neither pin certifies a graph.
pub struct PhysicalCopyPair {
    selected: PhysicalPin,
    predecessor: Option<PhysicalPin>,
}
impl PhysicalCopyPair {
    pub fn selected(&self) -> &PhysicalPin {
        &self.selected
    }
    pub fn predecessor(&self) -> Option<&PhysicalPin> {
        self.predecessor.as_ref()
    }
    pub fn into_pins(self) -> (PhysicalPin, Option<PhysicalPin>) {
        (self.selected, self.predecessor)
    }
}
pub struct MaintenanceLease {
    inner: Arc<Inner>,
    _gate: Lease,
}
/// The exact descriptor decoded from a checked, persisted checkpoint.
/// This is not a certificate of the reachable logical data graph.
#[derive(Clone, Debug)]
pub struct PinnedRoot {
    manifest: Manifest,
    checkpoint: [u8; 64],
}
impl PinnedRoot {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn checkpoint(&self) -> &[u8; 64] {
        &self.checkpoint
    }
    pub fn matches_manifest(&self, manifest: &Manifest) -> Result<bool> {
        Ok(manifest.uuid == self.manifest.uuid && checkpoint_bytes(manifest)? == self.checkpoint)
    }
}
pub struct PhysicalPin {
    inner: Arc<Inner>,
    path: PathBuf,
    lease: Lease,
    root: PinnedRoot,
    birth: Option<generations::Birth>,
}
impl PhysicalPin {
    /// Compare an independently held directory object with this pin's exact
    /// physical namespace, without trusting a path that can be replaced.
    pub fn matches_directory_handle(&self, directory: &File) -> Result<bool> {
        self.inner.check()?;
        let metadata = directory.metadata()?;
        Ok(metadata.is_dir() && Identity::of(&metadata) == self.inner.identity)
    }
    pub fn root(&self) -> &PinnedRoot {
        &self.root
    }
    pub fn coordination(&self) -> Coordination {
        Coordination {
            inner: self.inner.clone(),
        }
    }
    /// Checks both directory ownership and the exact immutable descriptor.
    /// A copied/restored environment may share UUID/checkpoint bytes but cannot
    /// borrow another directory's physical lease.
    pub fn matches_environment(
        &self,
        directory: impl AsRef<Path>,
        manifest: &Manifest,
    ) -> Result<bool> {
        Ok(self.matches_directory(directory)? && self.root.matches_manifest(manifest)?)
    }
    /// Directory-only ownership proof for consumers of this pin's own immutable
    /// manifest. This does not validate any caller-supplied manifest or graph.
    pub fn matches_directory(&self, directory: impl AsRef<Path>) -> Result<bool> {
        self.inner.check()?;
        let metadata = fs::symlink_metadata(directory)?;
        Ok(metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && Identity::of(&metadata) == self.inner.identity)
    }
    /// Diagnostic path, not an API for changing the record.
    pub fn record_path(&self) -> &Path {
        &self.path
    }
}
impl Drop for PhysicalPin {
    fn drop(&mut self) {
        if self.lease.creator == std::process::id() {
            // Never wait from Drop: registration can unwind while it still owns
            // Registry. Discovery SH also keeps sealed membership immutable.
            let _ = generations::release(&self.inner, &self.path, &self.lease.file, self.birth);
        }
    }
}
struct Stale {
    path: PathBuf,
    lease: Lease,
}
/// Retains independent stale-file leases until cleanup or drop. The borrowed
/// maintenance guard prevents captures until this result has been consumed.
pub struct PinScan<'a> {
    roots: Vec<PinnedRoot>,
    stale: Vec<Stale>,
    owner: &'a mut MaintenanceLease,
    _registry: Lease,
    _generations: Vec<(u64, File, Lease)>,
}
impl PinScan<'_> {
    pub fn live_roots(&self) -> &[PinnedRoot] {
        &self.roots
    }
    pub fn stale_records(&self) -> usize {
        self.stale.len()
    }
    /// All live records were validated by scan before the first possible unlink.
    pub fn cleanup_stale(self) -> Result<usize> {
        self.owner.inner.check()?;
        let mut removed = 0;
        for stale in &self.stale {
            if remove_owned(&stale.path, &stale.lease.file)? {
                generations::removed(&self.owner.inner, &self._registry.file, &stale.lease.file)?;
                removed += 1;
            }
        }
        generations::retire_empty(&self.owner.inner, &self._registry.file, &self._generations)?;
        Ok(removed)
    }
}

fn opened(path: &Path, create: bool, directory: bool) -> Result<File> {
    // Share only this operation's checked policy. Revalidate its creator,
    // namespace and actual opened file below; no authority survives this call.
    let access = if create {
        let access = crate::access::context_for_entry(path)?;
        access.validate_parent(path)?;
        Some(access)
    } else {
        None
    };
    let mut options = match &access {
        Some(access) => access.options()?,
        None => OpenOptions::new(),
    };
    options
        .read(true)
        .write(!directory)
        .create(create)
        .truncate(false)
        .custom_flags(
            libc::O_NOFOLLOW | libc::O_CLOEXEC | if directory { libc::O_DIRECTORY } else { 0 },
        );
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !directory {
        match &access {
            Some(access) => access.revalidate_file(&file)?,
            None => crate::access::validate_entry(path, &file)?,
        }
    }
    let named = fs::symlink_metadata(path)?;
    if Identity::of(&metadata) != Identity::of(&named)
        || if directory {
            !metadata.is_dir() || named.file_type().is_symlink()
        } else {
            !metadata.is_file() || !named.is_file() || metadata.nlink() != 1
        }
    {
        return Err(Error::Corrupt("invalid coordination inode"));
    }
    Ok(file)
}
fn ensure_directory(path: &Path) -> Result<File> {
    match crate::access::builder_for_entry(path)?.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    opened(path, false, true)
}
fn gate(path: &Path, shared: bool) -> Result<Lease> {
    let file = opened(path, true, false)?;
    if file.metadata()?.len() != 0 {
        return Err(Error::Corrupt("nonempty coordination gate"));
    }
    Lease::acquire(file, shared)
}
fn remove_owned(path: &Path, file: &File) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || Identity::of(&metadata) != Identity::of(&file.metadata()?)
    {
        return Err(Error::Corrupt("coordination path was replaced"));
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
impl Inner {
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited coordination capability"));
        }
        // These private Files stay owned for the entire Inner lifetime. Their
        // dev/ino identities cannot change while open; only the named binding
        // needs refreshing. Keep every no-follow lookup and after-wait check.
        for (path, identity) in [
            (&self.directory, self.identity),
            (&self.area, self.area_identity),
            (&self.pins, self.pins_identity),
        ] {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || Identity::of(&metadata) != identity
            {
                return Err(Error::Corrupt("coordination directory was replaced"));
            }
        }
        Ok(())
    }
    fn registry(&self) -> Result<Lease> {
        let path = self.area.join("registry");
        let file = opened(&path, true, false)?;
        // The registry may contain the explicitly negotiated generation header.
        // Old implementations require an empty gate and therefore fail closed.
        loop {
            match file.lock() {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        let lease = Lease {
            file,
            creator: std::process::id(),
        };
        // This boundary follows a genuinely blocking kernel wait. Do not touch
        // pin paths or create a record using the pre-wait namespace observation.
        self.check()?;
        let named = fs::symlink_metadata(&path)?;
        if !named.is_file()
            || named.file_type().is_symlink()
            || Identity::of(&named) != Identity::of(&lease.file.metadata()?)
        {
            return Err(Error::Corrupt("coordination registry gate replaced"));
        }
        generations::profile(self, &lease.file)?;
        Ok(lease)
    }
    fn try_registry(&self) -> Result<Lease> {
        self.check()?;
        let path = self.area.join("registry");
        let lease = Lease::acquire(opened(&path, false, false)?, false)?;
        self.check()?;
        let named = fs::symlink_metadata(&path)?;
        if !named.is_file()
            || named.file_type().is_symlink()
            || Identity::of(&named) != Identity::of(&lease.file.metadata()?)
        {
            return Err(Error::Corrupt("coordination registry gate replaced"));
        }
        generations::profile(self, &lease.file)?;
        Ok(lease)
    }
    fn paths(&self) -> Result<Vec<(PathBuf, u128)>> {
        if let Some(paths) = generations::paths(self)? {
            return Ok(paths);
        }
        let mut paths = Vec::new();
        for entry in fs::read_dir(&self.pins)? {
            if paths.len() >= self.limits.max_pin_records {
                return Err(Error::LimitExceeded);
            }
            let entry = entry?;
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(Error::Corrupt("nonregular physical pin"));
            }
            let name = entry.file_name();
            let nonce = parse_name(
                name.to_str()
                    .ok_or(Error::Corrupt("invalid pin filename"))?,
            )?;
            paths.push((entry.path(), nonce));
        }
        Ok(paths)
    }
}
fn initialize(directory: &Path, uuid: [u8; 16], limits: Limits) -> Result<Arc<Inner>> {
    initialize_with_transform(directory, uuid, limits, None)
}
fn initialize_with_transform(
    directory: &Path,
    uuid: [u8; 16],
    limits: Limits,
    transform: Option<Arc<crate::transform::TransformContext>>,
) -> Result<Arc<Inner>> {
    if limits.max_pin_records == 0
        || limits.max_pin_records > MAX_LIMIT
        || limits.max_inventory_entries > 16_777_216
    {
        return Err(Error::InvalidInput("coordination limits"));
    }
    let directory_file = opened(directory, false, true)?;
    let identity = Identity::of(&directory_file.metadata()?);
    let directory = directory.canonicalize()?;
    let area = directory.join(AREA);
    let area_file = ensure_directory(&area)?;
    let area_identity = Identity::of(&area_file.metadata()?);
    let pins = area.join("pins");
    let pins_file = ensure_directory(&pins)?;
    let pins_identity = Identity::of(&pins_file.metadata()?);
    let inner = Arc::new(Inner {
        transform,
        directory,
        area,
        pins,
        identity,
        area_identity,
        pins_identity,
        uuid,
        limits,
        creator: std::process::id(),
        _directory: directory_file,
        _area: area_file,
        _pins: pins_file,
    });
    inner.check()?;
    Ok(inner)
}
impl DirectoryCapture {
    /// Checked nonblocking metadata reads for new client-copy operations only.
    pub fn capture_current_for_copy(self) -> Result<(Coordination, CaptureLease, PhysicalPin)> {
        self.inner.check()?;
        let history = crate::history::HistoryGate::shared(&self.inner.directory)?;
        self.inner.check()?;
        let (manifest, checkpoint) =
            crate::publication::read_copy_current_checkpoint_with_transform(
                &self.inner.directory,
                &history,
                self.inner.transform.clone(),
            )?;
        let (coordination, capture) = self.bind(manifest.uuid)?;
        let pin = capture.register(PinnedRoot {
            manifest,
            checkpoint,
        })?;
        drop(history);
        Ok((coordination, capture, pin))
    }
    /// Captured directory namespace; validates creator and the owned directory FDs.
    pub fn directory(&self) -> Result<&Path> {
        self.inner.check()?;
        Ok(&self.inner.directory)
    }
    pub fn directory_identity(&self) -> Result<(u64, u64)> {
        self.inner.check()?;
        Ok((self.inner.identity.device, self.inner.identity.inode))
    }
    /// Observe both immutable directory fields after one creator/namespace
    /// validation. Capture exclusion stays held; this grants no root or graph.
    pub fn directory_binding(&self) -> Result<(&Path, (u64, u64))> {
        self.inner.check()?;
        Ok((
            &self.inner.directory,
            (self.inner.identity.device, self.inner.identity.inode),
        ))
    }
    /// Select, decode and pin CURRENT while retaining the bootstrap gate.
    /// Realm discovery and descriptor verification occur once, inside storage.
    pub fn capture_current(self) -> Result<(Coordination, CaptureLease, PhysicalPin)> {
        self.inner.check()?;
        let (manifest, checkpoint) = crate::publication::read_current_checkpoint_with_transform(
            &self.inner.directory,
            self.inner.transform.clone(),
        )?;
        let (coordination, capture) = self.bind(manifest.uuid)?;
        let pin = capture.register(PinnedRoot {
            manifest,
            checkpoint,
        })?;
        Ok((coordination, capture, pin))
    }
    /// Capture CURRENT and immutable durability facts in the same metadata decode.
    pub fn capture_current_with_durability(
        self,
    ) -> Result<(
        Coordination,
        CaptureLease,
        PhysicalPin,
        crate::FileDurabilityState,
    )> {
        self.inner.check()?;
        let (manifest, checkpoint, state) =
            crate::publication::read_current_checkpoint_with_durability_and_transform(
                &self.inner.directory,
                self.inner.transform.clone(),
            )?;
        let (coordination, capture) = self.bind(manifest.uuid)?;
        let pin = capture.register(PinnedRoot {
            manifest,
            checkpoint,
        })?;
        Ok((coordination, capture, pin, state))
    }
    /// Capture one authenticated predecessor pair while publication is excluded,
    /// then register A before releasing that short history lease. Capture stays
    /// shared through the caller's graph validation and residency admission.
    pub fn capture_previous(
        self,
    ) -> Result<
        Option<(
            Coordination,
            CaptureLease,
            crate::PreviousSelection,
            PhysicalPin,
        )>,
    > {
        self.inner.check()?;
        let history = crate::history::HistoryGate::shared(&self.inner.directory)?;
        let Some(selection) =
            crate::publication::capture_previous_selection_guarded_with_transform(
                &self.inner.directory,
                &history,
                self.inner.transform.clone(),
            )?
        else {
            return Ok(None);
        };
        let manifest = selection.previous().clone();
        let checkpoint = crate::checkpoint_bytes(&manifest)?;
        let (coordination, capture) = self.bind(manifest.uuid)?;
        let pin = capture.register(PinnedRoot {
            manifest,
            checkpoint,
        })?;
        drop(history);
        Ok(Some((coordination, capture, selection, pin)))
    }
    /// The caller learns/validates the realm while this shared directory gate
    /// remains held. A zero realm can never escape into a usable coordinator.
    pub fn bind(mut self, uuid: [u8; 16]) -> Result<(Coordination, CaptureLease)> {
        self.inner.check()?;
        if uuid == [0; 16] {
            return Err(Error::InvalidInput("null coordination UUID"));
        }
        if self.inner.uuid == [0; 16] {
            Arc::get_mut(&mut self.inner)
                .ok_or(Error::InvalidInput("shared unbound directory capture"))?
                .uuid = uuid;
        } else if self.inner.uuid != uuid {
            return Err(Error::Corrupt("cached capture coordination realm changed"));
        }
        let coordination = Coordination {
            inner: self.inner.clone(),
        };
        let capture = CaptureLease {
            inner: self.inner,
            _gate: self.gate,
        };
        Ok((coordination, capture))
    }
}
impl Coordination {
    /// Retain this genuine namespace, realm and limits while acquiring Capture
    /// before a caller's metadata lock. The selected realm must still match
    /// when the resulting directory capture is consumed by bind.
    pub fn capture_bound_directory(&self) -> Result<DirectoryCapture> {
        self.inner.check()?;
        let lease = gate(&self.inner.area.join("capture"), true)?;
        self.inner.check()?;
        Ok(DirectoryCapture {
            inner: self.inner.clone(),
            gate: lease,
        })
    }
    /// Acquires capture exclusion before a caller reads CURRENT or its descriptor
    /// to discover the UUID. Binding retains this same gate without an unlock gap.
    pub fn capture_directory(
        directory: impl AsRef<Path>,
        limits: Limits,
    ) -> Result<DirectoryCapture> {
        Self::capture_directory_with_transform(directory, limits, None)
    }
    pub fn capture_directory_with_transform(
        directory: impl AsRef<Path>,
        limits: Limits,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<DirectoryCapture> {
        let inner = initialize_with_transform(directory.as_ref(), [0; 16], limits, transform)?;
        let gate = gate(&inner.area.join("capture"), true)?;
        inner.check()?;
        Ok(DirectoryCapture { inner, gate })
    }
    /// Opens writable coordination metadata, binding a caller-verified realm.
    /// It does not select a head or recover/modify any database data file.
    pub fn open(directory: impl AsRef<Path>, uuid: [u8; 16], limits: Limits) -> Result<Self> {
        Self::open_with_transform(directory, uuid, limits, None)
    }
    pub fn open_with_transform(
        directory: impl AsRef<Path>,
        uuid: [u8; 16],
        limits: Limits,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        if uuid == [0; 16] {
            return Err(Error::InvalidInput("null coordination UUID"));
        }
        Ok(Self {
            inner: initialize_with_transform(directory.as_ref(), uuid, limits, transform)?,
        })
    }
    pub fn try_capture(&self) -> Result<CaptureLease> {
        self.inner.check()?;
        let lease = gate(&self.inner.area.join("capture"), true)?;
        self.inner.check()?;
        Ok(CaptureLease {
            inner: self.inner.clone(),
            _gate: lease,
        })
    }
    /// Flush this bound namespace without acquiring a writer or another pin.
    pub fn flush_async(&self) -> Result<crate::FileAsyncFlushReport> {
        crate::durability::async_capture(self.try_capture()?)
    }
    pub fn sync(&self) -> Result<crate::FileSyncReport> {
        crate::durability::sync_capture(self.try_capture()?)
    }
    pub fn try_maintenance(&self) -> Result<MaintenanceLease> {
        self.inner.check()?;
        let lease = gate(&self.inner.area.join("capture"), false)?;
        self.inner.check()?;
        Ok(MaintenanceLease {
            inner: self.inner.clone(),
            _gate: lease,
        })
    }
}
fn parse_name(name: &str) -> Result<u128> {
    let hex = name
        .strip_prefix("pin-")
        .and_then(|value| value.strip_suffix(".lease"))
        .ok_or(Error::Corrupt("invalid pin filename"))?;
    if hex.len() != 32
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Corrupt("invalid pin filename"));
    }
    let nonce = u128::from_str_radix(hex, 16).map_err(|_| Error::Corrupt("invalid pin nonce"))?;
    if nonce == 0 {
        return Err(Error::Corrupt("null pin nonce"));
    }
    Ok(nonce)
}
fn root(inner: &Inner, checkpoint: &[u8; 64]) -> Result<PinnedRoot> {
    checkpoint_identity(checkpoint)?;
    let manifest = crate::publication::read_checkpoint_with_transform(
        &inner.directory,
        checkpoint,
        inner.transform.clone(),
    )?;
    if manifest.uuid != inner.uuid || checkpoint_bytes(&manifest)? != *checkpoint {
        return Err(Error::Corrupt("physical pin checkpoint binding"));
    }
    Ok(PinnedRoot {
        manifest,
        checkpoint: *checkpoint,
    })
}
fn encode(inner: &Inner, root: &PinnedRoot, nonce: u128) -> [u8; RECORD_BYTES] {
    let mut bytes = [0; RECORD_BYTES];
    bytes[..8].copy_from_slice(b"ULMDBPIN");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(RECORD_BYTES as u16).to_le_bytes());
    bytes[16..24].copy_from_slice(&inner.identity.device.to_le_bytes());
    bytes[24..32].copy_from_slice(&inner.identity.inode.to_le_bytes());
    bytes[32..48].copy_from_slice(&inner.uuid);
    bytes[48..112].copy_from_slice(&root.checkpoint);
    bytes[112..120].copy_from_slice(&root.manifest.sequence.to_le_bytes());
    bytes[120..136].copy_from_slice(&root.manifest.commit.to_le_bytes());
    bytes[136..152].copy_from_slice(&nonce.to_le_bytes());
    let checksum = crate::publication::crc(&bytes[..152]);
    bytes[152..156].copy_from_slice(&checksum.to_le_bytes());
    bytes
}
fn decode(inner: &Inner, file: &mut File, nonce: u128) -> Result<PinnedRoot> {
    if file.metadata()?.len() != RECORD_BYTES as u64 {
        return Err(Error::Corrupt("physical pin length"));
    }
    let mut bytes = [0; RECORD_BYTES];
    file.read_exact(&mut bytes)?;
    if bytes[8..10] != 1u16.to_le_bytes() {
        generations::validate_record(inner, file, &bytes, nonce)?;
        generations::record_source(&bytes)?;
    }
    if &bytes[..8] == b"ULMDBPIN" && bytes[8..10] == 2u16.to_le_bytes() {
        // A live opaque predecessor source is protected but not yet descriptor
        // authority. Full scanners must defer rather than omit its assets.
        return Err(Error::Busy);
    }
    if &bytes[..8] != b"ULMDBPIN"
        || !(bytes[8..10] == 1u16.to_le_bytes()
            || bytes[8..10] == 3u16.to_le_bytes()
            || bytes[8..10] == 4u16.to_le_bytes())
        || bytes[10..12] != (RECORD_BYTES as u16).to_le_bytes()
        || bytes[12..16] != [0; 4]
        || bytes[156..] != [0; 4]
        || bytes[152..156] != crate::publication::crc(&bytes[..152]).to_le_bytes()
        || bytes[16..24] != inner.identity.device.to_le_bytes()
        || bytes[24..32] != inner.identity.inode.to_le_bytes()
        || bytes[32..48] != inner.uuid
        || bytes[136..152] != nonce.to_le_bytes()
    {
        return Err(Error::Corrupt("physical pin record binding"));
    }
    let checkpoint = bytes[48..112]
        .try_into()
        .map_err(|_| Error::Corrupt("physical pin checkpoint"))?;
    let root = root(inner, &checkpoint)?;
    if bytes[8..10] != 3u16.to_le_bytes()
        && (bytes[112..120] != root.manifest.sequence.to_le_bytes()
            || bytes[120..136] != root.manifest.commit.to_le_bytes())
    {
        return Err(Error::Corrupt("physical pin logical identity"));
    }
    Ok(root)
}
impl CaptureLease {
    pub(crate) fn transform_context(&self) -> Option<Arc<crate::transform::TransformContext>> {
        self.inner.transform.clone()
    }
    /// Keep both actual roots registered before releasing short History SH.
    /// Does not follow the predecessor of a facade's older chosen view.
    pub fn capture_copy_pair_for(&self, selected: PhysicalPin) -> Result<PhysicalCopyPair> {
        self.inner.check()?;
        if !selected.matches_directory(&self.inner.directory)?
            || selected.root.manifest.uuid != self.inner.uuid
        {
            return Err(Error::InvalidInput("copy selected pin namespace"));
        }
        let history = crate::history::HistoryGate::shared(&self.inner.directory)?;
        self.inner.check()?;
        let (manifest, previous) = crate::publication::capture_copy_roots_expected_with_transform(
            &self.inner.directory,
            &selected.root.checkpoint,
            &history,
            self.inner.transform.clone(),
        )?;
        if manifest.uuid != self.inner.uuid
            || checkpoint_bytes(&manifest)? != selected.root.checkpoint
        {
            return Err(Error::Corrupt("copy selected pin binding"));
        }
        let predecessor = previous
            .map(|manifest| {
                let checkpoint = checkpoint_bytes(&manifest)?;
                self.register(PinnedRoot {
                    manifest,
                    checkpoint,
                })
            })
            .transpose()?;
        drop(history);
        Ok(PhysicalCopyPair {
            selected,
            predecessor,
        })
    }
    pub(crate) fn sync_namespace(&self) -> Result<(&Path, [u8; 16])> {
        self.inner.check()?;
        Ok((&self.inner.directory, self.inner.uuid))
    }
    /// Capture A only if CURRENT still matches this same-directory authenticated
    /// pin. A real full-checkpoint change is Busy before any predecessor IO;
    /// stable corruption and absent history retain their own outcomes.
    pub fn capture_previous_for(
        &self,
        selected: PhysicalPin,
    ) -> Result<Option<(crate::PreviousSelection, PhysicalPin)>> {
        self.inner.check()?;
        if !selected.matches_directory(&self.inner.directory)?
            || selected.root.manifest.uuid != self.inner.uuid
        {
            return Err(Error::InvalidInput(
                "previous capture selected pin namespace",
            ));
        }
        let history = crate::history::HistoryGate::shared(&self.inner.directory)?;
        let Some(selection) =
            crate::publication::capture_previous_selection_expected_with_transform(
                &self.inner.directory,
                &history,
                Some(&selected.root.checkpoint),
                self.inner.transform.clone(),
            )?
        else {
            return Ok(None);
        };
        let manifest = selection.previous().clone();
        let checkpoint = *selection.previous_checkpoint();
        // The same genesis root needs no second registration. For a changed
        // view, release the old D pin before registering A; Capture still
        // excludes reclamation and keeps admission within the original quota.
        let pin = if selected.root.checkpoint == checkpoint {
            selected
        } else {
            drop(selected);
            self.register(PinnedRoot {
                manifest,
                checkpoint,
            })?
        };
        drop(history);
        Ok(Some((selection, pin)))
    }

    /// Verify and pin one complete current descriptor without decoding it twice.
    /// The returned manifest is descriptor authority, not a graph certificate.
    pub fn pin_current(&self) -> Result<PhysicalPin> {
        self.inner.check()?;
        let (manifest, checkpoint) = crate::publication::read_current_checkpoint_with_transform(
            &self.inner.directory,
            self.inner.transform.clone(),
        )?;
        if manifest.uuid != self.inner.uuid {
            return Err(Error::Corrupt("current physical pin realm mismatch"));
        }
        self.register(PinnedRoot {
            manifest,
            checkpoint,
        })
    }
    /// Return the pin and durability facts from one actual CURRENT decode.
    pub fn pin_current_with_durability(&self) -> Result<(PhysicalPin, crate::FileDurabilityState)> {
        self.inner.check()?;
        let (manifest, checkpoint, state) =
            crate::publication::read_current_checkpoint_with_durability_and_transform(
                &self.inner.directory,
                self.inner.transform.clone(),
            )?;
        if manifest.uuid != self.inner.uuid {
            return Err(Error::Corrupt("current physical pin realm mismatch"));
        }
        let pin = self.register(PinnedRoot {
            manifest,
            checkpoint,
        })?;
        Ok((pin, state))
    }
    pub fn coordination(&self) -> Coordination {
        Coordination {
            inner: self.inner.clone(),
        }
    }
    /// Registers one persisted root under capture exclusion. This validates its
    /// descriptor/checkpoint binding only; callers separately validate data.
    pub fn pin_checkpoint(&self, checkpoint: &[u8; 64]) -> Result<PhysicalPin> {
        self.inner.check()?;
        let root = root(&self.inner, checkpoint)?;
        self.register(root)
    }
    /// Register a newly sealed descriptor without reopening or decoding its
    /// inventory. The opaque storage token binds directory identity and exact
    /// immutable bytes; this is not proof of the logical data graph. The caller
    /// retains its writer fence until publication/abort as for the token itself.
    pub fn pin_prepared(
        &self,
        publication: &PreparedPublication,
        manifest: &Manifest,
    ) -> Result<PhysicalPin> {
        self.inner.check()?;
        let checkpoint = publication.validated_pin_checkpoint(
            (self.inner.identity.device, self.inner.identity.inode),
            self.inner.uuid,
            manifest,
        )?;
        self.register(PinnedRoot {
            manifest: manifest.clone(),
            checkpoint,
        })
    }
    fn register(&self, root: PinnedRoot) -> Result<PhysicalPin> {
        self.inner.check()?;
        let _registry = self.inner.registry()?;
        let pin_directory = match generations::reserve_birth(&self.inner, &_registry.file)? {
            Some(directory) => directory,
            None => {
                if self.inner.paths()?.len() >= self.inner.limits.max_pin_records {
                    return Err(Error::LimitExceeded);
                }
                self.inner.pins.clone()
            }
        };
        let nonce = new_identity()?;
        let birth = generations::birth(&self.inner, &pin_directory)?;
        let path = pin_directory.join(format!("pin-{nonce:032x}.lease"));
        let file = crate::access::options_for_entry(&path)?
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        crate::access::validate_entry(&path, &file)?;
        let lease = Lease::acquire(file, false)?;
        let mut pin = PhysicalPin {
            inner: self.inner.clone(),
            path,
            lease,
            root,
            birth,
        };
        pin.lease
            .file
            .write_all(&encode(&self.inner, &pin.root, nonce))?;
        // Pins are host-lifetime leases, not durable retention records. A power
        // loss destroys their live owners; no data publication relies on a pin
        // record surviving it. Completed writes precede capture-gate release.
        self.inner.check()?;
        Ok(pin)
    }
}
impl MaintenanceLease {
    pub(crate) fn fixed_reuse_check(&self, directory: &Path, uuid: [u8; 16]) -> Result<()> {
        self.check()?;
        if directory != self.inner.directory || uuid != self.inner.uuid {
            return Err(Error::Corrupt("fixed reuse maintenance namespace"));
        }
        Ok(())
    }
    /// Revalidate the same held exclusive gate without reacquiring any lock.
    pub fn check(&self) -> Result<()> {
        self.inner.check()?;
        use std::os::unix::fs::MetadataExt;
        let named = fs::symlink_metadata(self.inner.area.join("capture"))?;
        let held = self._gate.file.metadata()?;
        if !named.is_file()
            || named.file_type().is_symlink()
            || named.len() != 0
            || named.dev() != held.dev()
            || named.ino() != held.ino()
        {
            return Err(Error::Corrupt("maintenance capture gate replaced"));
        }
        Ok(())
    }

    /// Persist an exact historical descriptor before the caller retires an
    /// unreferenced cohort. Caller also retains native writer exclusion.
    pub fn archive_cohort_history(
        &self,
        generation: u128,
        max_bytes: u64,
        max_inventory_entries: usize,
    ) -> Result<crate::HistoryArchiveReport> {
        self.check()?;
        let report = crate::cohort_history::archive(
            &self.inner.directory,
            self.inner.uuid,
            generation,
            max_bytes,
            max_inventory_entries,
            self.inner.transform.clone(),
        )?;
        self.check()?;
        Ok(report)
    }

    /// Reads at most max_pin_records fixed-size records. No removal occurs until
    /// every live descriptor binding has passed validation.
    pub fn scan(&mut self) -> Result<PinScan<'_>> {
        self.inner.check()?;
        let registry = self.inner.registry()?;
        let generations = generations::cleanup_leases(&self.inner, &registry.file)?;
        let paths = self.inner.paths()?;
        let mut roots = Vec::new();
        let mut stale = Vec::new();
        let mut inventory_entries = 0usize;
        for (path, nonce) in paths {
            let mut file = match opened(&path, false, false) {
                Ok(file) => file,
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            match file.try_lock() {
                Ok(()) => stale.push(Stale {
                    path,
                    lease: Lease {
                        file,
                        creator: std::process::id(),
                    },
                }),
                Err(TryLockError::WouldBlock) => {
                    let root = decode(&self.inner, &mut file, nonce)?;
                    inventory_entries = inventory_entries
                        .checked_add(root.manifest.segments.len())
                        .filter(|count| *count <= self.inner.limits.max_inventory_entries)
                        .ok_or(Error::LimitExceeded)?;
                    roots.push(root);
                }
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        self.inner.check()?;
        Ok(PinScan {
            roots,
            stale,
            owner: self,
            _registry: registry,
            _generations: generations,
        })
    }
}
