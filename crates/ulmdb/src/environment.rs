use crate::{Error, ReadTxn, Result, WriteTxn, catalog::Database};
use arc_swap::ArcSwap;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};
use ulmdb_storage::{
    Arena, Manifest, ResidencyAdmission, ResidencyController, ResidencyPolicy, SegmentTable,
};

pub(crate) struct Snapshot {
    pub range_backed: bool,
    pub external: Option<Arc<crate::ExternalSynchronization>>,
    pub physical_pin: Option<Arc<ulmdb_storage::coordination::PhysicalPin>>,
    #[cfg(feature = "range-lifecycle")]
    pub range_checkpoint: Option<ulmdb_storage::range::RangeCheckpoint>,
    pub owner: u128,
    pub manifest: Manifest,
    pub segments: SegmentTable,
    pub catalog: BTreeMap<Vec<u8>, Database>,
    pub admission: Option<ResidencyAdmission>,
}
fn owner_identity() -> Result<u128> {
    // Handles are process-local. A checked monotonic namespace avoids relying
    // on random noncollision when distinguishing two live/restored environments.
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map(u128::from)
        .map_err(|_| Error::LimitExceeded)
}
/// Publication strategy persisted by the selected manifest; query indexes are shared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PublicationMode {
    #[default]
    Legacy,
    Cohort,
    /// Persistent path-copied segment metadata; candidate format 0.4.
    Indexed,
    /// Experimental format5 sealed-range Environment; lifecycle qualification pending.
    RangeV5,
    /// Experimental format6 persistent direct physical locator directory.
    Range,
}
pub struct Environment {
    pub(crate) directory: PathBuf,
    pub(crate) current: ArcSwap<Snapshot>,
    pub(crate) writer: Mutex<()>,
    // Weak owners retain no historical data themselves. Live readers/exports
    // keep their roots upgradeable until the reclaimer has enumerated them.
    pub(crate) retired: Mutex<Vec<Weak<Snapshot>>>,
    pub(crate) poisoned: AtomicBool,
    pub(crate) database_ids: AtomicU64,
    pub(crate) durability: AtomicU8,
    pub(crate) file_durability: Mutex<Option<ulmdb_storage::FileDurabilityState>>,
    pub(crate) maintenance: Mutex<crate::maintenance::MaintenanceState>,
    pub(crate) maintenance_enabled: AtomicBool,
    pub(crate) replica_role: Option<crate::replication::ReplicaIdentity>,
    pub(crate) residency: ResidencyController,
    pub(crate) range: Option<Mutex<ulmdb_storage::range::RangeStore>>,
    pub(crate) coordination: Option<ulmdb_storage::coordination::Coordination>,
    pub(crate) external_selected: Mutex<Option<[u8; 64]>>,
    pub(crate) _file_lock: Option<ulmdb_storage::WriterFence>,
}

