//! Fallible fresh-head file transactions for cooperating native and C clients.
use crate::{Environment, Error, ReadTxn, ResidencyPolicy, Result, WriteTxn};
use std::path::{Path, PathBuf};
use ulmdb_storage::coordination::{Coordination, WriterLease};
mod reuse;

type DirectoryIdentity = ulmdb_storage::coordination::DirectoryIdentity;

// Module-private, non-Clone, and consumed immediately by the loader below.
// No caller can assemble this from unchecked paths or carry it across its own
// callbacks/waits. The plain-capture entry point accepts no transform or cache.
struct NamespaceCheckedPin<'a> {
    directory: &'a Path,
    pin: ulmdb_storage::coordination::PhysicalPin,
}

fn identity(path: &Path) -> Result<DirectoryIdentity> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Corrupt("invalid coordinated environment directory"));
        }
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        let m = ulmdb_platform::path_identity(path)?;
        if !m.is_dir() {
            return Err(Error::Corrupt("invalid coordinated environment directory"));
        }
        Ok((m.volume, m.file_id))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "coordinated file transactions are unavailable on this platform",
        )
        .into())
    }
}

/// An explicit cooperating file-client API. Each read captures CURRENT afresh;
/// each update owns a newly opened native writer, so no cached head goes stale.
/// Ordinary `Environment` retains its exclusive lifetime ownership contract.
pub struct CoordinatedEnvironment {
    admission_reuse: reuse::Cache,
    cache: Option<crate::chunked::CacheOptions>,
    directory: PathBuf,
    identity: DirectoryIdentity,
    uuid: [u8; 16],
    creator: u32,
    policy: ResidencyPolicy,
    read_ahead: crate::ReadAhead,
    coordination: Coordination,
    transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
}
impl CoordinatedEnvironment {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_residency(path, ResidencyPolicy::Demand)
    }
    pub fn open_with_residency(path: impl AsRef<Path>, policy: ResidencyPolicy) -> Result<Self> {
        Self::open_with_initial_read_and_residency(path, policy).map(|(environment, _)| environment)
    }
    pub fn open_with_residency_and_read_ahead(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::open_with_initial_read_and_mapping_policy(path, policy, read_ahead)
            .map(|(environment, _)| environment)
    }
    /// Return the validated, pinned snapshot used while opening this wrapper.
    pub fn open_with_initial_read(path: impl AsRef<Path>) -> Result<(Self, ReadTxn)> {
        Self::open_with_initial_read_and_residency(path, ResidencyPolicy::Demand)
    }
    /// Reuse only the namespace and policy of an admitted, physically pinned
    /// file reader. This neither certifies/re-audits its logical graph nor
    /// caches a serving snapshot. Later begin_read/export validation is unchanged.
    pub fn from_admitted_file_read(
        path: impl AsRef<Path>,
        read: &ReadTxn,
        policy: ResidencyPolicy,
    ) -> Result<Self> {
        if read.snapshot.range_backed {
            return Err(Error::InvalidInput(
                "Range coordinated namespace unsupported",
            ));
        }
        let directory = path.as_ref().canonicalize()?;
        read.snapshot.check_physical_pin(&directory)?;
        let pin = read
            .snapshot
            .physical_pin
            .as_ref()
            .ok_or(Error::InvalidInput(
                "coordinated namespace requires a physical file pin",
            ))?;
        let admission = read.snapshot.admission.as_ref().ok_or(Error::InvalidInput(
            "coordinated namespace requires admitted mappings",
        ))?;
        if !admission.is_for_table(&read.snapshot.segments) || admission.report().policy != policy {
            return Err(Error::InvalidInput(
                "coordinated namespace residency policy mismatch",
            ));
        }
        Ok(Self {
            admission_reuse: Default::default(),
            cache: crate::chunked::CacheOptions::from_table(&read.snapshot.segments),
            read_ahead: read.read_ahead(),
            identity: identity(&directory)?,
            directory,
            uuid: read.snapshot.manifest.uuid,
            creator: std::process::id(),
            policy,
            coordination: pin.coordination(),
            transform: read.snapshot.segments.transform().cloned(),
        })
    }
    /// Open with the requested residency and return its validated, physically pinned read.
    pub fn open_with_initial_read_and_residency(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
    ) -> Result<(Self, ReadTxn)> {
        Self::open_with_initial_read_and_mapping_policy(path, policy, crate::ReadAhead::Normal)
    }
    pub fn open_with_initial_read_and_mapping_policy(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<(Self, ReadTxn)> {
        Self::open_with_initial_read_and_transform_options(path, policy, read_ahead, None)
    }
    pub fn open_with_transform(
        path: impl AsRef<Path>,
        context: std::sync::Arc<ulmdb_storage::transform::TransformContext>,
    ) -> Result<Self> {
        Self::open_with_initial_read_and_transform_options(
            path,
            ResidencyPolicy::Demand,
            crate::ReadAhead::Normal,
            Some(context),
        )
        .map(|(env, _)| env)
    }
    pub fn open_with_initial_read_and_transform_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<(Self, ReadTxn)> {
        Self::open_with_initial_read_and_cache_options(path, policy, read_ahead, transform, None)
    }
    pub fn open_with_initial_read_and_cache_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<(Self, ReadTxn)> {
        if let Some(cache) = &cache {
            cache.validate()?;
            if policy != ResidencyPolicy::Demand {
                return Err(Error::InvalidInput("chunk cache requires Demand"));
            }
        }
        read_ahead.validate()?;
        let directory = path.as_ref().canonicalize()?;
        let identity = identity(&directory)?;
        if ulmdb_storage::range::is_selected(&directory)? {
            return Err(Error::InvalidInput(
                "coordinated file API does not recover range journals",
            ));
        }
        let unbound = Coordination::capture_directory_with_transform(
            &directory,
            Default::default(),
            transform.clone(),
        )?;
        let (coordination, capture, pin) = unbound.capture_current()?;
        let uuid = pin.root().manifest().uuid;
        let environment = Self {
            admission_reuse: Default::default(),
            cache: cache.clone(),
            directory,
            identity,
            uuid,
            creator: std::process::id(),
            policy,
            read_ahead,
            coordination,
            transform: transform.clone(),
        };
        let read = environment.admission_reuse.admit(
            &environment.directory,
            policy,
            pin,
            read_ahead,
            transform,
            cache,
        )?;
        drop(capture);
        Ok((environment, read))
    }
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited coordinated environment"));
        }
        if identity(&self.directory)? != self.identity {
            return Err(Error::Corrupt("coordinated environment was replaced"));
        }
        Ok(())
    }
    pub fn begin_read(&self) -> Result<ReadTxn> {
        self.check()?;
        let capture = self.coordination.try_capture()?;
        let pin=capture.pin_current()?;
        let read=self.admit_pin(&self.directory,pin,self.read_ahead)?;
        drop(capture);
        Ok(read)
    }
    pub(crate) fn admit_pin(
        &self, directory:&Path, pin:ulmdb_storage::coordination::PhysicalPin,
        advice:crate::ReadAhead,
    ) -> Result<ReadTxn> {
        self.check()?;
        if directory!=self.directory || pin.root().manifest().uuid!=self.uuid {
            return Err(Error::Corrupt("captured admission namespace/realm mismatch"));
        }
        self.admission_reuse.admit(directory,self.policy,pin,advice,self.transform.clone(),self.cache.clone())
    }
    /// Admit this owner's freshly captured view, reusing only an authenticated
    /// immutable graph. Source contents and physical protection remain fresh.
    pub fn admit_file_capture_with_read_ahead(
        &self, capture:crate::OwnedFileCapture, advice:crate::ReadAhead,
    ) -> Result<(ReadTxn,Option<ulmdb_storage::PreviousSelection>)> {
        capture.admit_with_coordinator(self,advice)
    }
    /// Acquire namespace exclusion for a subsequent opaque file capture. This
    /// reuses this owner's directory descriptors and immutable realm/limits;
    /// it neither captures a serving head nor certifies a logical graph.
    pub fn capture_directory(&self) -> Result<ulmdb_storage::coordination::DirectoryCapture> {
        self.check()?;
        self.coordination
            .capture_bound_directory()
            .map_err(Into::into)
    }
    /// Flush selected file roots through this owner's checked namespace.
    pub fn flush_async(&self) -> Result<ulmdb_storage::FileAsyncFlushReport> {
        self.check()?;
        self.coordination.flush_async().map_err(Into::into)
    }
    pub fn sync(&self) -> Result<ulmdb_storage::FileSyncReport> {
        self.check()?;
        self.coordination.sync().map_err(Into::into)
    }
    pub fn begin_read_with_empty_kv(&self, name: &[u8]) -> Result<ReadTxn> {
        if name.is_empty() || name.len() > 255 {
            return Err(Error::InvalidInput(
                "virtual database name must have 1..255 bytes",
            ));
        }
        self.begin_read()?.project_empty_kv(name)
    }
    fn with_native<T>(&self, action: impl FnOnce(&Environment) -> Result<T>) -> Result<T> {
        self.check()?;
        let _lease = WriterLease::acquire(&self.directory)?;
        self.check()?;
        let native = Environment::open_with_cache_options(
            &self.directory,
            self.policy,
            self.read_ahead,
            self.transform.clone(),
            self.cache.clone(),
        )?;
        self.check()?;
        if native.current.load().manifest.uuid != self.uuid {
            return Err(Error::Corrupt("coordinated environment realm changed"));
        }
        // native drops before the outer writer lease, including on unwind.
        action(&native)
    }
    pub fn update<T>(&self, action: impl FnOnce(&mut WriteTxn<'_>) -> Result<T>) -> Result<T> {
        self.with_native(|native| {
            let mut write = native.begin_write()?;
            let result = action(&mut write)?;
            write.commit()?;
            Ok(result)
        })
    }
    pub fn reclaim(&self) -> Result<crate::ReclaimReport> {
        self.with_native(Environment::reclaim)
    }
    pub fn compact(&self) -> Result<crate::CompactionReport> {
        self.with_native(Environment::compact)
    }
    pub fn prune_inventory(&self) -> Result<crate::PruneReport> {
        self.with_native(Environment::prune_inventory)
    }
}

