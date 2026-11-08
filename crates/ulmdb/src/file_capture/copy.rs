//! Client-copy sources cannot be assembled from independently acquired readers.
use super::*;
use ulmdb_storage::coordination::PhysicalCopyPair;

fn nonreplica(directory: &Path) -> Result<()> {
    match std::fs::symlink_metadata(directory.join("REPLICA")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => Err(Error::ReadOnly),
    }
}
/// Actual canonical D and eligible P, both pinned under one History capture.
/// Full graph and residency admission are deliberately deferred.
pub struct OwnedFileCopyPairCapture {
    directory: PathBuf,
    pair: PhysicalCopyPair,
    capture: CaptureLease,
}
/// Full-audited immutable copy source. Readers are borrowed, never projected.
pub struct FileCopyPair {
    directory: PathBuf,
    creator: u32,
    selected: ReadTxn,
    predecessor: Option<ReadTxn>,
}
/// Full-audited actual chosen view, before a facade's virtual MAIN projection.
pub struct FileCopyView {
    directory: PathBuf,
    creator: u32,
    selected_checkpoint: [u8; 64],
    view_checkpoint: [u8; 64],
    read: ReadTxn,
}
fn check_read(directory: &Path, creator: u32, read: &ReadTxn) -> Result<()> {
    if creator != std::process::id() {
        return Err(Error::InvalidInput("inherited file copy source"));
    }
    if let Some(external) = &read.snapshot.external {
        external.check(directory, &read.snapshot.manifest)?;
        return nonreplica(directory);
    }
    let pin = read
        .snapshot
        .physical_pin
        .as_ref()
        .ok_or(Error::InvalidInput("copy requires physical pin"))?;
    if read.snapshot.range_backed || !pin.matches_environment(directory, &read.snapshot.manifest)? {
        return Err(Error::InvalidInput("file copy source namespace"));
    }
    nonreplica(directory)
}
impl OwnedFileCapture {
    /// Copy-only checked pointer/descriptor opens; ordinary readers are unchanged.
    pub fn current_for_copy(
        directory: impl AsRef<Path>,
        capture: DirectoryCapture,
    ) -> Result<Self> {
        // Reject an inherited capture before even resolving the caller path.
        capture.directory()?;
        let (directory, identity) = namespace(directory.as_ref(), &capture)?;
        nonreplica(&directory)?;
        let (_, capture, pin) = capture.capture_current_for_copy()?;
        if !pin.matches_directory(&directory)? {
            return Err(Error::InvalidInput("copy capture namespace"));
        }
        let selected = pin.root().manifest().clone();
        let selected_checkpoint = *pin.root().checkpoint();
        Ok(Self {
            directory,
            identity,
            selected,
            selected_checkpoint,
            selection: None,
            pin,
            _capture: capture,
        })
    }
    /// Capture D/P from this same namespace; never A's arbitrary predecessor.
    pub fn into_copy_pair(self) -> Result<OwnedFileCopyPairCapture> {
        self.check()?;
        nonreplica(&self.directory)?;
        let Self {
            directory,
            selected_checkpoint,
            pin,
            _capture,
            ..
        } = self;
        let selected = if pin.root().checkpoint() == &selected_checkpoint {
            pin
        } else {
            // Capture still excludes reclamation; the replaced chosen A pin
            // must not consume a third quota record while building D/P.
            drop(pin);
            _capture.pin_checkpoint(&selected_checkpoint)?
        };
        let pair = _capture.capture_copy_pair_for(selected)?;
        Ok(OwnedFileCopyPairCapture {
            directory,
            pair,
            capture: _capture,
        })
    }
    /// Caller has released its short visibility lease before this full audit.
    pub fn admit_copy_view(self, policy: ResidencyPolicy) -> Result<FileCopyView> {
        self.admit_copy_view_with_read_ahead(policy, crate::ReadAhead::Normal)
    }
    pub fn admit_copy_view_with_read_ahead(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<FileCopyView> {
        self.admit_copy_view_with_transform(policy, read_ahead, None)
    }
    pub fn admit_copy_view_with_transform(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<FileCopyView> {
        self.admit_copy_view_with_cache_options(policy, read_ahead, transform, None)
    }
    pub fn admit_copy_view_with_cache_options(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<FileCopyView> {
        read_ahead.validate()?;
        self.check()?;
        nonreplica(&self.directory)?;
        if !(2..=4).contains(&self.pin.root().manifest().format_minor) {
            return Err(Error::InvalidInput("unsupported copy view format"));
        }
        let directory = self.directory.clone();
        let selected_checkpoint = self.selected_checkpoint;
        crate::backup::preflight_copy_assets(&directory, self.pin.root().manifest())?;
        let (read, _) = self.admit_full_with_cache_options(policy, read_ahead, transform, cache)?;
        Ok(FileCopyView {
            directory,
            creator: std::process::id(),
            selected_checkpoint,
            view_checkpoint: read.file_checkpoint()?,
            read,
        })
    }
}
impl OwnedFileCopyPairCapture {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn selected_manifest(&self) -> &Manifest {
        self.pair.selected().root().manifest()
    }
    pub fn selected_checkpoint(&self) -> &[u8; 64] {
        self.pair.selected().root().checkpoint()
    }
    pub fn predecessor_manifest(&self) -> Option<&Manifest> {
        self.pair.predecessor().map(|pin| pin.root().manifest())
    }
    pub fn predecessor_checkpoint(&self) -> Option<&[u8; 64]> {
        self.pair.predecessor().map(|pin| pin.root().checkpoint())
    }
    pub fn admit(self, policy: ResidencyPolicy) -> Result<FileCopyPair> {
        self.admit_with_read_ahead(policy, crate::ReadAhead::Normal)
    }
    pub fn admit_with_read_ahead(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<FileCopyPair> {
        self.admit_with_transform(policy, read_ahead, None)
    }
    pub fn admit_with_transform(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<FileCopyPair> {
        self.admit_with_cache_options(policy, read_ahead, transform, None)
    }
    pub fn admit_with_cache_options(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<FileCopyPair> {
        read_ahead.validate()?;
        if !self.pair.selected().matches_directory(&self.directory)? {
            return Err(Error::InvalidInput("copy pair namespace"));
        }
        nonreplica(&self.directory)?;
        let Self {
            directory,
            pair,
            capture,
        } = self;
        let (selected, predecessor) = pair.into_pins();
        crate::backup::preflight_copy_assets(&directory, selected.root().manifest())?;
        if let Some(pin) = &predecessor {
            crate::backup::preflight_copy_assets(&directory, pin.root().manifest())?;
        }
        let selected = ReadTxn::from_physical_pin_with_cache_options(
            &directory,
            policy,
            selected,
            read_ahead,
            transform.clone(),
            cache.clone(),
        )?;
        let predecessor = predecessor
            .map(|pin| {
                ReadTxn::from_physical_pin_with_cache_options(
                    &directory,
                    policy,
                    pin,
                    read_ahead,
                    transform.clone(),
                    cache.clone(),
                )
            })
            .transpose()?;
        drop(capture);
        Ok(FileCopyPair {
            directory,
            creator: std::process::id(),
            selected,
            predecessor,
        })
    }
    fn admit_chosen_view(
        self,
        previous: bool,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<FileCopyView> {
        self.admit_chosen_view_with_cache_options(previous, policy, read_ahead, transform, None)
    }
    fn admit_chosen_view_with_cache_options(
        self,
        previous: bool,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<FileCopyView> {
        read_ahead.validate()?;
        if !self.pair.selected().matches_directory(&self.directory)? {
            return Err(Error::InvalidInput("copy view namespace"));
        }
        nonreplica(&self.directory)?;
        let selected_checkpoint = *self.selected_checkpoint();
        let Self {
            directory,
            pair,
            capture,
        } = self;
        let (selected, predecessor) = pair.into_pins();
        let pin = if previous {
            if let Some(previous) = predecessor {
                previous
            } else {
                let m = selected.root().manifest();
                if m.sequence != 0
                    || m.previous != 0
                    || !m.catalog.is_null()
                    || m.parent != 0
                    || m.next_database != 1
                    || m.next_segment != 1
                    || !m.segments.is_empty()
                {
                    return Err(Error::InvalidInput("copy view has no predecessor"));
                }
                selected
            }
        } else {
            selected
        };
        crate::backup::preflight_copy_assets(&directory, pin.root().manifest())?;
        let read = ReadTxn::from_physical_pin_with_cache_options(
            &directory,
            policy,
            pin,
            read_ahead,
            transform.clone(),
            cache.clone(),
        )?;
        drop(capture);
        Ok(FileCopyView {
            directory,
            creator: std::process::id(),
            selected_checkpoint,
            view_checkpoint: read.file_checkpoint()?,
            read,
        })
    }
    /// Admit only chosen D; an unchosen predecessor graph is not a prerequisite.
    pub fn admit_selected_view_with_transform(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<FileCopyView> {
        self.admit_selected_view_with_cache_options(policy, read_ahead, transform, None)
    }
    pub fn admit_selected_view_with_cache_options(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<FileCopyView> {
        self.admit_chosen_view_with_cache_options(false, policy, read_ahead, transform, cache)
    }
    pub fn admit_predecessor_view_with_transform(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<FileCopyView> {
        self.admit_predecessor_view_with_cache_options(policy, read_ahead, transform, None)
    }
    pub fn admit_predecessor_view_with_cache_options(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<FileCopyView> {
        self.admit_chosen_view_with_cache_options(true, policy, read_ahead, transform, cache)
    }
    pub fn admit_selected_view(self, policy: ResidencyPolicy) -> Result<FileCopyView> {
        self.admit_selected_view_with_read_ahead(policy, crate::ReadAhead::Normal)
    }
    pub fn admit_selected_view_with_read_ahead(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<FileCopyView> {
        self.admit_chosen_view(false, policy, read_ahead, None)
    }
    /// Admit actual P (or genuine same-genesis fallback), never P.previous.
    pub fn admit_predecessor_view(self, policy: ResidencyPolicy) -> Result<FileCopyView> {
        self.admit_predecessor_view_with_read_ahead(policy, crate::ReadAhead::Normal)
    }
    pub fn admit_predecessor_view_with_read_ahead(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<FileCopyView> {
        self.admit_chosen_view(true, policy, read_ahead, None)
    }
}
impl FileCopyPair {
    pub fn selected(&self) -> &ReadTxn {
        &self.selected
    }
    pub fn predecessor(&self) -> Option<&ReadTxn> {
        self.predecessor.as_ref()
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub(crate) fn check(&self) -> Result<()> {
        check_read(&self.directory, self.creator, &self.selected)?;
        if let Some(read) = &self.predecessor {
            check_read(&self.directory, self.creator, read)?;
        }
        Ok(())
    }
}
impl FileCopyView {
    pub fn read(&self) -> &ReadTxn {
        &self.read
    }
    pub fn selected_checkpoint(&self) -> &[u8; 64] {
        &self.selected_checkpoint
    }
    pub fn view_checkpoint(&self) -> &[u8; 64] {
        &self.view_checkpoint
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub(crate) fn check(&self) -> Result<()> {
        check_read(&self.directory, self.creator, &self.read)
    }
}

/// Genuine caller-managed D/P metadata capture; admission is separately consumed.
pub struct ExternalFileCopyPairCapture {
    directory: PathBuf,
    authority: crate::ExternalSynchronization,
    roots: ulmdb_storage::external::ExternalCopyRoots,
}
impl crate::ExternalSynchronization {
    pub fn capture_copy_pair(
        &self,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<ExternalFileCopyPairCapture> {
        let directory = self.storage.directory()?;
        nonreplica(&directory)?;
        let roots = self.storage.capture_copy_roots(transform)?;
        Ok(ExternalFileCopyPairCapture {
            directory,
            authority: self.clone(),
            roots,
        })
    }
    pub(crate) fn admit_manifest(
        &self,
        directory: &Path,
        manifest: &Manifest,
        policy: ResidencyPolicy,
        advice: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<ReadTxn> {
        self.admit_manifest_with_cache_options(directory, manifest, policy, advice, transform, None)
    }
    pub(crate) fn admit_manifest_with_cache_options(
        &self,
        directory: &Path,
        manifest: &Manifest,
        policy: ResidencyPolicy,
        advice: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<ReadTxn> {
        self.check(directory, manifest)?;
        crate::backup::preflight_copy_assets(directory, manifest)?;
        let controller = ulmdb_storage::ResidencyController::new(policy)?;
        if let Some(cache) = &cache {
            cache.validate()?;
            if policy != ResidencyPolicy::Demand {
                return Err(Error::InvalidInput("chunk cache requires Demand"));
            }
        }
        let owner = crate::client_snapshot::snapshot_owner()?;
        let mut snapshot = match &cache {
            Some(cache) => crate::environment::load_snapshot_with_cache(
                directory,
                manifest.clone(),
                owner,
                advice,
                transform,
                cache,
            )?,
            None => crate::environment::load_snapshot_with_transform(
                directory,
                manifest.clone(),
                owner,
                advice,
                transform,
            )?,
        };
        if cache.is_none() {
            crate::backup::validate_for_admission(&snapshot.segments, &snapshot.manifest)?;
        }
        snapshot.external = Some(std::sync::Arc::new(self.clone()));
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        self.check(directory, &snapshot.manifest)?;
        Ok(ReadTxn {
            snapshot: std::sync::Arc::new(snapshot),
        })
    }
}
impl ExternalFileCopyPairCapture {
    pub fn selected_manifest(&self) -> &Manifest {
        self.roots.selected()
    }
    pub fn selected_checkpoint(&self) -> &[u8; 64] {
        self.roots.checkpoint()
    }
    pub fn predecessor_manifest(&self) -> Option<&Manifest> {
        self.roots.previous()
    }
    pub fn predecessor_checkpoint(&self) -> Result<Option<[u8; 64]>> {
        self.roots
            .previous()
            .map(ulmdb_storage::checkpoint_bytes)
            .transpose()
    }
    pub fn admit(
        self,
        policy: ResidencyPolicy,
        advice: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<FileCopyPair> {
        self.admit_with_cache_options(policy, advice, transform, None)
    }
    pub fn admit_with_cache_options(
        self,
        policy: ResidencyPolicy,
        advice: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<FileCopyPair> {
        self.roots.check()?;
        let selected = self.authority.admit_manifest_with_cache_options(
            &self.directory,
            self.roots.selected(),
            policy,
            advice,
            transform.clone(),
            cache.clone(),
        )?;
        let predecessor = self
            .roots
            .previous()
            .map(|m| {
                self.authority.admit_manifest_with_cache_options(
                    &self.directory,
                    m,
                    policy,
                    advice,
                    transform.clone(),
                    cache.clone(),
                )
            })
            .transpose()?;
        Ok(FileCopyPair {
            directory: self.directory,
            creator: std::process::id(),
            selected,
            predecessor,
        })
    }
    pub fn admit_view(
        self,
        previous: bool,
        policy: ResidencyPolicy,
        advice: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<FileCopyView> {
        self.admit_view_with_cache_options(previous, policy, advice, transform, None)
    }
    pub fn admit_view_with_cache_options(
        self,
        previous: bool,
        policy: ResidencyPolicy,
        advice: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<FileCopyView> {
        self.roots.check()?;
        let selected_checkpoint = *self.roots.checkpoint();
        let manifest = if previous {
            match self.roots.previous() {
                Some(p) => p,
                None => {
                    let m = self.roots.selected();
                    if m.sequence != 0
                        || m.previous != 0
                        || !m.catalog.is_null()
                        || m.parent != 0
                        || m.next_database != 1
                        || m.next_segment != 1
                        || !m.segments.is_empty()
                    {
                        return Err(Error::InvalidInput("external copy has no predecessor"));
                    }
                    m
                }
            }
        } else {
            self.roots.selected()
        };
        let read = self.authority.admit_manifest_with_cache_options(
            &self.directory,
            manifest,
            policy,
            advice,
            transform,
            cache,
        )?;
        let view_checkpoint = read.file_checkpoint()?;
        Ok(FileCopyView {
            directory: self.directory,
            creator: std::process::id(),
            selected_checkpoint,
            view_checkpoint,
            read,
        })
    }
}