impl Environment {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::create_with_publication(path, PublicationMode::Legacy)
    }
    pub fn create_with_publication(path: impl AsRef<Path>, mode: PublicationMode) -> Result<Self> {
        Self::create_with_residency(path, mode, ResidencyPolicy::Demand)
    }
    pub fn create_with_residency(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        policy: ResidencyPolicy,
    ) -> Result<Self> {
        Self::create_with_residency_and_read_ahead(path, mode, policy, crate::ReadAhead::Normal)
    }
    /// Create a file environment with a fixed, nonpersistent mapping advice request.
    pub fn create_with_residency_and_read_ahead(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::create_with_geometry(path, mode, policy, read_ahead, crate::PageSize::DEFAULT)
    }
    pub fn create_with_page_size(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        page_size: crate::PageSize,
    ) -> Result<Self> {
        Self::create_with_page_size_and_transform_options(
            path,
            mode,
            page_size,
            ResidencyPolicy::Demand,
            crate::ReadAhead::Normal,
            None,
        )
    }
    pub fn create_with_geometry(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        page_size: crate::PageSize,
    ) -> Result<Self> {
        Self::create_with_page_size_and_transform_options(
            path, mode, page_size, policy, read_ahead, None,
        )
    }
    pub fn create_with_transform(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        context: Arc<ulmdb_storage::transform::TransformContext>,
    ) -> Result<Self> {
        Self::create_with_transform_options(
            path,
            mode,
            ResidencyPolicy::Demand,
            crate::ReadAhead::Normal,
            Some(context),
        )
    }
    pub fn create_with_transform_options(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::create_with_page_size_and_transform_options(
            path,
            mode,
            crate::PageSize::DEFAULT,
            policy,
            read_ahead,
            transform,
        )
    }
    pub fn create_with_page_size_and_transform_options(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        page_size: crate::PageSize,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::create_with_file_access(
            path,
            mode,
            page_size,
            policy,
            read_ahead,
            transform,
            ulmdb_storage::access::FileAccess::PRIVATE,
        )
    }
    pub fn create_with_file_access(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        page_size: crate::PageSize,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        access: ulmdb_storage::access::FileAccess,
    ) -> Result<Self> {
        Self::create_file_inner(
            path, mode, page_size, policy, read_ahead, transform, access, false,
        )
    }
    pub fn create_externally_synchronized(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        page_size: crate::PageSize,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        access: ulmdb_storage::access::FileAccess,
    ) -> Result<Self> {
        Self::create_file_inner(
            path, mode, page_size, policy, read_ahead, transform, access, true,
        )
    }
    fn create_file_inner(
        path: impl AsRef<Path>,
        mode: PublicationMode,
        page_size: crate::PageSize,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        access: ulmdb_storage::access::FileAccess,
        external: bool,
    ) -> Result<Self> {
        if external && !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(Error::InvalidInput(
                "external synchronization requires Linux",
            ));
        }
        read_ahead.validate()?;
        if transform.is_some() && matches!(mode, PublicationMode::RangeV5 | PublicationMode::Range)
        {
            return Err(Error::InvalidInput("Range transforms unsupported"));
        }
        if matches!(mode, PublicationMode::RangeV5 | PublicationMode::Range) {
            if external {
                return Err(Error::InvalidInput("external Range unsupported"));
            }
            if access != ulmdb_storage::access::FileAccess::PRIVATE {
                return Err(Error::InvalidInput("Range file access policy unsupported"));
            }
            if page_size != crate::PageSize::DEFAULT {
                return Err(Error::InvalidInput(
                    "nondefault Range page geometry unsupported",
                ));
            }
            if read_ahead != crate::ReadAhead::Normal {
                return Err(Error::InvalidInput(
                    "Range random mapping advice unsupported",
                ));
            }
            return Self::create_range(path.as_ref(), policy, mode);
        }
        let controller = ResidencyController::new(policy)?;
        let directory = path.as_ref();
        ulmdb_storage::access::create_layout(directory, access)?;
        let lock = if external {
            None
        } else {
            Some(Self::lock(directory)?)
        };
        let mut manifest = Manifest::empty()?;
        manifest.page_size = page_size;
        ulmdb_storage::transform::create_configuration(
            directory,
            manifest.uuid,
            transform.as_ref(),
        )?;
        manifest.format_minor = match mode {
            PublicationMode::Legacy => 2,
            PublicationMode::Cohort => 3,
            PublicationMode::Indexed => 4,
            PublicationMode::RangeV5 | PublicationMode::Range => {
                unreachable!("handled range creation")
            }
        };
        if mode == PublicationMode::Indexed {
            manifest.prepare_indexed(None)?;
        }
        let mut snapshot =
            load_snapshot_with_transform(directory, manifest, 0, read_ahead, transform)?;
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        let publication = ulmdb_storage::prepare_publication(directory, &snapshot.manifest)?;
        if external {
            snapshot.external = Some(Arc::new(crate::ExternalSynchronization::bind(
                directory,
                &snapshot.manifest,
            )?));
        } else {
            snapshot.pin_prepared(directory, &publication)?;
        }
        let env = Self::from_verified_inner(directory, snapshot, lock, controller)?;
        // All fallible residency work precedes initial CURRENT publication.
        let selected = if external {
            let authority = env
                .current
                .load()
                .external
                .clone()
                .ok_or(Error::InvalidInput("missing external bootstrap"))?;
            publication
                .prepare_ready_externally_synchronized(None, &authority.storage)?
                .publish(crate::Durability::Full)?
        } else {
            publication
                .prepare_ready(None)?
                .publish(crate::Durability::Full)?
        };
        env.remember_file_durability(selected)?;
        Ok(env)
    }
    fn create_range(
        directory: &Path,
        policy: ResidencyPolicy,
        mode: PublicationMode,
    ) -> Result<Self> {
        let controller = ResidencyController::new(policy)?;
        ulmdb_storage::create_layout(directory)?;
        let lock = Self::lock(directory)?;
        let (store, state) = if mode == PublicationMode::Range {
            ulmdb_storage::range::RangeStore::create(directory, &controller)?
        } else {
            ulmdb_storage::range::RangeStore::create_v5_compatibility(directory, &controller)?
        };
        let mut snapshot = snapshot_from_parts(state.segments, state.manifest, 0)?;
        snapshot.range_backed = true;
        #[cfg(feature = "range-lifecycle")]
        {
            snapshot.range_checkpoint = state.checkpoint;
        }
        snapshot.admission = Some(state.admission);
        let mut env = Self::from_verified_with_controller(directory, snapshot, lock, controller)?;
        env.range = Some(Mutex::new(store));
        Ok(env)
    }
    fn open_range(
        directory: &Path,
        controller: ResidencyController,
        lock: ulmdb_storage::WriterFence,
    ) -> Result<Self> {
        if directory.join("REPLICA").try_exists()? {
            return Err(Error::InvalidInput(
                "range replica lifecycle pending PACK-03",
            ));
        }
        struct Borrowed<'a>(&'a dyn ulmdb_core::storage::ReadStore);
        impl ulmdb_core::storage::ReadStore for Borrowed<'_> {
            fn page_size(&self) -> usize {
                self.0.page_size()
            }
            fn max_value_size(&self) -> usize {
                self.0.max_value_size()
            }
            fn read(&self, at: ulmdb_core::storage::Reference, len: usize) -> Result<&[u8]> {
                self.0.read(at, len)
            }
        }
        let (store, state) = ulmdb_storage::range::RangeStore::open_checked(
            directory,
            &controller,
            |table, manifest| crate::backup::validate_for_admission(&Borrowed(table), manifest),
        )?;
        let mut snapshot = snapshot_from_parts(state.segments, state.manifest, 0)?;
        snapshot.range_backed = true;
        #[cfg(feature = "range-lifecycle")]
        {
            snapshot.range_checkpoint = state.checkpoint;
        }
        snapshot.admission = Some(state.admission);
        let mut env = Self::from_verified_with_controller(directory, snapshot, lock, controller)?;
        env.range = Some(Mutex::new(store));
        Ok(env)
    }
    pub fn locator_work(&self) -> Option<ulmdb_storage::range::LocatorWork> {
        self.range
            .as_ref()
            .and_then(|store| store.lock().ok())
            .and_then(|store| store.locator_work())
    }
    pub fn is_range_backed(&self) -> bool {
        self.range.is_some()
    }
    pub(crate) fn require_file_backend(&self) -> Result<()> {
        if self.is_range_backed() {
            Err(Error::InvalidInput(
                "range lifecycle pending PACK-03/LIFE-03",
            ))
        } else {
            Ok(())
        }
    }
    pub fn publication_mode(&self) -> PublicationMode {
        if self.range.is_some() {
            return if self.current.load().manifest.format_minor == 6 {
                PublicationMode::Range
            } else {
                PublicationMode::RangeV5
            };
        }
        match self.current.load().manifest.format_minor {
            4 => PublicationMode::Indexed,
            3 => PublicationMode::Cohort,
            _ => PublicationMode::Legacy,
        }
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_residency(path, ResidencyPolicy::Demand)
    }
    pub fn open_with_residency(path: impl AsRef<Path>, policy: ResidencyPolicy) -> Result<Self> {
        Self::open_with_residency_and_read_ahead(path, policy, crate::ReadAhead::Normal)
    }
    /// Open with per-VMA advice; other owners and existing borrowed mappings keep their policies.
    pub fn open_with_residency_and_read_ahead(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::open_with_transform_options(path, policy, read_ahead, None)
    }
    pub fn open_with_file_access(
        path: impl AsRef<Path>,
        access: ulmdb_storage::access::FileAccess,
    ) -> Result<Self> {
        let path = path.as_ref();
        access.validate_root(path)?;
        let actual = ulmdb_storage::access::FileAccessContext::open(path)?;
        if let Some(gid) = access.group() {
            if actual.policy().group() != Some(gid) {
                return Err(Error::InvalidInput(
                    "existing namespace is not shared with requested group",
                ));
            }
        }
        Self::open(path)
    }
    pub fn open_with_transform(
        path: impl AsRef<Path>,
        context: Arc<ulmdb_storage::transform::TransformContext>,
    ) -> Result<Self> {
        Self::open_with_transform_options(
            path,
            ResidencyPolicy::Demand,
            crate::ReadAhead::Normal,
            Some(context),
        )
    }
    pub fn open_with_transform_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::open_with_cache_options(path, policy, read_ahead, transform, None)
    }
    pub fn open_with_cache_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        if let Some(cache) = &cache {
            cache.validate()?;
            if policy != ResidencyPolicy::Demand
                || !cfg!(any(target_os = "linux", target_os = "macos", windows))
            {
                return Err(Error::InvalidInput(
                    "chunk cache requires Linux, macOS or Windows Demand file policy",
                ));
            }
        }
        read_ahead.validate()?;
        let controller = ResidencyController::new(policy)?;
        let directory = path.as_ref().canonicalize()?;
        let directory = directory.as_path();
        let lock = Self::lock(directory)?;
        let configured_uuid =
            ulmdb_storage::transform::check_configuration(directory, transform.as_ref())?;
        if ulmdb_storage::range::is_selected(directory)? {
            if ulmdb_core::work::current_observer().is_some() {
                return Err(Error::InvalidInput("controlled Range snapshot admission is unsupported"));
            }
            if cache.is_some() {
                return Err(Error::InvalidInput("chunk cache does not support Range"));
            }
            if transform.is_some() {
                return Err(Error::InvalidInput("Range transforms unsupported"));
            }
            if read_ahead != crate::ReadAhead::Normal {
                return Err(Error::InvalidInput(
                    "Range random mapping advice unsupported",
                ));
            }
            return Self::open_range(directory, controller, lock);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let (_, capture, pin, durability) =
            ulmdb_storage::coordination::Coordination::capture_directory_with_transform(
                directory,
                Default::default(),
                transform.clone(),
            )?
            .capture_current_with_durability()?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let capture = if ulmdb_core::work::current_observer().is_some() {
            // The exact physical pin now protects every root dependency. A
            // controlled cold load may pause for caller work grants; do not
            // retain shared Capture across those pauses or defer maintenance.
            drop(capture);
            None
        } else {
            Some(capture)
        };
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let mut snapshot = {
            // The storage pin owns the exact decoded CURRENT descriptor. Keep
            // capture through loading/admission and reuse that authority rather
            // than reopening and decoding the same descriptor during pinning.
            if configured_uuid.is_some_and(|uuid| uuid != pin.root().manifest().uuid) {
                return Err(Error::CryptoMismatch);
            }
            let mut snapshot = match &cache {
                Some(cache) => load_snapshot_with_cache(
                    directory,
                    pin.root().manifest().clone(),
                    0,
                    read_ahead,
                    transform.clone(),
                    cache,
                )?,
                None => load_snapshot_with_transform(
                    directory,
                    pin.root().manifest().clone(),
                    0,
                    read_ahead,
                    transform.clone(),
                )?,
            };
            snapshot.physical_pin = Some(Arc::new(pin));
            snapshot
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let mut snapshot = load_snapshot_with_read_ahead(
            directory,
            ulmdb_storage::read_current(directory)?,
            0,
            read_ahead,
        )?;
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if capture.is_none()
            && let Some(pin) = &snapshot.physical_pin
            && !pin.matches_directory(directory)?
        {
            return Err(Error::Corrupt("file namespace changed during admission"));
        }
        let environment =
            Self::from_verified_with_controller(directory, snapshot, lock, controller)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        environment.remember_file_durability(durability)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        drop(capture);
        Ok(environment)
    }
    /// Requested mapping advice, fixed for this owner and inherited by its snapshots.
    pub fn read_ahead(&self) -> crate::ReadAhead {
        self.current.load().segments.read_ahead()
    }
    pub(crate) fn lock(directory: &Path) -> Result<ulmdb_storage::WriterFence> {
        let file = ulmdb_storage::access::options_for_entry(&directory.join("LOCK"))?
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("LOCK"))?;
        ulmdb_storage::access::validate_entry(&directory.join("LOCK"), &file)?;
        ulmdb_storage::WriterFence::try_lock(file).map_err(|_| Error::Busy)
    }
    pub(crate) fn from_verified_with_controller(
        directory: &Path,
        snapshot: Snapshot,
        lock: ulmdb_storage::WriterFence,
        controller: ResidencyController,
    ) -> Result<Self> {
        Self::from_verified_inner(directory, snapshot, Some(lock), controller)
    }
    pub(crate) fn from_verified_inner(
        directory: &Path,
        mut snapshot: Snapshot,
        lock: Option<ulmdb_storage::WriterFence>,
        controller: ResidencyController,
    ) -> Result<Self> {
        if lock.is_none() && snapshot.external.is_none() {
            return Err(Error::InvalidInput("missing owner synchronization"));
        }
        snapshot.check_external()?;

        if !snapshot.admission.as_ref().is_some_and(|ready| {
            ready.is_for(&controller) && ready.is_for_table(&snapshot.segments)
        }) {
            return Err(Error::InvalidInput(
                "snapshot has not passed residency admission",
            ));
        }
        let replica_role = crate::replication::load_role(directory, &snapshot.manifest)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let coordination = if snapshot.range_backed || snapshot.external.is_some() {
            None
        } else {
            Some(match &snapshot.physical_pin {
                Some(pin) => pin.coordination(),
                None => ulmdb_storage::coordination::Coordination::open_with_transform(
                    directory,
                    snapshot.manifest.uuid,
                    Default::default(),
                    snapshot.segments.transform().cloned(),
                )?,
            })
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let coordination: Option<ulmdb_storage::coordination::Coordination> = None;
        if let Some(coordinator) = &coordination {
            if snapshot.physical_pin.is_none() {
                snapshot.pin_with_capture(&coordinator.try_capture()?)?;
            }
            snapshot.check_physical_pin(directory)?;
        }
        snapshot.owner = owner_identity()?;
        let database_ids = AtomicU64::new(snapshot.manifest.next_database);
        let automatic = cfg!(any(target_os = "linux", target_os = "macos", windows))
            && !snapshot.range_backed
            && snapshot.external.is_none()
            && replica_role.is_none();
        Ok(Self {
            directory: directory.canonicalize()?,
            current: ArcSwap::from_pointee(snapshot),
            writer: Mutex::new(()),
            retired: Mutex::new(Vec::new()),
            poisoned: AtomicBool::new(false),
            database_ids,
            durability: AtomicU8::new(0),
            file_durability: Mutex::new(None),
            maintenance: Mutex::new(crate::maintenance::MaintenanceState::for_file_owner(
                automatic,
            )),
            maintenance_enabled: AtomicBool::new(automatic),
            replica_role,
            residency: controller,
            range: None,
            coordination,
            external_selected: Mutex::new(None),
            _file_lock: lock,
        })
    }
    pub fn is_replica(&self) -> bool {
        self.replica_role.is_some()
    }
    /// Return this owner's next database-ID floor, including IDs burned by
    /// aborted writers. This lock-free observation is valid while a writer or
    /// prepared transaction exists and never reads or changes selection files.
    pub fn database_id_floor(&self) -> u64 {
        self.database_ids.load(Ordering::Acquire)
    }
    /// Import a monotonic ID floor before creating a new file writer.
    ///
    /// Compatibility owners can retain this value across short-lived native
    /// environments. No ID is allocated and no descriptor or CURRENT is written;
    /// a later publication persists the floor. An active local writer returns
    /// Busy, including when this method is called by its owning thread.
    pub fn raise_database_id_floor(&self, floor: u64) -> Result<()> {
        self.require_file_backend()?;
        self.require_writable()?;
        if floor == 0 {
            return Err(Error::InvalidInput("database ID floor must be nonzero"));
        }
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.database_ids.fetch_max(floor, Ordering::AcqRel);
        Ok(())
    }
    pub(crate) fn require_writable(&self) -> Result<()> {
        self.current.load().check_external()?;
        if self.is_replica() {
            Err(Error::ReadOnly)
        } else {
            Ok(())
        }
    }
    /// Exact activated file checkpoint. An uncertain owner cannot certify its
    /// durable selection. This getter takes no writer mutex.
    pub fn file_checkpoint(&self) -> Result<[u8; 64]> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.check_file_namespace()?;
        self.begin_read().file_checkpoint()
    }
    pub(crate) fn check_file_namespace(&self) -> Result<()> {
        let snapshot = self.current.load();
        if let Some(external) = &snapshot.external {
            return external.check(&self.directory, &snapshot.manifest);
        }
        if snapshot.range_backed {
            return Err(Error::InvalidInput("Range file checkpoint unsupported"));
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            let pin = snapshot
                .physical_pin
                .as_ref()
                .ok_or(Error::InvalidInput("file owner has no physical pin"))?;
            if !pin.matches_directory(&self.directory)? {
                return Err(Error::Corrupt("file owner namespace changed"));
            }
        }
        Ok(())
    }
    #[inline]
    pub fn begin_read(&self) -> ReadTxn {
        ReadTxn {
            snapshot: self.current.load_full(),
        }
    }
    pub fn begin_write(&self) -> Result<WriteTxn<'_>> {
        self.begin_write_with_durability(crate::Durability::Full)
    }
    /// A local policy can weaken the current environment policy. Environment
    /// changes are read at preparation and final selection, not cached here.
    pub fn begin_write_with_durability(&self, local: crate::Durability) -> Result<WriteTxn<'_>> {
        if local != crate::Durability::Full {
            self.require_file_backend()?;
        }
        self.require_writable()?;
        self.check_external_head()?;
        // Optional scheduler state is process-local. Reject inherited/changed
        // namespace authority before touching either writer or progress mutex.
        if self.maintenance_enabled.load(Ordering::Acquire)
            && !self.poisoned.load(Ordering::Acquire)
        {
            self.check_file_namespace()?;
        }
        let guard = if self.is_externally_synchronized() {
            self.writer.try_lock().map_err(|_| Error::Busy)?
        } else {
            self.writer.lock().map_err(|_| Error::Busy)?
        };
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.maintenance_before_write();
        let base = self.current.load_full();
        let arena = Arena::new(base.segments.clone(), base.manifest.next_segment)?;
        Ok(WriteTxn {
            env: self,
            selection: crate::previous::FileSelectionPlan::Ordinary,
            local_durability: local,
            value_limit: base.manifest.value_limit,
            catalog_capacity: crate::catalog_profile::read_file(
                &base.segments,
                base.manifest.catalog,
            )?,
            _guard: guard,
            catalog: base.catalog.clone(),
            detached_roots: BTreeMap::new(),
            database_views: None,
            next_view_id: 1,
            next_retained_id: 1,
            view_creator_pid: std::process::id(),
            created_database_ids: BTreeSet::new(),
            next_database: base.manifest.next_database,
            base,
            arena,
            changed: BTreeSet::new(),
            failed: false,
        })
    }
    pub(crate) fn reserve_database_id(&self) -> Result<u64> {
        self.database_ids
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Error::LimitExceeded)
    }
    /// Called while holding the single writer. Register the old root before the
    /// atomic swap, so a racing reader either pins a registered old root or the
    /// new current one. Reclamation also holds the writer for its entire pass.
    pub(crate) fn activate(&self, snapshot: Arc<Snapshot>) -> Result<()> {
        self.activate_inner(snapshot, None, false, true)
    }
    pub(crate) fn activate_prepared(
        &self,
        snapshot: crate::physical::PreparedSnapshot,
    ) -> Result<()> {
        let (snapshot, checkpoint) = snapshot.into_parts();
        self.activate_inner(snapshot, Some(checkpoint), false, true)
    }
    // Only ordinary WriteTxn finalization mints forward/no-resurrection coverage.
    // Restore/import/rollback/previous sessions keep the generic invalidating path.
    pub(crate) fn activate_prepared_business(
        &self,
        snapshot: crate::physical::PreparedSnapshot,
        rebuilt: bool,
    ) -> Result<()> {
        let (snapshot, checkpoint) = snapshot.into_parts();
        self.activate_inner(snapshot, Some(checkpoint), true, rebuilt)
    }
    fn activate_inner(
        &self,
        snapshot: Arc<Snapshot>,
        prepared_checkpoint: Option<[u8; 64]>,
        forward: bool,
        retired_inventory: bool,
    ) -> Result<()> {
        if snapshot.segments.read_ahead() != self.read_ahead()
            || !snapshot.admission.as_ref().is_some_and(|ready| {
                ready.is_for(&self.residency) && ready.is_for_table(&snapshot.segments)
            })
        {
            return Err(Error::OutcomeUnknown);
        }
        if let Some(checkpoint) = prepared_checkpoint {
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            if let Some(external) = &snapshot.external {
                external.check(&self.directory, &snapshot.manifest)?;
                if ulmdb_storage::checkpoint_bytes(&snapshot.manifest)? != checkpoint {
                    return Err(Error::OutcomeUnknown);
                }
            } else {
                let pin = snapshot
                    .physical_pin
                    .as_ref()
                    .ok_or(Error::OutcomeUnknown)?;
                if pin.root().checkpoint() != &checkpoint
                    || !pin.matches_directory(&self.directory)?
                {
                    return Err(Error::OutcomeUnknown);
                }
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
            let _ = checkpoint;
        } else {
            snapshot.check_physical_pin(&self.directory)?;
        }
        if snapshot.external.is_some() {
            *self
                .external_selected
                .lock()
                .map_err(|_| Error::OutcomeUnknown)? = None;
        }
        let mut retired = self.retired.lock().map_err(|_| Error::OutcomeUnknown)?;
        let previous = self.current.load_full();
        if snapshot.owner != previous.owner {
            return Err(Error::OutcomeUnknown);
        }
        // Amortize dead weak-control-block cleanup without scanning all live
        // readers on every commit. A strong count of zero can never revive.
        // Prepared activation already cleaned and reserved under the retained
        // writer guard. Keep reader-metadata scans outside final selection.
        if prepared_checkpoint.is_none() && retired.len() >= 64 && retired.len().is_power_of_two() {
            retired.retain(|weak| weak.strong_count() != 0);
        }
        retired.push(Arc::downgrade(&previous));
        drop(retired);
        self.maintenance_activation(&previous, &snapshot, forward, retired_inventory);
        self.current.store(snapshot);
        // Generic maintenance may have selected a different container/fence
        // history. Prepared business commits install their returned state only
        // after this activation succeeds.
        *self
            .file_durability
            .lock()
            .map_err(|_| Error::OutcomeUnknown)? = None;
        Ok(())
    }
}

