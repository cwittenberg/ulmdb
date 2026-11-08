//! Explicit native previous-root ownership; canonical publication has no C view offset.
use crate::{Environment, Error, ReadTxn, ResidencyPolicy, Result, WriteTxn};
use std::{
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug)]
pub enum PreviousError {
    NoPrevious,
    Engine(Error),
}
impl From<Error> for PreviousError {
    fn from(error: Error) -> Self {
        Self::Engine(error)
    }
}
impl fmt::Display for PreviousError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPrevious => write!(f, "no retained previous snapshot"),
            Self::Engine(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for PreviousError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoPrevious => None,
            Self::Engine(e) => Some(e),
        }
    }
}
pub(crate) struct PreviousPlan {
    pub selection: ulmdb_storage::PreviousSelection,
    pub pending: Arc<AtomicBool>,
}
pub(crate) enum FileSelectionPlan {
    Ordinary,
    Previous(Arc<PreviousPlan>),
}
impl FileSelectionPlan {
    pub(crate) fn check(&self, env: &Environment) -> Result<()> {
        if let Self::Previous(plan) = self {
            if plan.selection.check_current().is_err() {
                env.poisoned.store(true, Ordering::Release);
                return Err(Error::OutcomeUnknown);
            }
        }
        Ok(())
    }
    pub(crate) fn publication(
        self,
        manifest: &ulmdb_storage::Manifest,
        publication: ulmdb_storage::PreparedPublication,
        state: Option<&ulmdb_storage::FileDurabilityState>,
        external: Option<&crate::ExternalSynchronization>,
    ) -> Result<crate::prepared::FilePublication> {
        if let Some(external) = external {
            return Ok(match self {
                Self::Ordinary => crate::prepared::FilePublication::External(
                    publication.prepare_ready_externally_synchronized(state, &external.storage)?,
                ),
                Self::Previous(plan) => crate::prepared::FilePublication::ExternalPrevious {
                    publication: ulmdb_storage::prepare_previous_publication(
                        &plan.selection,
                        manifest,
                        publication,
                    )?
                    .prepare_ready_externally_synchronized(
                        Some(plan.selection.selected_durability()),
                        &external.storage,
                    )?,
                    pending: plan.pending.clone(),
                },
            });
        }
        Ok(match self {
            Self::Ordinary => {
                crate::prepared::FilePublication::Ordinary(publication.prepare_ready(state)?)
            }
            Self::Previous(plan) => crate::prepared::FilePublication::Previous {
                publication: ulmdb_storage::prepare_previous_publication(
                    &plan.selection,
                    manifest,
                    publication,
                )?
                .prepare_ready(Some(plan.selection.selected_durability()))?,
                pending: plan.pending.clone(),
            },
        })
    }
}
/// An exclusive file owner serving authenticated previous A while durable B
/// remains selected. Ordinary maintenance and the inner Environment are hidden
/// until the first publishing commit is selected and activated. Existing read
/// snapshots keep their own physical pins after this session is dropped.
///
/// Same-format Legacy/Cohort/Indexed pairs use Linux, macOS or Windows physical leases. Selected
/// B's descriptor must authenticate; only A's complete logical graph is loaded.
/// Native reads after publication see C. LMDB's shared view offset is not modeled.
pub struct PreviousSession {
    environment: Environment,
    plan: Arc<PreviousPlan>,
}
impl PreviousSession {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn bind_fixed_map_layout(&self, fixed: Arc<crate::FixedMap>) -> Result<()> {
        self.environment.bind_fixed_map_layout(fixed)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn track_fixed_map_declaration(
        &self,
        declaration: Arc<crate::FixedMapDeclaration>,
    ) -> Result<()> {
        self.environment.track_fixed_map_declaration(declaration)
    }

