use crate::{
    Address, Environment, Error, Prefix, Result,
    catalog::{Database, DatabaseKind},
    environment::{Snapshot, WriterGuard},
    read::check_family,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::Ordering},
};
use ulmdb_core::storage::Reference;
use ulmdb_storage::Arena;

pub struct WriteTxn<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) selection: crate::previous::FileSelectionPlan,
    pub(crate) local_durability: crate::Durability,
    pub(crate) value_limit: crate::ValueLimit,
    pub(crate) catalog_capacity: crate::CatalogCapacity,
    pub(crate) _guard: WriterGuard<'a>,
    pub(crate) base: Arc<Snapshot>,
    pub(crate) arena: Arena,
    pub(crate) catalog: BTreeMap<Vec<u8>, Database>,
    pub(crate) detached_roots: BTreeMap<u64, Arc<crate::detached::DetachedRoot>>,
    pub(crate) database_views: Option<crate::database_view::ViewRegistry>,
    pub(crate) next_view_id: u64,
    pub(crate) next_retained_id: u64,
    pub(crate) view_creator_pid: u32,
    pub(crate) created_database_ids: BTreeSet<u64>,
    pub(crate) changed: BTreeSet<Vec<u8>>,
    pub(crate) next_database: u64,
    pub(crate) failed: bool,
}
impl<'env> WriteTxn<'env> {
    pub fn value_limit(&self) -> crate::ValueLimit {
        self.value_limit
    }
    /// Explicitly enlarge this file writer's authenticated contiguous value profile.
    /// Existing smaller profiles keep their original physical envelope.
    pub fn set_value_limit(&mut self, limit: crate::ValueLimit) -> Result<()> {
        self.live()?;
        if !limit.is_supported() {
            return Err(Error::InvalidInput(
                "maximum file profile requires64-bit host",
            ));
        }
        self.env.require_file_backend()?;
        if limit < self.value_limit {
            return Err(Error::InvalidInput("value profile cannot be lowered"));
        }
        if limit != self.value_limit {
            self.value_limit = limit;
            self.arena.set_value_limit(limit);
            self.changed.extend(self.catalog.keys().cloned());
            self.mark_changed(b"");
        }
        Ok(())
    }
    pub(crate) fn writable_value_limit(&self) -> usize {
        self.value_limit.max(crate::ValueLimit::EXTENDED).get()
    }
    pub(crate) fn enable_value_limit(&mut self, length: usize) -> Result<()> {
        if length > self.writable_value_limit() {
            return Err(Error::InvalidInput("file value size limit exceeded"));
        }
        if length > self.value_limit.get() {
            self.env.require_file_backend()?;
            self.value_limit = crate::ValueLimit::EXTENDED;
            self.arena.set_value_limit(self.value_limit);
            self.changed.extend(self.catalog.keys().cloned());
        }
        Ok(())
    }
    pub(crate) fn mark_changed(&mut self, name: &[u8]) {
        if !self.changed.contains(name) {
            self.changed.insert(name.to_vec());
        }
    }