// Keep writer mutex ownership inside the consuming transaction lifetime.
pub(crate) type WriterGuard<'a> = MutexGuard<'a, ()>;

pub(crate) fn load_snapshot(directory: &Path, manifest: Manifest, owner: u128) -> Result<Snapshot> {
    load_snapshot_with_read_ahead(directory, manifest, owner, crate::ReadAhead::Normal)
}
pub(crate) fn load_snapshot_with_read_ahead(
    directory: &Path,
    manifest: Manifest,
    owner: u128,
    read_ahead: crate::ReadAhead,
) -> Result<Snapshot> {
    load_snapshot_with_transform(directory, manifest, owner, read_ahead, None)
}
pub(crate) fn load_snapshot_with_transform(
    directory: &Path,
    manifest: Manifest,
    owner: u128,
    read_ahead: crate::ReadAhead,
    transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
) -> Result<Snapshot> {
    let manifest = manifest.with_transform_context(transform.clone());
    let segments = SegmentTable::load_with_profile(
        directory,
        &manifest.segments,
        read_ahead,
        transform,
        manifest.value_limit,
    )?;
    snapshot_from_parts(segments, manifest, owner)
}
pub(crate) fn load_snapshot_with_cache(
    directory: &Path,
    manifest: Manifest,
    owner: u128,
    read_ahead: crate::ReadAhead,
    transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    cache: &crate::chunked::CacheOptions,
) -> Result<Snapshot> {
    cache.validate()?;
    let manifest = manifest.with_transform_context(transform.clone());
    let table = SegmentTable::load_chunked(
        directory,
        &manifest.segments,
        cache.cache.clone(),
        cache.pins,
        manifest.page_size,
        manifest.value_limit,
        read_ahead,
        transform,
    )?
    .with_chunk_budget(
        cache.admission.reads,
        cache.admission.nodes,
        cache.admission.metadata_bytes,
    );
    snapshot_from_parts(table, manifest, owner)
}
pub(crate) fn snapshot_from_parts(
    segments: SegmentTable,
    mut manifest: Manifest,
    owner: u128,
) -> Result<Snapshot> {
    let mut segments = segments
        .with_page_size(manifest.page_size)?
        .with_value_limit(manifest.value_limit)?;
    if segments.is_chunked() {
        segments = segments.fresh_read_scope()?;
    }
    manifest.certify_indexed_table(&segments)?;
    let mut catalog = BTreeMap::new();
    let mut ids = BTreeSet::new();
    if segments.is_chunked() {
        let budget = segments
            .chunk_budget()
            .map(
                |(reads, nodes, metadata_bytes)| crate::chunked::AdmissionLimits {
                    reads,
                    nodes,
                    metadata_bytes,
                },
            )
            .unwrap_or_default();
        // The scoped ascending walker parses only an exact leading profile.
        // No generic get may pin a catalog path before full cached admission.
        catalog = crate::chunked::admission::admit(&segments, &manifest, budget)?;
    } else {
        let capacity = crate::catalog_profile::read(&segments, manifest.catalog)?;
        for item in ulmdb_kv::Cursor::new(
            &segments,
            manifest.catalog,
            Bound::Unbounded,
            Bound::Unbounded,
            false,
        )? {
            let (name, bytes) = item?;
            if name.is_empty() {
                crate::CatalogCapacity::decode_marker(bytes)?;
                continue;
            }
            if name.len() > 255 || catalog.len() >= capacity.get() {
                return Err(Error::Corrupt("invalid catalog name/count"));
            }
            let database =
                Database::decode_for_profile(bytes, manifest.page_size, manifest.value_limit)?;
            if database.id >= manifest.next_database || !ids.insert(database.id) {
                return Err(Error::Corrupt("invalid/duplicate database identity"));
            }
            catalog.insert(name.to_vec(), database);
        }
    }
    Ok(Snapshot {
        range_backed: false,
        external: None,
        physical_pin: None,
        #[cfg(feature = "range-lifecycle")]
        range_checkpoint: None,
        owner,
        manifest,
        segments,
        catalog,
        admission: None,
    })
}

