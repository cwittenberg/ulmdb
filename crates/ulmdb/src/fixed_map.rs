//! Explicit fixed placement. Generic opens/backup audits retain normal mappings;
//! only these constructors or the bound C option promise a persisted address.
use crate::{
    Environment, Error, FixedMap, FixedMapInfo, ReadTxn, ResidencyPolicy, Result, WriteTxn,
    environment::Snapshot,
};
use std::{path::Path, sync::Arc};
fn snapshot_at(old: &Snapshot, directory: &Path, fixed: Arc<FixedMap>) -> Result<Snapshot> {
    if old.segments.is_chunked() {
        return Err(Error::InvalidInput("fixed chunk mapping incompatible"));
    }
    if old.range_backed {
        return Err(Error::InvalidInput("fixed Range placement unsupported"));
    }
    fixed.check_for(directory, old.manifest.uuid, old.manifest.epoch)?;
    if old.external.is_some() && fixed.is_reusable()? {
        return Err(Error::InvalidInput(
            "reusable fixed layout requires coordination",
        ));
    }
    let segments = old
        .segments
        .with_fixed_map(directory, fixed)?
        .with_fixed_external_authority(old.external.as_ref().map(|e| e.storage.clone()));
    let mut manifest = old.manifest.clone();
    manifest.certify_indexed_table(&segments)?;
    Ok(Snapshot {
        range_backed: false,
        physical_pin: old.physical_pin.clone(),
        external: old.external.clone(),
        #[cfg(feature = "range-lifecycle")]
        range_checkpoint: None,
        owner: old.owner,
        manifest,
        segments,
        catalog: old.catalog.clone(),
        admission: None,
    })
}
impl Environment {
    /// Create a genuine empty namespace plus a checked, persisted address window.
    /// The fixed window is bounded; new immutable segments consume disjoint slots.
    /// Exhaustion fails a future writer before selecting any candidate.
    pub fn create_fixed_map(
        path: impl AsRef<Path>,
        mode: crate::PublicationMode,
        capacity: usize,
        requested: Option<usize>,
    ) -> Result<Self> {
        let mut env = Self::create_with_publication(path, mode)?;
        env.enable_fixed_map(capacity, requested)?;
        Ok(env)
    }
    /// Install placement only in an empty sequence0 environment before its first
    /// business commit. No pointers are relocated or invented for existing data.
    pub fn enable_fixed_map(
        &mut self,
        capacity: usize,
        requested: Option<usize>,
    ) -> Result<FixedMapInfo> {
        self.enable_fixed_map_profile(capacity, requested, false)
    }
    /// Fresh opt-in v2 layout with explicit slot reclamation. Existing v1
    /// namespaces remain append-only and are never silently upgraded.
    pub fn create_reusable_fixed_map(
        path: impl AsRef<Path>,
        mode: crate::PublicationMode,
        capacity: usize,
        requested: Option<usize>,
    ) -> Result<Self> {
        let mut env = Self::create_with_publication(path, mode)?;
        env.enable_reusable_fixed_map(capacity, requested)?;
        Ok(env)
    }
    pub fn enable_reusable_fixed_map(
        &mut self,
        capacity: usize,
        requested: Option<usize>,
    ) -> Result<FixedMapInfo> {
        self.enable_fixed_map_profile(capacity, requested, true)
    }
    fn enable_fixed_map_profile(
        &mut self,
        capacity: usize,
        requested: Option<usize>,
        reusable: bool,
    ) -> Result<FixedMapInfo> {
        self.require_file_backend()?;
        self.require_writable()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        self.check_file_namespace()?;
        let old = self.current.load_full();
        if old.segments.is_chunked() {
            return Err(Error::InvalidInput("fixed chunk mapping incompatible"));
        }
        if old.manifest.sequence != 0
            || !old.catalog.is_empty()
            || !old.manifest.segments.is_empty()
        {
            return Err(Error::InvalidInput(
                "fixed map must be selected at creation",
            ));
        }
        if old.external.is_some() && reusable {
            return Err(Error::InvalidInput(
                "reusable fixed layout requires coordination",
            ));
        }
        let create = if reusable {
            FixedMap::create_reusable
        } else {
            FixedMap::create
        };
        let fixed = create(
            &self.directory,
            old.manifest.uuid,
            old.manifest.epoch,
            capacity,
            requested,
        )?;
        let info = fixed.info()?;
        let mut snapshot = snapshot_at(&old, &self.directory, fixed)?;
        self.admit_snapshot(&mut snapshot)?;
        self.current.store(Arc::new(snapshot));
        Ok(info)
    }
    /// Reserve the persisted address before opening any ordinary data mappings,
    /// preventing this constructor's temporary maps from colliding with itself.
    pub fn open_fixed_map(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_fixed_map_with_transform_options(
            path,
            ResidencyPolicy::Demand,
            crate::ReadAhead::Normal,
            None,
        )
    }
    pub fn open_fixed_map_with_transform_options(
        path: impl AsRef<Path>,
        residency: ResidencyPolicy,
        advice: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        let fixed = FixedMap::open_namespace(path.as_ref())?;
        let mut env =
            Self::open_with_transform_options(&path, ResidencyPolicy::Demand, advice, transform)?;
        if residency != ResidencyPolicy::Demand {
            env.residency = ulmdb_storage::ResidencyController::new(residency)?;
        }
        let old = env.current.load_full();
        let mut snapshot = snapshot_at(&old, &env.directory, fixed)?;
        env.admit_snapshot(&mut snapshot)?;
        env.current.store(Arc::new(snapshot));
        Ok(env)
    }
    /// Bind this owner's subsequent operations to an already-declared layout.
    /// Placement changes use new Mapping owners and preserve old reader owners.
    pub fn bind_fixed_map_layout(&self, fixed: Arc<FixedMap>) -> Result<()> {
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        self.check_file_namespace()?;
        let old = self.current.load_full();
        let mut snapshot = snapshot_at(&old, &self.directory, fixed)?;
        self.admit_snapshot(&mut snapshot)?;
        self.current.store(Arc::new(snapshot));
        Ok(())
    }
    pub fn track_fixed_map_declaration(
        &self,
        declaration: Arc<crate::FixedMapDeclaration>,
    ) -> Result<()> {
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        self.check_file_namespace()?;
        let old = self.current.load_full();
        declaration.check_for(&self.directory, old.manifest.uuid, old.manifest.epoch)?;
        declaration.register_manifest_with_policy(
            &old.manifest,
            crate::Durability::Full,
            old.external.as_ref().map(|e| e.storage.clone()),
        )?;
        let snapshot = Snapshot {
            range_backed: old.range_backed,
            physical_pin: old.physical_pin.clone(),
            external: old.external.clone(),
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: old.range_checkpoint.clone(),
            owner: old.owner,
            manifest: old.manifest.clone(),
            segments: old
                .segments
                .clone()
                .with_fixed_declaration(declaration)
                .with_fixed_external_authority(old.external.as_ref().map(|e| e.storage.clone())),
            catalog: old.catalog.clone(),
            admission: old.admission.clone(),
        };
        self.current.store(Arc::new(snapshot));
        Ok(())
    }
    pub fn fixed_map_info(&self) -> Result<Option<FixedMapInfo>> {
        self.file_checkpoint()?;
        self.begin_read().fixed_map_info()
    }
}
impl ReadTxn {
    /// Rebind this exact immutable graph to a genuine namespace-bound layout.
    /// This constructs new Mapping owners; it never mutates another reader's
    /// mappings or substitutes a snapshot/physical database identity.
    pub fn with_fixed_map(self, directory: &Path, fixed: Arc<FixedMap>) -> Result<Self> {
        fixed.check_for(
            directory,
            self.snapshot.manifest.uuid,
            self.snapshot.manifest.epoch,
        )?;
        if self
            .snapshot
            .segments
            .fixed_map()
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &fixed))
        {
            return Ok(self);
        }
        let prior = self.residency_report().policy;
        let controller = ulmdb_storage::ResidencyController::new(prior)?;
        let mut snapshot = snapshot_at(&self.snapshot, directory, fixed)?;
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        Ok(Self {
            snapshot: Arc::new(snapshot),
        })
    }
    pub fn fixed_map_info(&self) -> Result<Option<FixedMapInfo>> {
        self.snapshot
            .segments
            .fixed_map()
            .map(|fixed| fixed.info())
            .transpose()
    }
}
impl WriteTxn<'_> {
    /// Apply explicit placement before alias views/reserves or mutations exist.
    /// The source remains the exact authenticated base, including a PREV base.
    pub fn bind_fixed_map(&mut self, fixed: Arc<FixedMap>) -> Result<()> {
        self.live()?;
        if self.arena.dirty_bytes() != 0
            || !self.changed.is_empty()
            || self.database_views.is_some()
        {
            return Err(Error::InvalidInput("fixed placement must precede mutation"));
        }
        if self
            .base
            .segments
            .fixed_map()
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &fixed))
        {
            return Ok(());
        }
        let mut base = snapshot_at(&self.base, &self.env.directory, fixed)?;
        base.admission = Some(self.env.residency.admit(&base.segments)?);
        let arena = ulmdb_storage::Arena::new(base.segments.clone(), base.manifest.next_segment)?;
        self.base = Arc::new(base);
        self.arena = arena;
        Ok(())
    }
}