    pub(crate) fn live(&self) -> Result<()> {
        self.base.check_external()?;
        if self.failed {
            Err(Error::InvalidInput("failed write transaction must abort"))
        } else {
            Ok(())
        }
    }
    pub fn create_database(&mut self, name: &[u8], kind: DatabaseKind) -> Result<()> {
        self.create_database_ordered(name, kind, crate::ordering::DatabaseOrdering::default())
    }
    pub fn create_database_ordered(
        &mut self,
        name: &[u8],
        kind: DatabaseKind,
        ordering: crate::ordering::DatabaseOrdering,
    ) -> Result<()> {
        self.live()?;
        ordering.validate(kind)?;
        if matches!(kind, DatabaseKind::Duplicates { width: Some(width) } if width > 65_536) {
            return Err(Error::InvalidInput("fixed duplicate width exceeds limit"));
        }
        if name.is_empty() || name.len() > 255 {
            return Err(Error::InvalidInput("database name must have 1..255 bytes"));
        }
        if self.catalog.contains_key(name) {
            return Err(Error::InvalidInput("database already exists"));
        }
        if self.catalog.len() >= self.catalog_capacity.get() {
            return Err(Error::LimitExceeded);
        }
        let id = self.env.reserve_database_id()?;
        self.next_database = id.checked_add(1).ok_or(Error::LimitExceeded)?;
        self.catalog.insert(
            name.to_vec(),
            Database {
                ordering,
                id,
                kind,
                root: Reference::NULL,
            },
        );
        self.created_database_ids.insert(id);
        self.mark_changed(name);
        Ok(())
    }
    pub fn drop_database(&mut self, name: &[u8]) -> Result<()> {
        self.live()?;
        let removed = self.catalog.remove(name).ok_or(Error::MissingDatabase)?;
        self.created_database_ids.remove(&removed.id);
        self.remove_database_view_identity(removed.id);
        self.mark_changed(name);
        Ok(())
    }
    pub fn put(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<()> {
        self.live()?;
        if key.len() > self.base.manifest.page_size.native_key_limit()
            || value.len() > self.writable_value_limit()
        {
            return Err(Error::InvalidInput("KV size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if db.ordering.keys != crate::ordering::OrderingSpec::Bytewise {
            return self.put_with_ordering(name, key, value, &Default::default());
        }
        let prior_root = db.root;
        self.enable_value_limit(value.len())?;
        let prior_revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::put(&mut self.arena, prior_root, key, value)?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != prior_root || self.arena.revision() != prior_revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(())
    }
    /// Insert or replace a value, returning its transaction-local prior value.
    ///
    /// `Some(Vec::new())` means a present empty value; `None` means absent.
    /// The prior value is owned because the mutation may rewrite private bytes.
    /// Validation and fallible prior-value allocation precede any mutation. A
    /// failed copy leaves the writer usable; a mutator failure requires abort,
    /// exactly as with [`Self::put`]. Use `put` when the prior value is unneeded.
    pub fn put_with_previous(
        &mut self,
        name: &[u8],
        key: &[u8],
        value: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        self.live()?;
        if key.len() > self.base.manifest.page_size.native_key_limit()
            || value.len() > self.writable_value_limit()
        {
            return Err(Error::InvalidInput("KV size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let previous = match self.get(name, key)? {
            Some(bytes) => {
                let mut owned = Vec::new();
                owned
                    .try_reserve_exact(bytes.len())
                    .map_err(|_| Error::LimitExceeded)?;
                owned.extend_from_slice(bytes);
                Some(owned)
            }
            None => None,
        };
        self.put(name, key, value)?;
        Ok(previous)
    }

    /// Remove a key and report whether it existed in this write transaction.
    /// A valid absent key is a successful no-op. This performs no value copy.
    pub fn delete_with_presence(&mut self, name: &[u8], key: &[u8]) -> Result<bool> {
        let existed = self.get(name, key)?.is_some();
        if existed {
            self.delete(name, key)?;
        }
        Ok(existed)
    }

    pub fn delete(&mut self, name: &[u8], key: &[u8]) -> Result<()> {
        self.live()?;
        if key.len() > self.base.manifest.page_size.native_key_limit() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if db.ordering.keys != crate::ordering::OrderingSpec::Bytewise {
            return self.delete_with_ordering(name, key, &Default::default());
        }
        let prior_root = db.root;
        let prior_revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::delete(&mut self.arena, db.root, key)?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != prior_root || self.arena.revision() != prior_revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(())
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.live()?;
        if key.len() > self.base.manifest.page_size.native_key_limit() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if db.ordering.keys == crate::ordering::OrderingSpec::Bytewise {
            return ulmdb_kv::get(&self.arena, db.root, key);
        }
        let order = crate::ordering::BoundComparator::bind(db.ordering.keys, None)?;
        ulmdb_kv::get_with(&self.arena, db.root, key, &order)
    }
    pub fn insert_prefix(&mut self, name: &[u8], prefix: Prefix, value: &[u8]) -> Result<()> {
        self.live()?;
        if value.len() > self.writable_value_limit() {
            return Err(Error::InvalidInput("prefix value limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        check_family(db.kind, prefix.network())?;
        let prior_root = db.root;
        self.enable_value_limit(value.len())?;
        let prior_revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_prefix::insert(&mut self.arena, prior_root, prefix, value)?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != prior_root || self.arena.revision() != prior_revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(())
    }
    pub fn delete_prefix(&mut self, name: &[u8], prefix: Prefix) -> Result<()> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        check_family(db.kind, prefix.network())?;
        let prior_root = db.root;
        let prior_revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_prefix::delete(&mut self.arena, db.root, prefix)?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != prior_root || self.arena.revision() != prior_revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(())
    }
    pub fn longest_match(&self, name: &[u8], address: Address) -> Result<Option<(Prefix, &[u8])>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        check_family(db.kind, address)?;
        ulmdb_prefix::get(&self.arena, db.root, address)
    }
    pub fn dirty_bytes(&self) -> usize {
        self.arena.dirty_bytes()
    }
    pub fn abort(self) {}
    fn commit_range(self, manifest: ulmdb_storage::Manifest) -> Result<u64> {
        // Allocate the complete activation owner before any publication. It is
        // private and unservable until the selected range/admission is moved in.
        let mut snapshot = Arc::new(Snapshot {
            range_backed: true,
            external: None,
            physical_pin: None,
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: None,
            owner: self.base.owner,
            manifest: self.base.manifest.clone_metadata(),
            segments: ulmdb_storage::SegmentTable::default(),
            catalog: self.catalog,
            admission: None,
        });
        let mut store = self
            .env
            .range
            .as_ref()
            .ok_or(Error::Corrupt("missing range writer"))?
            .lock()
            .map_err(|_| Error::Busy)?;
        let prior = self
            .base
            .admission
            .as_ref()
            .map(|ready| ulmdb_storage::range::PriorAdmission::new(&self.base.segments, ready));
        let state = match store.commit_arena_with_admission(
            self.arena,
            manifest,
            &self.base.manifest,
            &self.env.residency,
            prior,
        ) {
            Ok(s) => s,
            Err(e) => {
                if store.is_poisoned() {
                    self.env.poisoned.store(true, Ordering::Release);
                }
                return Err(e);
            }
        };
        let sequence = state.manifest.sequence;
        let inner = Arc::get_mut(&mut snapshot).expect("private range activation owner");
        #[cfg(feature = "range-lifecycle")]
        {
            inner.range_checkpoint = state.checkpoint;
        }
        inner.manifest = state.manifest;
        inner.segments = state.segments;
        inner.admission = Some(state.admission);
        if self.env.activate(snapshot).is_err() {
            self.env.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        Ok(sequence)
    }
    /// Prepare a file transaction without selecting its immutable candidate.
    /// A clean writer returns a guarded no-op owner. Range preparation is not
    /// qualified and is rejected even when the transaction is clean.
    pub fn prepare(self) -> Result<crate::PreparedWriteTxn<'env>> {
        self.prepare_with_intent(crate::CommitIntent::IfChanged)
    }
    pub fn prepare_with_intent(
        self,
        intent: crate::CommitIntent,
    ) -> Result<crate::PreparedWriteTxn<'env>> {
        let environment_policy = self.env.durability();
        self.prepare_with_data_policy(intent, environment_policy)
    }
    /// Supply the compatibility owner's current environment data policy. The
    /// retained local override is applied separately, so changing the owner's
    /// metadata policy after preparation cannot make an old env flag sticky.
    pub fn prepare_with_data_policy(
        self,
        intent: crate::CommitIntent,
        environment_policy: crate::Durability,
    ) -> Result<crate::PreparedWriteTxn<'env>> {
        self.prepare_file_with_data_policy(intent, environment_policy, true)
    }
    /// Prepare ordinary commit while retaining its conservative Legacy
    /// descriptor-error boundary, distinct from explicit prepare.
    pub fn prepare_for_commit(self) -> Result<crate::PreparedWriteTxn<'env>> {
        self.prepare_for_commit_with_intent(crate::CommitIntent::IfChanged)
    }
    pub fn prepare_for_commit_with_intent(
        self,
        intent: crate::CommitIntent,
    ) -> Result<crate::PreparedWriteTxn<'env>> {
        let environment_policy = self.env.durability();
        self.prepare_for_commit_with_data_policy(intent, environment_policy)
    }
    pub fn prepare_for_commit_with_data_policy(
        self,
        intent: crate::CommitIntent,
        environment_policy: crate::Durability,
    ) -> Result<crate::PreparedWriteTxn<'env>> {
        self.prepare_file_with_data_policy(intent, environment_policy, false)
    }
    fn prepare_file_with_data_policy(
        mut self,
        intent: crate::CommitIntent,
        environment_policy: crate::Durability,
        explicit_prepare: bool,
    ) -> Result<crate::PreparedWriteTxn<'env>> {
        self.live()?;
        self.selection.check(self.env)?;
        self.ensure_database_views_selected()?;
        if self.env.is_range_backed() {
            return Err(Error::InvalidInput(if explicit_prepare {
                "range transaction preparation is unsupported"
            } else {
                "Range commit preparation unsupported"
            }));
        }
        let data_policy = environment_policy.weaken(self.local_durability);
        if self.changed.is_empty() && intent == crate::CommitIntent::IfChanged {
            return Ok(crate::PreparedWriteTxn {
                env: self.env,
                state: crate::prepared::PreparedState::Noop(self.base.manifest.sequence),
                local_durability: self.local_durability,
                prepared_data_policy: data_policy,
                maintenance_rebuilt: false,
                maintenance_rewrite: None,
                cleanup: crate::prepared::PreparationCleanup::new(self.env, false),
                _base: self.base,
                _guard: self._guard,
            });
        }
        let manifest = self.candidate_manifest()?;
        self.prepare_file_candidate(manifest, explicit_prepare, data_policy)
    }
    pub fn commit(self) -> Result<u64> {
        self.commit_with_intent(crate::CommitIntent::IfChanged)
    }
    /// ForcePublication is a file-only compatibility intent; ordinary Range
    /// commits retain their existing path and no-op behavior.
    pub fn commit_with_intent(mut self, intent: crate::CommitIntent) -> Result<u64> {
        self.live()?;
        self.selection.check(self.env)?;
        self.ensure_database_views_selected()?;
        if self.env.is_range_backed() && intent == crate::CommitIntent::ForcePublication {
            return Err(Error::InvalidInput(
                "forced range publication is unsupported",
            ));
        }
        if self.changed.is_empty() && intent == crate::CommitIntent::IfChanged {
            return Ok(self.base.manifest.sequence);
        }
        let manifest = self.candidate_manifest()?;
        if self.env.is_range_backed() {
            return self.commit_range(manifest);
        }
        let data_policy = self.durability();
        self.prepare_file_candidate(manifest, false, data_policy)?
            .commit()
    }
    fn candidate_manifest(&mut self) -> Result<ulmdb_storage::Manifest> {
        let mut catalog_root = self.base.manifest.catalog;
        for name in &self.changed {
            if name.is_empty() {
                catalog_root = match self.catalog_capacity.marker() {
                    Some(bytes) => ulmdb_kv::put(&mut self.arena, catalog_root, b"", &bytes)?,
                    None => ulmdb_kv::delete(&mut self.arena, catalog_root, b"")?,
                };
                continue;
            }
            catalog_root = if let Some(db) = self.catalog.get(name) {
                ulmdb_kv::put(
                    &mut self.arena,
                    catalog_root,
                    name,
                    &db.encode_for_profile(self.base.manifest.page_size, self.value_limit),
                )?
            } else {
                ulmdb_kv::delete(&mut self.arena, catalog_root, name)?
            };
        }
        let mut manifest = self.base.manifest.clone_metadata();
        manifest.value_limit = self.value_limit;
        manifest.format_minor = manifest.format_minor.max(2);
        manifest.previous = manifest.generation;
        manifest.parent = manifest.commit;
        manifest.sequence = manifest
            .sequence
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        manifest.generation = ulmdb_storage::new_identity()?;
        manifest.commit = manifest.generation;
        manifest.catalog = catalog_root;
        manifest.next_database = self
            .next_database
            .max(self.env.database_ids.load(Ordering::Relaxed));
        Ok(manifest)
    }
    fn prepare_file_candidate(
        mut self,
        mut manifest: ulmdb_storage::Manifest,
        explicit_prepare: bool,
        data_policy: crate::Durability,
    ) -> Result<crate::PreparedWriteTxn<'env>> {
        let cleanup = crate::prepared::PreparationCleanup::new(self.env, true);
        let data_policy = self.arena.mapped_data_policy(data_policy);
        let (fallback, committed_rewrite) = self.env.maintenance_fallback_state()?;
        let (mut maintenance_rewrite, evacuated) = if fallback { (committed_rewrite, false) }
            else { self.incremental_rewrite(&mut manifest)? };
        let rebuilt = self.rebuild_for_maintenance(&mut manifest, data_policy)?;
        let mut maintenance_rebuilt = rebuilt.is_some() || evacuated;
        let (segments, publication, sealed, catalog) = if evacuated {
            let (segments, entry, sealed) = self.arena
                .seal_with_durability(&self.env.directory, data_policy)?;
            if entry.is_some() {
                manifest.next_segment = manifest.next_segment.checked_add(1).ok_or(Error::LimitExceeded)?;
            }
            manifest.segments = segments.entries().into();
            let publication = if manifest.format_minor >= 3 {
                if manifest.format_minor == 4 { manifest.prepare_indexed(None)?; }
                Some(ulmdb_storage::prepare_publication_with_durability(
                    &self.env.directory, &manifest, data_policy)?)
            } else { None };
            let sealed = if publication.is_some() { None } else { Some(sealed) };
            (segments, publication, sealed, self.catalog)
        } else if let Some((segments, catalog)) = rebuilt {
            let publication = ulmdb_storage::prepare_publication_with_durability(
                &self.env.directory,
                &manifest,
                data_policy,
            )?;
            (segments, Some(publication), None, catalog)
        } else if manifest.format_minor == 4 {
            let (segments, prepared) = self.arena.seal_indexed_with_durability(
                &self.env.directory,
                &mut manifest,
                &self.base.manifest,
                data_policy,
            )?;
            (segments, Some(prepared), None, self.catalog)
        } else if manifest.format_minor == 3 {
            let (segments, prepared) = self.arena.seal_cohort_with_durability(
                &self.env.directory,
                &mut manifest,
                data_policy,
            )?;
            (segments, Some(prepared), None, self.catalog)
        } else {
            let (segments, new_segment, sealed) = self
                .arena
                .seal_with_durability(&self.env.directory, data_policy)?;
            if new_segment.is_some() {
                manifest.next_segment = manifest
                    .next_segment
                    .checked_add(1)
                    .ok_or(Error::LimitExceeded)?;
            }
            manifest.segments = segments.entries().into();
            (segments, None, Some(sealed), self.catalog)
        };
        let segments = if self.base.segments.is_chunked() {
            drop(segments);
            crate::environment::cached_candidate_segments(
                &self.env.directory,
                &manifest,
                &self.base.segments,
                data_policy,
            )?
        } else {
            segments.with_value_limit(manifest.value_limit)?
        };
        let mut snapshot = Snapshot {
            range_backed: false,
            external: self.base.external.clone(),
            physical_pin: None,
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: None,
            owner: self.base.owner,
            manifest,
            segments,
            catalog,
            admission: None,
        };
        // A denied candidate has not changed CURRENT and does not poison the
        // writer. Mapping-owned leases preserve every already-admitted reader.
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        self.env
            .admit_fixed_placement_with_policy(&mut snapshot, data_policy)?;
        self.env.admit_snapshot_from(&mut snapshot, &self.base)?;
        let publication = self.env.prepare_business_file_snapshot(
            &mut snapshot,
            publication,
            sealed,
            explicit_prepare,
        )?;
        let state = self.env.cached_file_durability()?;
        let ordinary = matches!(&self.selection, crate::previous::FileSelectionPlan::Ordinary);
        let mut publication = self.selection.publication(
            &snapshot.manifest,
            publication,
            state.as_ref(),
            snapshot.external.as_deref(),
        )?;
        let mut snapshot = snapshot.prepare_activation(self.env, publication.prepared())?;
        if fallback && ordinary {
            match self.env.prepare_maintenance_alternative(&snapshot, &self.base, data_policy) {
                Ok(Some(alternative)) => {
                    maintenance_rewrite = alternative.progress;
                    maintenance_rebuilt |= alternative.retired;
                    if let Some((next_snapshot, next_publication)) = alternative.candidate {
                        snapshot = next_snapshot;
                        publication = next_publication;
                    }
                }
                Ok(None) => (),
                Err(error) => self.env.note_fallback_error(&error),
            }
        }
        Ok(crate::PreparedWriteTxn {
            env: self.env,
            state: crate::prepared::PreparedState::Publish {
                snapshot,
                publication,
            },
            local_durability: self.local_durability,
            prepared_data_policy: data_policy,
            maintenance_rebuilt,
            maintenance_rewrite,
            cleanup,
            _base: self.base,
            _guard: self._guard,
        })
    }
}