/// Load a new physical namespace with the source owner's immutable cache policy.
/// No ordinary whole-object mapping is constructed on the cached path.
pub(crate) fn load_snapshot_like(
    directory: &Path,
    manifest: Manifest,
    owner: u128,
    source: &SegmentTable,
) -> Result<Snapshot> {
    if let Some(cache) = crate::chunked::CacheOptions::from_table(source) {
        load_snapshot_with_cache(
            directory,
            manifest,
            owner,
            source.read_ahead(),
            source.transform().cloned(),
            &cache,
        )
    } else {
        load_snapshot_with_transform(
            directory,
            manifest,
            owner,
            source.read_ahead(),
            source.transform().cloned(),
        )
    }
}

/// Writer-fenced same-directory candidates can retain authenticated old Sources
/// and authenticate only newly sealed files. Their read pin arena starts empty.
pub(crate) fn cached_candidate_segments(
    directory: &Path,
    manifest: &Manifest,
    base: &SegmentTable,
    policy: crate::Durability,
) -> Result<SegmentTable> {
    let mut table = base
        .empty_like()?
        .with_page_size(manifest.page_size)?
        .with_value_limit(manifest.value_limit)?;
    #[cfg(target_os = "linux")]
    {
        table = table.with_fixed_declaration_from(base, policy);
    }
    for entry in &manifest.segments {
        if !table.reuse_from(base, entry)? {
            table.add(directory, entry.clone())?;
        }
    }
    Ok(table)
}