    pub fn open(path: impl AsRef<Path>) -> std::result::Result<Self, PreviousError> {
        Self::open_with_residency(path, ResidencyPolicy::Demand)
    }
    pub fn open_with_residency(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
    ) -> std::result::Result<Self, PreviousError> {
        Self::open_with_residency_and_read_ahead(path, policy, crate::ReadAhead::Normal)
    }
    pub fn open_with_residency_and_read_ahead(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> std::result::Result<Self, PreviousError> {
        Self::open_with_transform_options(path, policy, read_ahead, None)
    }
    pub fn open_with_transform_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> std::result::Result<Self, PreviousError> {
        Self::open_with_cache_options(path, policy, read_ahead, transform, None)
    }
    pub fn open_with_cache_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> std::result::Result<Self, PreviousError> {
        if let Some(cache) = &cache {
            cache.validate()?;
            if policy != ResidencyPolicy::Demand {
                return Err(Error::InvalidInput("chunk cache requires Demand").into());
            }
        }
        read_ahead.validate()?;
        if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(
                Error::InvalidInput("previous sessions require Linux, macOS or Windows").into(),
            );
        }
        let directory = path.as_ref().canonicalize().map_err(Error::from)?;
        let lock = Environment::lock(&directory)?;
        let configured =
            ulmdb_storage::transform::check_configuration(&directory, transform.as_ref())?;
        if ulmdb_storage::range::is_selected(&directory)? {
            return Err(Error::InvalidInput("range previous sessions are unsupported").into());
        }
        if directory
            .join("REPLICA")
            .try_exists()
            .map_err(Error::from)?
        {
            return Err(Error::ReadOnly.into());
        }
        let unbound = ulmdb_storage::coordination::Coordination::capture_directory_with_transform(
            &directory,
            Default::default(),
            transform.clone(),
        )?;
        let (_, capture, selection, pin) = unbound
            .capture_previous()?
            .ok_or(PreviousError::NoPrevious)?;
        if configured.is_some_and(|uuid| uuid != pin.root().manifest().uuid) {
            return Err(Error::CryptoMismatch.into());
        }
        let mut snapshot = match &cache {
            Some(cache) => crate::environment::load_snapshot_with_cache(
                &directory,
                pin.root().manifest().clone(),
                0,
                read_ahead,
                transform,
                cache,
            )?,
            None => crate::environment::load_snapshot_with_transform(
                &directory,
                pin.root().manifest().clone(),
                0,
                read_ahead,
                transform,
            )?,
        };
        if cache.is_none() {
            crate::backup::validate_for_admission(&snapshot.segments, &snapshot.manifest)?;
        }
        let controller = ulmdb_storage::ResidencyController::new(policy)?;
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        snapshot.physical_pin = Some(Arc::new(pin));
        let environment =
            Environment::from_verified_with_controller(&directory, snapshot, lock, controller)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        environment.set_maintenance_policy(crate::MaintenancePolicy::Disabled)?;
        environment.remember_pending_file_durability(selection.selected_durability().clone())?;
        environment.raise_database_id_floor(
            selection
                .selected()
                .next_database
                .max(selection.previous().next_database),
        )?;
        drop(capture);
        Ok(Self {
            environment,
            plan: Arc::new(PreviousPlan {
                selection,
                pending: Arc::new(AtomicBool::new(true)),
            }),
        })
    }
    pub fn open_externally_synchronized(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> std::result::Result<Self, PreviousError> {
        Self::open_externally_synchronized_with_cache_options(
            path, policy, read_ahead, transform, None,
        )
    }
    pub fn open_externally_synchronized_with_cache_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> std::result::Result<Self, PreviousError> {
        let directory = path.as_ref().canonicalize().map_err(Error::from)?;
        if directory
            .join("REPLICA")
            .try_exists()
            .map_err(Error::from)?
        {
            return Err(Error::ReadOnly.into());
        }
        let authority =
            crate::ExternalSynchronization::open_namespace(&directory, transform.clone())?;
        let selection = authority
            .storage
            .capture_previous_selection(transform.clone())?
            .ok_or(PreviousError::NoPrevious)?;
        if let Some(cache) = &cache {
            cache.validate()?;
            if policy != ResidencyPolicy::Demand {
                return Err(Error::InvalidInput("chunk cache requires Demand").into());
            }
        }
        let mut snapshot = match &cache {
            Some(cache) => crate::environment::load_snapshot_with_cache(
                &directory,
                selection.previous().clone(),
                0,
                read_ahead,
                transform,
                cache,
            )?,
            None => crate::environment::load_snapshot_with_transform(
                &directory,
                selection.previous().clone(),
                0,
                read_ahead,
                transform,
            )?,
        };
        if cache.is_none() {
            crate::backup::validate_for_admission(&snapshot.segments, &snapshot.manifest)?;
        }
        let controller = ulmdb_storage::ResidencyController::new(policy)?;
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        snapshot.external = Some(Arc::new(authority));
        let environment = Environment::from_verified_inner(&directory, snapshot, None, controller)?;
        *environment
            .external_selected
            .lock()
            .map_err(|_| Error::Busy)? = Some(*selection.selected_checkpoint());
        environment.remember_pending_file_durability(selection.selected_durability().clone())?;
        environment.raise_database_id_floor(
            selection
                .selected()
                .next_database
                .max(selection.previous().next_database),
        )?;
        Ok(Self {
            environment,
            plan: Arc::new(PreviousPlan {
                selection,
                pending: Arc::new(AtomicBool::new(true)),
            }),
        })
    }
    /// Exact durable B while pending, or activated C after successful publication.
    /// No IO rediscovery and no writer mutex; uncertainty is an explicit error.
    pub fn selected_checkpoint(&self) -> Result<[u8; 64]> {
        if self.environment.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        self.environment.check_file_namespace()?;
        if self.is_previous() {
            Ok(*self.plan.selection.selected_checkpoint())
        } else {
            self.environment.file_checkpoint()
        }
    }
    pub fn file_checkpoint(&self) -> Result<[u8; 64]> {
        self.environment.file_checkpoint()
    }
    pub fn database_id_floor(&self) -> u64 {
        self.environment.database_id_floor()
    }
    pub fn raise_database_id_floor(&self, floor: u64) -> Result<()> {
        self.environment.check_file_namespace()?;
        self.environment.raise_database_id_floor(floor)
    }
    pub fn begin_read(&self) -> ReadTxn {
        self.environment.begin_read()
    }
    pub fn durability(&self) -> crate::Durability {
        self.environment.durability()
    }
    pub fn set_durability(&self, policy: crate::Durability) -> Result<()> {
        self.environment.set_durability(policy)
    }
    pub fn sync(&self) -> Result<ulmdb_storage::FileSyncReport> {
        self.environment.sync()
    }
    pub fn sync_with_force(&self, force: bool) -> Result<Option<ulmdb_storage::FileSyncReport>> {
        self.environment.sync_with_force(force)
    }
    pub fn begin_write(&self) -> Result<WriteTxn<'_>> {
        self.begin_write_with_durability(crate::Durability::Full)
    }
    pub fn begin_write_with_durability(&self, local: crate::Durability) -> Result<WriteTxn<'_>> {
        let mut writer = self.environment.begin_write_with_durability(local)?;
        // Inspect mode while holding the writer mutex: an earlier writer may
        // have published while this begin waited for that same mutex.
        if self.is_previous() {
            writer.value_limit = writer
                .value_limit
                .max(self.plan.selection.selected().value_limit);
            writer.arena.set_value_limit(writer.value_limit);
            writer.selection = FileSelectionPlan::Previous(self.plan.clone());
            writer.selection.check(&self.environment)?;
        }
        Ok(writer)
    }
    pub fn is_previous(&self) -> bool {
        self.plan.pending.load(Ordering::Acquire)
    }
    /// Conversion becomes available only after the canonical C activation.
    pub fn into_environment(self) -> std::result::Result<Environment, Self> {
        if self.is_previous() {
            Err(self)
        } else {
            Ok(self.environment)
        }
    }
}
impl ReadTxn {
    /// Capture and pin a coherent previous root without acquiring the native
    /// writer-lifetime LOCK. Active clean, dirty and prepared writers may remain
    /// alive; only their short pointer publication excludes pair capture.
    /// Replica, Range and platforms without physical coordination remain unsupported.
    pub fn open_previous_coordinated_snapshot(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
    ) -> std::result::Result<Self, PreviousError> {
        Self::open_previous_coordinated_snapshot_with_read_ahead(
            path,
            policy,
            crate::ReadAhead::Normal,
        )
    }
    pub fn open_previous_coordinated_snapshot_with_read_ahead(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> std::result::Result<Self, PreviousError> {
        Self::open_previous_coordinated_snapshot_with_transform_options(
            path, policy, read_ahead, None,
        )
    }
    pub fn open_previous_coordinated_snapshot_with_transform_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> std::result::Result<Self, PreviousError> {
        Self::open_previous_coordinated_snapshot_with_cache_options(
            path, policy, read_ahead, transform, None,
        )
    }
    pub fn open_previous_coordinated_snapshot_with_cache_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> std::result::Result<Self, PreviousError> {
        read_ahead.validate()?;
        if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(
                Error::InvalidInput("previous captures require Linux, macOS or Windows").into(),
            );
        }
        let directory = path.as_ref().canonicalize().map_err(Error::from)?;
        let capture = ulmdb_storage::coordination::Coordination::capture_directory_with_transform(
            &directory,
            Default::default(),
            transform.clone(),
        )?;
        let owned = crate::OwnedFileCapture::previous(&directory, capture)?;
        Ok(owned
            .admit_with_cache_options(policy, read_ahead, transform, cache)?
            .0)
    }
}