impl ReadTxn {
    /// Capture and physically pin a file snapshot against cooperating native
    /// reclamation. Unlike `open_snapshot`, this writes coordination metadata.
    /// Capture races may return Busy; no journal recovery or healing is performed.
    pub fn open_coordinated_snapshot(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_coordinated_snapshot_with_read_ahead(path, crate::ReadAhead::Normal)
    }
    pub fn open_coordinated_snapshot_with_read_ahead(
        path: impl AsRef<Path>,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::open_coordinated_snapshot_with_transform_options(path, read_ahead, None)
    }
    pub fn open_coordinated_snapshot_with_transform_options(
        path: impl AsRef<Path>,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::open_coordinated_snapshot_with_cache_options(path, read_ahead, transform, None)
    }
    pub fn open_coordinated_snapshot_with_cache_options(
        path: impl AsRef<Path>,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        read_ahead.validate()?;
        let directory = path.as_ref().canonicalize()?;
        let directory = directory.as_path();
        if ulmdb_storage::range::is_selected(directory)? {
            return Err(Error::InvalidInput(
                "coordinated snapshots require a file environment",
            ));
        }
        let unbound = Coordination::capture_directory_with_transform(
            directory,
            Default::default(),
            transform.clone(),
        )?;
        let (_, capture, pin) = unbound.capture_current()?;
        let read = Self::from_physical_pin_with_cache_options(
            directory,
            ResidencyPolicy::Demand,
            pin,
            read_ahead,
            transform,
            cache,
        )?;
        drop(capture);
        Ok(read)
    }
    pub(crate) fn capture_coordinated_with_read_ahead(
        directory: &Path,
        uuid: [u8; 16],
        policy: ResidencyPolicy,
        capture: &ulmdb_storage::coordination::CaptureLease,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::capture_coordinated_with_transform(directory, uuid, policy, capture, read_ahead, None)
    }
    pub(crate) fn capture_coordinated_with_transform(
        directory: &Path,
        uuid: [u8; 16],
        policy: ResidencyPolicy,
        capture: &ulmdb_storage::coordination::CaptureLease,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        read_ahead.validate()?;
        let pin = capture.pin_current()?;
        if pin.root().manifest().uuid != uuid {
            return Err(Error::Corrupt("snapshot realm changed during capture"));
        }
        Self::from_physical_pin_with_transform(directory, policy, pin, read_ahead, transform)
    }
    pub(crate) fn from_physical_pin_with_read_ahead(
        directory: &Path,
        policy: ResidencyPolicy,
        pin: ulmdb_storage::coordination::PhysicalPin,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::from_physical_pin_with_transform(directory, policy, pin, read_ahead, None)
    }
    pub(crate) fn from_physical_pin_with_transform(
        directory: &Path,
        policy: ResidencyPolicy,
        pin: ulmdb_storage::coordination::PhysicalPin,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::from_physical_pin_with_cache_options(
            directory, policy, pin, read_ahead, transform, None,
        )
    }
    pub(crate) fn capture_coordinated_with_cache_options(
        directory: &Path,
        uuid: [u8; 16],
        policy: ResidencyPolicy,
        capture: &ulmdb_storage::coordination::CaptureLease,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        let pin = capture.pin_current()?;
        if pin.root().manifest().uuid != uuid {
            return Err(Error::Corrupt("snapshot realm changed during capture"));
        }
        Self::from_physical_pin_with_cache_options(
            directory, policy, pin, read_ahead, transform, cache,
        )
    }
    pub(crate) fn from_physical_pin_with_cache_options(
        directory: &Path,
        policy: ResidencyPolicy,
        pin: ulmdb_storage::coordination::PhysicalPin,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        if let Some(cache) = &cache {
            cache.validate()?;
            if policy != ResidencyPolicy::Demand {
                return Err(Error::InvalidInput("chunk cache requires Demand"));
            }
        }
        read_ahead.validate()?;
        if transform.is_some()
            && ulmdb_storage::transform::check_configuration(directory, transform.as_ref())?
                .is_some_and(|uuid| uuid != pin.root().manifest().uuid)
        {
            return Err(Error::CryptoMismatch);
        }
        if !pin.matches_directory(directory)? {
            return Err(Error::InvalidInput(
                "physical snapshot pin belongs to another source",
            ));
        }
        Self::from_namespace_checked_pin(
            NamespaceCheckedPin { directory, pin },
            policy,
            read_ahead,
            transform,
            cache,
        )
    }
    /// Only canonical, untransformed, uncached OwnedFileCapture admission uses
    /// this entry. It preserves that caller's outer error and validation order,
    /// and cannot introduce a callback before consuming the checked pin.
    pub(crate) fn from_plain_file_capture(
        directory: &Path,
        policy: ResidencyPolicy,
        pin: ulmdb_storage::coordination::PhysicalPin,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        read_ahead.validate()?;
        if !pin.matches_directory(directory)? {
            return Err(Error::Corrupt("file capture directory changed"));
        }
        Self::from_namespace_checked_pin(
            NamespaceCheckedPin { directory, pin },
            policy,
            read_ahead,
            None,
            None,
        )
    }
    fn from_namespace_checked_pin(
        checked: NamespaceCheckedPin<'_>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        let NamespaceCheckedPin { directory, pin } = checked;
        let manifest = pin.root().manifest().clone();
        let owner = super::client_snapshot::snapshot_owner()?;
        let mut snapshot = match &cache {
            Some(cache) => crate::environment::load_snapshot_with_cache(
                directory, manifest, owner, read_ahead, transform, cache,
            )?,
            None => crate::environment::load_snapshot_with_transform(
                directory, manifest, owner, read_ahead, transform,
            )?,
        };
        if cache.is_none() {
            crate::backup::validate_file_for_admission(&snapshot.segments, &snapshot.manifest)?;
        }
        let controller = ulmdb_storage::ResidencyController::new(policy)?;
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        snapshot.physical_pin = Some(std::sync::Arc::new(pin));
        Ok(Self {
            snapshot: std::sync::Arc::new(snapshot),
        })
    }
    pub fn open_coordinated_snapshot_with_empty_kv(
        path: impl AsRef<Path>,
        name: &[u8],
    ) -> Result<Self> {
        if name.is_empty() || name.len() > 255 {
            return Err(Error::InvalidInput(
                "virtual database name must have 1..255 bytes",
            ));
        }
        Self::open_coordinated_snapshot(path)?.project_empty_kv(name)
    }
}
