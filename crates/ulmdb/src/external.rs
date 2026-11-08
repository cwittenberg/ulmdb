//! Explicit caller-managed file environments. The caller serializes all access
//! across every process, including capture/publication/reclaim/restore, and must
//! preserve immutable mapped assets until every borrowed reader is finished.
use crate::{Environment, Error, ReadAhead, ReadTxn, ResidencyPolicy, Result};
use std::{path::Path, sync::Arc};
/// An explicitly caller-managed namespace. Cloning grants no exclusion; every
/// clone shares the same process, directory and realm identity. The caller must
/// serialize all captures/publications and retain every mapped immutable file.
#[derive(Clone)]
pub struct ExternalSynchronization {
    pub(crate) storage: ulmdb_storage::external::ExternalSynchronization,
}
impl ExternalSynchronization {
    /// Bind actual selected metadata only. This is no logical graph certificate.
    pub fn open_namespace(
        directory: impl AsRef<Path>,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        let directory = directory.as_ref();
        if ulmdb_storage::range::is_selected(directory)? {
            return Err(Error::InvalidInput("external Range unsupported"));
        }
        let configured =
            ulmdb_storage::transform::check_configuration(directory, transform.as_ref())?;
        let storage = ulmdb_storage::external::ExternalSynchronization::discover(
            directory,
            transform.clone(),
        )?;
        let (manifest, _, _) = storage.capture_current(transform)?;
        if configured.is_some_and(|uuid| uuid != manifest.uuid) {
            return Err(Error::CryptoMismatch);
        }
        Ok(Self { storage })
    }
    pub(crate) fn bind(directory: &Path, manifest: &ulmdb_storage::Manifest) -> Result<Self> {
        Ok(Self {
            storage: ulmdb_storage::external::ExternalSynchronization::caller_managed(
                directory,
                manifest.uuid,
                manifest.epoch,
            )?,
        })
    }
    pub(crate) fn check(&self, directory: &Path, manifest: &ulmdb_storage::Manifest) -> Result<()> {
        self.storage.check(directory, manifest.uuid, manifest.epoch)
    }
    pub fn open_environment(
        &self,
        directory: impl AsRef<Path>,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Environment> {
        self.open_environment_with_cache_options(directory, policy, advice, transform, None)
    }
    pub fn open_environment_with_cache_options(
        &self,
        directory: impl AsRef<Path>,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Environment> {
        let directory = directory.as_ref();
        let (manifest, _, _) = self.storage.capture_current(transform.clone())?;
        self.check(directory, &manifest)?;
        Environment::open_external_bound_with_cache_options(
            directory,
            policy,
            advice,
            transform,
            self.clone(),
            cache,
        )
    }
    pub fn begin_read(
        &self,
        directory: impl AsRef<Path>,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<ReadTxn> {
        self.begin_read_with_cache_options(directory, advice, transform, None)
    }
    pub fn begin_read_with_cache_options(
        &self,
        directory: impl AsRef<Path>,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<ReadTxn> {
        let directory = directory.as_ref();
        let (manifest, _, _) = self.storage.capture_current(transform.clone())?;
        self.check(directory, &manifest)?;
        let mut read =
            ReadTxn::open_snapshot_with_cache_options(directory, advice, transform, cache)?;
        self.check(directory, &read.snapshot.manifest)?;
        Arc::get_mut(&mut read.snapshot)
            .ok_or(Error::Busy)?
            .external = Some(Arc::new(self.clone()));
        Ok(read)
    }
    pub fn flush_async(
        &self,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<ulmdb_storage::FileAsyncFlushReport> {
        self.storage.flush_async(transform)
    }
    pub fn sync(
        &self,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<ulmdb_storage::FileSyncReport> {
        self.storage.sync(transform)
    }
}
impl ReadTxn {
    pub fn open_externally_synchronized(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_externally_synchronized_with_transform_options(path, ReadAhead::Normal, None)
    }
    pub fn open_externally_synchronized_with_transform_options(
        path: impl AsRef<Path>,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::open_externally_synchronized_with_cache_options(path, advice, transform, None)
    }
    pub fn open_externally_synchronized_with_cache_options(
        path: impl AsRef<Path>,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        if ulmdb_storage::range::is_selected(path.as_ref())? {
            return Err(Error::InvalidInput("external Range snapshots unsupported"));
        }
        let mut read = Self::open_snapshot_with_cache_options(&path, advice, transform, cache)?;
        let authority = ExternalSynchronization::bind(path.as_ref(), &read.snapshot.manifest)?;
        Arc::get_mut(&mut read.snapshot)
            .ok_or(Error::Busy)?
            .external = Some(Arc::new(authority));
        Ok(read)
    }
    pub fn external_synchronization(&self) -> Option<ExternalSynchronization> {
        self.snapshot.external.as_deref().cloned()
    }
}
impl crate::environment::Snapshot {
    pub(crate) fn check_external(&self) -> Result<()> {
        if let Some(external) = &self.external {
            external.check(&self.external_directory()?, &self.manifest)?;
        }
        Ok(())
    }
    fn external_directory(&self) -> Result<std::path::PathBuf> {
        self.external
            .as_ref()
            .ok_or(Error::InvalidInput("not external"))?
            .storage
            .directory()
    }
}
impl Environment {
    pub fn open_externally_synchronized(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_externally_synchronized_with_transform_options(
            path,
            ResidencyPolicy::Demand,
            ReadAhead::Normal,
            None,
        )
    }
    pub fn open_externally_synchronized_with_transform_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::open_externally_synchronized_with_cache_options(path, policy, advice, transform, None)
    }
    pub fn open_externally_synchronized_with_cache_options(
        path: impl AsRef<Path>,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        let read = ReadTxn::open_externally_synchronized_with_cache_options(
            &path,
            advice,
            transform.clone(),
            cache.clone(),
        )?;
        let authority = read
            .external_synchronization()
            .ok_or(Error::InvalidInput("missing external authority"))?;
        Self::open_external_bound_with_cache_options(
            path.as_ref(),
            policy,
            advice,
            transform,
            authority,
            cache,
        )
    }
    pub(crate) fn open_external_bound(
        directory: &Path,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        authority: ExternalSynchronization,
    ) -> Result<Self> {
        Self::open_external_bound_with_cache_options(
            directory, policy, advice, transform, authority, None,
        )
    }
    pub(crate) fn open_external_bound_with_cache_options(
        directory: &Path,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        authority: ExternalSynchronization,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        if let Some(cache) = &cache {
            cache.validate()?;
            if policy != ResidencyPolicy::Demand {
                return Err(Error::InvalidInput("chunk cache requires Demand"));
            }
        }
        advice.validate()?;
        if ulmdb_storage::range::is_selected(directory)? {
            return Err(Error::InvalidInput("external Range unsupported"));
        }
        let configured =
            ulmdb_storage::transform::check_configuration(directory, transform.as_ref())?;
        let (manifest, _, durability) = authority.storage.capture_current(transform.clone())?;
        authority.check(directory, &manifest)?;
        if configured.is_some_and(|uuid| uuid != manifest.uuid) {
            return Err(Error::CryptoMismatch);
        }
        let controller = ulmdb_storage::ResidencyController::new(policy)?;
        let mut snapshot = match &cache {
            Some(cache) => crate::environment::load_snapshot_with_cache(
                directory, manifest, 0, advice, transform, cache,
            )?,
            None => crate::environment::load_snapshot_with_transform(
                directory, manifest, 0, advice, transform,
            )?,
        };
        if cache.is_none() {
            crate::backup::validate_for_admission(&snapshot.segments, &snapshot.manifest)?;
        }
        snapshot.external = Some(Arc::new(authority));
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        let env = Self::from_verified_inner(directory, snapshot, None, controller)?;
        env.remember_file_durability(durability)?;
        Ok(env)
    }
    pub fn is_externally_synchronized(&self) -> bool {
        self.current.load().external.is_some()
    }
    pub(crate) fn require_coordinated(&self) -> Result<()> {
        self.current.load().check_external()?;
        if self.is_externally_synchronized() {
            return Err(Error::InvalidInput(
                "operation requires coordinated ownership",
            ));
        }
        Ok(())
    }
    pub(crate) fn check_external_head(&self) -> Result<()> {
        let snapshot = self.current.load();
        if let Some(external) = &snapshot.external {
            external.check(&self.directory, &snapshot.manifest)?;
            if self.poisoned.load(std::sync::atomic::Ordering::Acquire) {
                return Err(Error::OutcomeUnknown);
            }
            let (_, checkpoint, _) = external
                .storage
                .capture_current(snapshot.segments.transform().cloned())?;
            let expected = self
                .external_selected
                .lock()
                .map_err(|_| Error::Busy)?
                .unwrap_or(ulmdb_storage::checkpoint_bytes(&snapshot.manifest)?);
            if checkpoint != expected {
                return Err(Error::Busy);
            }
        }
        Ok(())
    }
}
/// Coherent actual durableB/previousA metadata captured under caller exclusion.
/// Admission loads only chosenA; no logical-graph authority for displacedB.
pub struct ExternalPreviousCapture {
    authority: ExternalSynchronization,
    directory: std::path::PathBuf,
    selection: ulmdb_storage::PreviousSelection,
}
impl ExternalSynchronization {
    pub fn capture_previous(
        &self,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> std::result::Result<ExternalPreviousCapture, crate::PreviousError> {
        let directory = self.storage.directory()?;
        if directory
            .join("REPLICA")
            .try_exists()
            .map_err(Error::from)?
        {
            return Err(Error::ReadOnly.into());
        }
        let selection = self
            .storage
            .capture_previous_selection(transform)?
            .ok_or(crate::PreviousError::NoPrevious)?;
        Ok(ExternalPreviousCapture {
            authority: self.clone(),
            directory,
            selection,
        })
    }
}
impl ExternalPreviousCapture {
    pub fn selected_manifest(&self) -> &ulmdb_storage::Manifest {
        self.selection.selected()
    }
    pub fn selected_checkpoint(&self) -> &[u8; 64] {
        self.selection.selected_checkpoint()
    }
    pub fn view_manifest(&self) -> &ulmdb_storage::Manifest {
        self.selection.previous()
    }
    pub fn view_checkpoint(&self) -> &[u8; 64] {
        self.selection.previous_checkpoint()
    }
    pub fn admit(
        self,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<ReadTxn> {
        self.admit_with_cache_options(policy, advice, transform, None)
    }
    pub fn admit_with_cache_options(
        self,
        policy: ResidencyPolicy,
        advice: ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<ReadTxn> {
        self.authority
            .check(&self.directory, self.selection.selected())?;
        self.selection.check_current()?;
        let read = self.authority.admit_manifest_with_cache_options(
            &self.directory,
            self.selection.previous(),
            policy,
            advice,
            transform,
            cache,
        )?;
        self.selection.check_current()?;
        Ok(read)
    }
}