impl Environment {
    pub(crate) fn admit_fixed_placement(&self, snapshot: &mut Snapshot) -> Result<()> {
        self.admit_fixed_placement_with_policy(snapshot, crate::Durability::Full)
    }
    pub(crate) fn admit_fixed_placement_with_policy(
        &self,
        snapshot: &mut Snapshot,
        policy: crate::Durability,
    ) -> Result<()> {
        let external = snapshot.external.as_ref().map(|e| e.storage.clone());
        let base = self.current.load();
        let fixed = base.segments.fixed_map();
        let declaration = if fixed.is_some() {
            Some(crate::FixedMapDeclaration::open(
                &self.directory,
                snapshot.manifest.uuid,
                snapshot.manifest.epoch,
            )?)
        } else {
            base.segments.fixed_declaration()
        };
        if let Some(declaration) = declaration {
            declaration.register_manifest_with_policy(
                &snapshot.manifest,
                policy,
                external.clone(),
            )?;
            // Imported pairs can introduce genuine P entries not present in D.
            // Register both actual histories, without inventing absent history.
            if snapshot.manifest.previous != 0 {
                match ulmdb_storage::read_generation_with_transform(
                    &self.directory,
                    snapshot.manifest.previous,
                    snapshot.manifest.format_minor,
                    snapshot.segments.transform().cloned(),
                ) {
                    Ok(previous) => declaration.register_manifest_with_policy(
                        &previous,
                        policy,
                        external.clone(),
                    )?,
                    Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            if let Some(fixed) = fixed {
                if snapshot.segments.fixed_map().is_none() {
                    snapshot.segments = snapshot
                        .segments
                        .clone()
                        .with_fixed_external_authority(external.clone())
                        .with_fixed_map(&self.directory, fixed)?;
                }
            } else {
                snapshot.segments = snapshot
                    .segments
                    .clone()
                    .with_fixed_declaration(declaration)
                    .with_fixed_external_authority(external);
            }
        }
        Ok(())
    }
}

impl Environment {
    /// Checked v2 capacity pressure. Ordinary/v1 layouts report false. C lazy
    /// maintenance admission uses the same predicate as optional preparation.
    pub fn reusable_fixed_map_pressure(&self) -> Result<bool> {
        let base = self.current.load();
        if base.segments.fixed_map().is_none() && base.segments.fixed_declaration().is_none() {
            return Ok(false);
        }
        self.check_file_namespace()?;
        let pressure = if let Some(fixed) = base.segments.fixed_map() {
            fixed.reuse_pressure()?
        } else if let Some(declaration) = base.segments.fixed_declaration() {
            declaration.reuse_pressure()?
        } else {
            None
        };
        Ok(pressure.is_some_and(|p| p.needs_rewrite()))
    }
    /// Reuse only slots outside all durable, named, live and exported roots.
    /// An actual VMA shared lease can still keep an otherwise obsolete slot busy.
    /// This scans bounded inventory metadata; it never relocates mapped bytes.
    #[cfg(target_os = "linux")]
    pub fn reclaim_fixed_map_slots(
        &self,
        limits: ulmdb_storage::fixed_map::FixedSlotReclaimLimits,
    ) -> Result<ulmdb_storage::fixed_map::FixedSlotReclaimReport> {
        let mut plan = self.begin_reclaim_plan(Default::default())?;
        plan.reclaim_fixed_map_slots(limits)
    }
}
