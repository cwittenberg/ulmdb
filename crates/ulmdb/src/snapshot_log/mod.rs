//! Opt-in ordered KV backend: immutable in-memory snapshots and a bounded,
//! synchronously persisted transaction log. This is a separate on-disk format.
//!
//! No Legacy/Cohort/Indexed file, admission, pin, or reclamation protocol is
//! changed. Values live in immutable tree pages or in epoch-pinned version
//! slots behind stable ordered keys. Old readers need no log lease. Two log
//! banks checkpoint/reuse independently of those readers.
//! This initial backend supports bytewise ordered KV databases. Prefix types,
//! duplicates, custom comparators, backup/replication and independent process
//! readers are explicitly unavailable. Device-power-loss behavior is not yet
//! qualified. These limits prohibit a production-parity/completion claim.

mod effects;
mod index;
mod lookup;
mod profile;
mod versioned;
use crate::{DatabaseKind, Durability, Error, Result};
use arc_swap::ArcSwap;
use index::{Bytes, Change, Index, MetadataReservation};
pub use profile::Metrics;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};
use ulmdb_storage::snapshot_log::Journal;
use ulmdb_storage::snapshot_versions::{Limits as VersionLimits, PoolWriter, View as VersionView};

const STATE: &[u8; 8] = b"ULMSNAP1";
const EFFECTS: &[u8; 8] = b"ULMTXNS1";
const EFFECTS_V2: &[u8; 8] = b"ULMTXNS2";
const MAX_KEY: usize = 65536;
const MAX_VALUE: usize = 16 << 20;
const STATE_HEADER: usize = 36;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub index_bytes: usize,
    pub journal_capacity: usize,
    pub transaction_bytes: usize,
    pub max_operations: usize,
    pub max_databases: usize,
    /// Large transactions use grouped edits; usize::MAX forces the small path.
    pub batch_threshold: usize,
    /// Keep stable key slots in the ordered tree and version values separately.
    pub versioned_values: bool,
    /// Maintain a bounded exact-key index over the same versioned records.
    pub exact_lookup: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            index_bytes: 64 << 20,
            journal_capacity: 4 << 20,
            transaction_bytes: 2 << 20,
            max_operations: 65536,
            max_databases: 4096,
            batch_threshold: 16,
            versioned_values: false,
            exact_lookup: true,
        }
    }
}
impl Options {
    fn validate(self) -> Result<Self> {
        if self.index_bytes < 4096
            || self.journal_capacity < 8192
            || self.journal_capacity > 1 << 30
            || !self.journal_capacity.is_multiple_of(4096)
            || self.transaction_bytes == 0
            || self.transaction_bytes > self.journal_capacity - 4096
            || self.max_operations == 0
            || self.max_operations > 1_048_576
            || self.max_databases == 0
            || self.max_databases > 65536
            || self.batch_threshold == 0
            || (self.versioned_values && self.index_bytes < 65536)
        {
            return Err(Error::InvalidInput("snapshot-log limits"));
        }
        Ok(self)
    }
    fn tree_bytes(self) -> usize {
        if self.versioned_values {
            self.index_bytes / 2
        } else {
            self.index_bytes
        }
    }
    fn version_limits(self) -> Result<VersionLimits> {
        VersionLimits::for_bytes(self.index_bytes / 2)
    }
}
struct Snapshot {
    index: Index,
    catalog: Arc<Catalog>,
    next_database: u64,
    sequence: u64,
    encoded_bytes: usize,
    _reservation: MetadataReservation,
    values: Option<VersionView>,
    lookup: Option<Arc<lookup::Lookup>>,
}
impl Snapshot {
    fn release_lookup(&self) -> bool {
        if let Some(lookup) = self.lookup.as_ref().filter(|lookup| lookup.is_enabled()) {
            lookup.disable();
            true
        } else {
            false
        }
    }
    #[inline(always)]
    fn value(&self, database: u64, key: &[u8]) -> Result<Option<&[u8]>> {
        match &self.values {
            Some(view) => {
                if let Some(lookup) = &self.lookup {
                    match lookup.get(view, database, key) {
                        lookup::Outcome::Hit(value) => return Ok(Some(value)),
                        lookup::Outcome::Absent => return Ok(None),
                        lookup::Outcome::Fallback => {}
                    }
                }
                versioned::get(&self.index, view, database, key)
            }
            None => Ok(self.index.get(database, key)),
        }
    }
    fn resolve<'a>(&'a self, stored: &'a [u8]) -> Result<&'a [u8]> {
        match &self.values {
            Some(view) => view
                .get(versioned::decode_handle(stored)?)
                .ok_or(Error::Corrupt("snapshot-log stale stable value")),
            None => Ok(stored),
        }
    }
}
// Conservative requested-node envelope for the current BTreeMap<Vec<u8>,u64>,
// plus each key's actual Vec capacity. This is not allocator usable size/RSS.
const CATALOG_ENTRY_BUDGET: usize = 2048;
struct Catalog {
    map: BTreeMap<Vec<u8>, u64>,
    ids: BTreeSet<u64>,
    names_bytes: usize,
    reservation: MetadataReservation,
}
impl std::ops::Deref for Catalog {
    type Target = BTreeMap<Vec<u8>, u64>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}
fn catalog_budget(map: &BTreeMap<Vec<u8>, u64>) -> Result<usize> {
    // Both ordered containers may retain an allocated empty root after remove.
    map.keys().try_fold(
        std::mem::size_of::<Catalog>() + 64 + CATALOG_ENTRY_BUDGET,
        |bytes, key| {
            bytes
                .checked_add(CATALOG_ENTRY_BUDGET)
                .and_then(|n| n.checked_add(key.capacity()))
                .ok_or(Error::LimitExceeded)
        },
    )
}
fn snapshot_budget() -> usize {
    std::mem::size_of::<Snapshot>() + 64
}
fn edit_catalog<'a>(
    index: &Index,
    catalog: &'a mut Arc<Catalog>,
    additional: usize,
) -> Result<&'a mut Catalog> {
    let requested = catalog_budget(&catalog.map)?
        .checked_add(additional)
        .ok_or(Error::LimitExceeded)?;
    if Arc::get_mut(catalog).is_none() {
        let reservation = index.reserve_metadata(requested)?;
        *catalog = Arc::new(Catalog {
            map: catalog.map.clone(),
            ids: catalog.ids.clone(),
            names_bytes: catalog.names_bytes,
            reservation,
        });
    } else {
        Arc::get_mut(catalog)
            .unwrap()
            .reservation
            .resize(requested)?;
    }
    Ok(Arc::get_mut(catalog).unwrap())
}
enum Metadata {
    Create(Vec<u8>, u64),
    Drop(Vec<u8>, u64),
}
struct Writer {
    journal: Journal,
    changes: Vec<Change>,
    metadata: Vec<Metadata>,
    payload: Vec<u8>,
    checkpoint: Vec<u8>,
    poisoned: bool,
    metrics: Metrics,
    versions: Option<PoolWriter>,
    collection_work: usize,
    input: ulmdb_storage::snapshot_input::Arena,
}

fn build_lookup(snapshot: &Snapshot, options: Options) -> Result<Option<Arc<lookup::Lookup>>> {
    let Some(view) = snapshot.values.as_ref().filter(|_| options.exact_lookup) else {
        return Ok(None);
    };
    let max_nodes = options.version_limits()?.max_heads;
    let buckets = (max_nodes / 2).clamp(256, 65536).next_power_of_two();
    match lookup::Lookup::new(&snapshot.index, view, buckets, max_nodes) {
        Ok(index) => Ok(Some(index)),
        // This is a derived index. Refusing its allocation leaves the ordered
        // index authoritative and cannot make an otherwise valid open fail.
        Err(Error::LimitExceeded) => Ok(None),
        Err(error) => Err(error),
    }
}

pub struct Environment {
    current: ArcSwap<Snapshot>,
    writer: Mutex<Writer>,
    options: Options,
}
#[derive(Clone, Debug)]
pub struct Stats {
    pub sequence: u64,
    pub index: index::Stats,
    pub journal: ulmdb_storage::snapshot_log::Stats,
    pub transaction_capacity_bytes: usize,
    pub checkpoint_capacity_bytes: usize,
    pub staged_capacity: usize,
    pub metrics: Metrics,
    pub versions: Option<ulmdb_storage::snapshot_versions::Stats>,
    pub exact_lookup: bool,
    /// These bytes are already included in `index.charged_bytes`.
    pub lookup: Option<LookupStats>,
    pub staged_input: ulmdb_storage::snapshot_input::Stats,
}

#[derive(Clone, Copy, Debug)]
pub struct LookupStats {
    pub enabled: bool,
    pub buckets: usize,
    pub max_nodes: usize,
    pub live_nodes: usize,
    pub node_bytes: usize,
    pub header_bytes: usize,
    pub workspace_bytes: usize,
    pub retired_nodes: usize,
    pub built_nodes: usize,
    pub published_inserts: u64,
    pub published_deletes: u64,
    pub gc_steps: u64,
    pub base_entries: usize,
    pub base_bytes: usize,
    pub base_capacity: usize,
    pub delta_nodes: usize,
    pub base_build_work: u64,
    pub initialized_buckets: usize,
}
impl From<lookup::Stats> for LookupStats {
    fn from(stats: lookup::Stats) -> Self {
        Self {
            enabled: stats.enabled,
            buckets: stats.buckets,
            max_nodes: stats.max_nodes,
            live_nodes: stats.live_nodes,
            node_bytes: stats.node_bytes,
            header_bytes: stats.header_bytes,
            workspace_bytes: stats.workspace_bytes,
            retired_nodes: stats.retired_nodes,
            built_nodes: stats.built_nodes,
            published_inserts: stats.published_inserts,
            published_deletes: stats.published_deletes,
            gc_steps: stats.gc_steps,
            base_entries: stats.base_entries,
            base_bytes: stats.base_bytes,
            base_capacity: stats.base_capacity,
            delta_nodes: stats.delta_nodes,
            base_build_work: stats.base_build_work,
            initialized_buckets: stats.initialized_buckets,
        }
    }
}

impl Environment {
    pub fn create(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        let options = options.validate()?;
        let index = Index::new(options.tree_bytes())?;
        let (versions, values) = if options.versioned_values {
            let (w, v, heads) = PoolWriter::seed(0, &[], options.version_limits()?)?;
            debug_assert!(heads.is_empty());
            (Some(w), Some(v))
        } else {
            (None, None)
        };
        let map = BTreeMap::new();
        let catalog = Arc::new(Catalog {
            reservation: index.reserve_metadata(catalog_budget(&map)?)?,
            map,
            ids: BTreeSet::new(),
            names_bytes: 0,
        });
        let reservation = index.reserve_metadata(snapshot_budget())?;
        let mut snapshot = Snapshot {
            index,
            catalog,
            next_database: 1,
            sequence: 0,
            encoded_bytes: STATE_HEADER,
            _reservation: reservation,
            values,
            lookup: None,
        };
        snapshot.lookup = build_lookup(&snapshot, options)?;
        let mut initial = Vec::new();
        encode_state(&snapshot, &mut initial, options)?;
        let journal = Journal::create(path.as_ref(), options.journal_capacity, &initial)?;
        Ok(Self::assemble(snapshot, journal, versions, options))
    }
    pub fn open(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        let options = options.validate()?;
        let (journal, checkpoint, effects) =
            Journal::open(path.as_ref(), options.journal_capacity)?;
        let (mut snapshot, mut versions) = decode_initial(&checkpoint, options)?;
        for frame in effects {
            if let Some(pool) = versions.as_mut() {
                pool.collect(options.max_operations.saturating_mul(4).saturating_add(64));
            }
            snapshot = apply_effects(snapshot, &frame, options, versions.as_mut())?;
        }
        if snapshot.sequence != journal.sequence() {
            return Err(Error::Corrupt("snapshot-log replay sequence"));
        }
        snapshot.lookup = build_lookup(&snapshot, options)?;
        Ok(Self::assemble(snapshot, journal, versions, options))
    }
    fn assemble(
        snapshot: Snapshot,
        journal: Journal,
        versions: Option<PoolWriter>,
        options: Options,
    ) -> Self {
        Self {
            current: ArcSwap::from_pointee(snapshot),
            writer: Mutex::new(Writer {
                journal,
                changes: Vec::new(),
                metadata: Vec::new(),
                payload: Vec::new(),
                checkpoint: Vec::new(),
                poisoned: false,
                metrics: Metrics::default(),
                versions,
                collection_work: 64,
                input: ulmdb_storage::snapshot_input::Arena::new(options.transaction_bytes),
            }),
            options,
        }
    }
    pub fn durability(&self) -> Durability {
        Durability::Full
    }
    pub fn begin_read(&self) -> ReadTxn {
        ReadTxn {
            snapshot: self.current.load_full(),
        }
    }
    pub fn begin_write(&self) -> Result<WriteTxn<'_>> {
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if writer.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let work = writer.collection_work;
        if let Some(pool) = writer.versions.as_mut() {
            pool.collect(work);
        }
        writer.changes.clear();
        writer.metadata.clear();
        writer.input.reset();
        let base = self.current.load_full();
        if let (Some(lookup), Some(pool)) = (&base.lookup, &writer.versions) {
            match lookup.collect(pool.oldest_view_epoch(), work) {
                Ok(_) | Err(Error::LimitExceeded) => {}
                Err(error) => return Err(error),
            }
        }
        if writer.journal.sequence() != base.sequence {
            writer.poisoned = true;
            return Err(Error::Corrupt("snapshot-log live sequence"));
        }
        Ok(WriteTxn {
            environment: self,
            catalog: base.catalog.clone(),
            next_database: base.next_database,
            base,
            writer,
            input_bytes: 0,
            failed: false,
        })
    }
    pub fn stats(&self) -> Result<Stats> {
        let w = self.writer.lock().map_err(|_| Error::Busy)?;
        let s = self.current.load();
        Ok(Stats {
            sequence: s.sequence,
            index: s.index.stats(),
            journal: w.journal.stats(),
            transaction_capacity_bytes: w.payload.capacity(),
            checkpoint_capacity_bytes: w.checkpoint.capacity(),
            staged_capacity: w.changes.capacity(),
            metrics: w.metrics,
            versions: w.versions.as_ref().map(PoolWriter::stats),
            exact_lookup: s.lookup.as_ref().is_some_and(|lookup| lookup.is_enabled()),
            lookup: s.lookup.as_ref().map(|lookup| lookup.stats().into()),
            staged_input: w.input.stats(),
        })
    }
    /// Explicit runtime reclamation after releasing historical read views.
    /// Callers timing this operation must charge it alongside their writes.
    pub fn collect_runtime(&self, work: usize) -> Result<usize> {
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        let mut done = writer
            .versions
            .as_mut()
            .map_or(0, |pool| pool.collect(work));
        let snapshot = self.current.load();
        if let (Some(lookup), Some(pool)) = (&snapshot.lookup, &writer.versions) {
            done = done.saturating_add(lookup.collect(pool.oldest_view_epoch(), work)?);
        }
        Ok(done)
    }
}

pub struct ReadTxn {
    snapshot: Arc<Snapshot>,
}
impl ReadTxn {
    pub fn sequence(&self) -> u64 {
        self.snapshot.sequence
    }
    pub fn kv(&self, name: &[u8]) -> Result<KvRead<'_>> {
        let db = *self
            .snapshot
            .catalog
            .get(name)
            .ok_or(Error::MissingDatabase)?;
        Ok(KvRead {
            snapshot: &self.snapshot,
            database: db,
        })
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.kv(name)?.get(key)
    }
}
pub struct KvRead<'a> {
    snapshot: &'a Snapshot,
    database: u64,
}
impl<'a> KvRead<'a> {
    #[inline(always)]
    pub fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        if key.len() > MAX_KEY {
            return Err(Error::InvalidInput("snapshot-log key size"));
        }
        self.snapshot.value(self.database, key)
    }
    pub fn cursor(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Cursor<'a>> {
        Ok(Cursor {
            iter: self
                .snapshot
                .index
                .scan(self.database, lower, upper, reverse),
            values: self.snapshot.values.as_ref(),
        })
    }
}
pub struct Cursor<'a> {
    iter: index::ScanIter<'a>,
    values: Option<&'a VersionView>,
}
impl<'a> Iterator for Cursor<'a> {
    type Item = Result<(&'a [u8], &'a [u8])>;
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let (_, key, value) = self.iter.next()?;
        Some(match self.values {
            Some(values) => versioned::decode_handle(value).and_then(|head| {
                values
                    .get(head)
                    .map(|value| (key, value))
                    .ok_or(Error::Corrupt("snapshot-log stale cursor value"))
            }),
            None => Ok((key, value)),
        })
    }
}

pub struct WriteTxn<'a> {
    environment: &'a Environment,
    base: Arc<Snapshot>,
    writer: MutexGuard<'a, Writer>,
    catalog: Arc<Catalog>,
    next_database: u64,
    input_bytes: usize,
    failed: bool,
}
impl WriteTxn<'_> {
    fn catalog_mut(&mut self, additional: usize) -> Result<&mut Catalog> {
        edit_catalog(&self.base.index, &mut self.catalog, additional)
    }
    fn live(&self) -> Result<()> {
        if self.failed {
            return Err(Error::InvalidInput(
                "snapshot-log failed transaction requires abort",
            ));
        }
        if self.writer.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        Ok(())
    }
    fn database(&self, name: &[u8]) -> Result<u64> {
        self.live()?;
        self.catalog
            .get(name)
            .copied()
            .ok_or(Error::MissingDatabase)
    }
    fn reserve(&mut self, bytes: usize) -> Result<()> {
        if self.writer.changes.len() + self.writer.metadata.len()
            >= self.environment.options.max_operations
        {
            return Err(Error::LimitExceeded);
        }
        self.input_bytes = self
            .input_bytes
            .checked_add(bytes)
            .ok_or(Error::LimitExceeded)?;
        if self.input_bytes > self.environment.options.transaction_bytes {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
    pub fn create_database(&mut self, name: &[u8], kind: DatabaseKind) -> Result<()> {
        let result = (|| {
            self.live()?;
            if kind != DatabaseKind::Kv {
                return Err(Error::InvalidInput(
                    "snapshot-log supports bytewise KV databases only",
                ));
            }
            if name.is_empty() || name.len() > 255 || self.catalog.contains_key(name) {
                return Err(Error::InvalidInput("snapshot-log database name"));
            }
            if self.catalog.len() >= self.environment.options.max_databases {
                return Err(Error::LimitExceeded);
            }
            self.reserve(16 + name.len())?;
            let id = self.next_database;
            self.next_database = id.checked_add(1).ok_or(Error::LimitExceeded)?;
            self.writer
                .metadata
                .try_reserve(1)
                .map_err(|_| Error::LimitExceeded)?;
            let catalog = self.catalog_mut(CATALOG_ENTRY_BUDGET + 255)?;
            catalog.map.insert(name.to_vec(), id);
            catalog.ids.insert(id);
            catalog.names_bytes = catalog
                .names_bytes
                .checked_add(10 + name.len())
                .ok_or(Error::LimitExceeded)?;
            catalog.reservation.resize(catalog_budget(&catalog.map)?)?;
            self.writer
                .metadata
                .push(Metadata::Create(name.to_vec(), id));
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        let db = self.database(name)?;
        self.get_id(db, key)
    }
    fn get_id(&self, db: u64, key: &[u8]) -> Result<Option<&[u8]>> {
        for change in self.writer.changes.iter().rev() {
            if change.database == db && change.key.as_slice() == key {
                return Ok(change.value.as_ref().map(Bytes::as_slice));
            }
        }
        self.base.value(db, key)
    }
    pub fn put(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<()> {
        let clock = profile::start();
        let result = (|| {
            let db = self.database(name)?;
            if key.len() > MAX_KEY || value.len() > MAX_VALUE {
                return Err(Error::InvalidInput("snapshot-log key/value size"));
            }
            self.reserve(17 + key.len() + value.len())?;
            let change = Change {
                database: db,
                key: Bytes::new(key)?,
                value: Some(Bytes::staged(value, &mut self.writer.input)?),
            };
            self.writer
                .changes
                .try_reserve(1)
                .map_err(|_| Error::LimitExceeded)?;
            self.writer.changes.push(change);
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        profile::finish(&mut self.writer.metrics, profile::Stage::Put, clock);
        result
    }
    pub fn delete_with_presence(&mut self, name: &[u8], key: &[u8]) -> Result<bool> {
        let result = (|| {
            let db = self.database(name)?;
            let Some(_) = self.get_id(db, key)? else {
                return Ok(false);
            };
            self.reserve(17 + key.len())?;
            let key = Bytes::new(key)?;
            self.writer
                .changes
                .try_reserve(1)
                .map_err(|_| Error::LimitExceeded)?;
            self.writer.changes.push(Change {
                database: db,
                key,
                value: None,
            });
            Ok(true)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn delete(&mut self, name: &[u8], key: &[u8]) -> Result<()> {
        self.delete_with_presence(name, key).map(|_| ())
    }
    pub fn drop_database(&mut self, name: &[u8]) -> Result<()> {
        let result = (|| {
            let db = self.database(name)?;
            let mut keys: Vec<Bytes> = Vec::new();
            for (d, k, _) in self.base.index.iter() {
                if d == db {
                    if keys.len() >= self.environment.options.max_operations {
                        return Err(Error::LimitExceeded);
                    }
                    keys.push(Bytes::new(k)?);
                }
            }
            for change in &self.writer.changes {
                if change.database == db {
                    if keys.len() >= self.environment.options.max_operations {
                        return Err(Error::LimitExceeded);
                    }
                    keys.push(change.key.clone());
                }
            }
            for key in keys {
                self.delete(name, key.as_slice())?;
            }
            self.reserve(16 + name.len())?;
            self.writer
                .metadata
                .try_reserve(1)
                .map_err(|_| Error::LimitExceeded)?;
            let catalog = self.catalog_mut(0)?;
            catalog.map.remove(name);
            catalog.ids.remove(&db);
            catalog.names_bytes = catalog
                .names_bytes
                .checked_sub(10 + name.len())
                .ok_or(Error::LimitExceeded)?;
            catalog.reservation.resize(catalog_budget(&catalog.map)?)?;
            self.writer.metadata.push(Metadata::Drop(name.to_vec(), db));
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn commit(mut self) -> Result<u64> {
        self.live()?;
        if self.writer.changes.is_empty() && self.writer.metadata.is_empty() {
            return Ok(self.base.sequence);
        }
        let sequence = self
            .base
            .sequence
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        if self.writer.versions.is_some() {
            return self.commit_versioned(sequence);
        }
        let clock = profile::start();
        let index = self.base.index.prepare_hybrid(
            &self.writer.changes,
            self.environment.options.batch_threshold,
        )?;
        // Grouped edits may reuse the original root after A->B->A. That
        // transaction still performed real writes and must publish just as
        // the point path does. Only effects identical to the selected base
        // throughout are the ordinary no-op optimization.
        if self.writer.metadata.is_empty()
            && index.same_state(&self.base.index)
            && self.writer.changes.iter().all(|change| {
                change.value.as_ref().map(Bytes::as_slice)
                    == self.base.index.get(change.database, change.key.as_slice())
            })
        {
            return Ok(self.base.sequence);
        }
        let encoded_bytes = STATE_HEADER
            .checked_add(self.catalog.names_bytes)
            .and_then(|n| n.checked_add(index.encoded_rows_bytes()))
            .ok_or(Error::LimitExceeded)?;
        if encoded_bytes > self.environment.options.journal_capacity - 4096 {
            return Err(Error::LimitExceeded);
        }
        let reservation = index.reserve_metadata(snapshot_budget())?;
        let next = Arc::new(Snapshot {
            index,
            catalog: self.catalog.clone(),
            next_database: self.next_database,
            sequence,
            encoded_bytes,
            _reservation: reservation,
            values: None,
            lookup: None,
        });
        profile::finish(&mut self.writer.metrics, profile::Stage::Prepare, clock);
        let clock = profile::start();
        {
            let Writer {
                changes,
                metadata,
                payload,
                journal,
                ..
            } = &mut *self.writer;
            encode_effects(
                sequence,
                self.next_database,
                metadata,
                changes,
                payload,
                self.environment.options,
                journal.uses_compact_frames(),
            )?;
        }
        profile::finish(&mut self.writer.metrics, profile::Stage::Encode, clock);
        let has_row_changes = !self.writer.changes.is_empty();
        if self.writer.journal.remaining_transaction(has_row_changes)
            < self.writer.payload.len().saturating_add(4096)
        {
            let clock = profile::start();
            if self
                .base
                .encoded_bytes
                .checked_add(self.writer.payload.len())
                .and_then(|n| n.checked_add(4096))
                .is_none_or(|n| n > self.environment.options.journal_capacity)
            {
                return Err(Error::LimitExceeded);
            }
            encode_state(
                &self.base,
                &mut self.writer.checkpoint,
                self.environment.options,
            )?;
            let Writer {
                journal,
                checkpoint,
                poisoned,
                ..
            } = &mut *self.writer;
            if journal.checkpoint(checkpoint).is_err() {
                *poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            profile::finish(&mut self.writer.metrics, profile::Stage::Checkpoint, clock);
        }
        // A checkpoint can change physical generation without publishing a
        // business transaction. A definite capacity refusal keeps the owner
        // usable and must not become an uncertain commit outcome.
        if self.writer.journal.remaining_transaction(has_row_changes) < self.writer.payload.len() {
            return Err(Error::LimitExceeded);
        }
        let clock = profile::start();
        let Writer {
            journal,
            payload,
            poisoned,
            ..
        } = &mut *self.writer;
        let committed = match journal.append_transaction(payload, has_row_changes) {
            Ok(seq) => seq,
            Err(_) => {
                *poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
        };
        if committed != sequence {
            *poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        profile::finish(&mut self.writer.metrics, profile::Stage::Append, clock);
        let clock = profile::start();
        self.environment.current.store(next);
        profile::finish(&mut self.writer.metrics, profile::Stage::Publish, clock);
        Ok(sequence)
    }
    fn commit_versioned(&mut self, sequence: u64) -> Result<u64> {
        let clock = profile::start();
        let view = self
            .base
            .values
            .as_ref()
            .ok_or(Error::Corrupt("snapshot-log missing value view"))?;
        let rows = self
            .base
            .encoded_bytes
            .checked_sub(STATE_HEADER + self.base.catalog.names_bytes)
            .ok_or(Error::Corrupt("snapshot-log logical row bytes"))?;
        let Writer {
            versions,
            journal,
            changes,
            metadata,
            payload,
            checkpoint,
            metrics,
            poisoned,
            collection_work,
            ..
        } = &mut *self.writer;
        let plan = match versioned::Plan::build_with_lookup(
            &self.base.index,
            view,
            changes,
            rows,
            self.base
                .lookup
                .as_deref()
                .filter(|lookup| lookup.is_enabled()),
        ) {
            Ok(plan) => plan,
            Err(Error::LimitExceeded) if self.base.release_lookup() => {
                versioned::Plan::build_with_lookup(&self.base.index, view, changes, rows, None)?
            }
            Err(error) => return Err(error),
        };
        if metadata.is_empty() && !plan.dirty() {
            return Ok(self.base.sequence);
        }
        let encoded_bytes = STATE_HEADER
            .checked_add(self.catalog.names_bytes)
            .and_then(|n| n.checked_add(plan.rows_bytes()))
            .ok_or(Error::LimitExceeded)?;
        if encoded_bytes > self.environment.options.journal_capacity - 4096 {
            return Err(Error::LimitExceeded);
        }
        // The usual one-key replacement uses an inline mutation, with no
        // temporary vector allocation or ordered-tree edit.
        let single = if plan.len() == 1 {
            Some(plan.mutation(0))
        } else {
            None
        };
        let mut many = Vec::new();
        if plan.len() > 1 {
            many.try_reserve_exact(plan.len())
                .map_err(|_| Error::LimitExceeded)?;
            for i in 0..plan.len() {
                many.push(plan.mutation(i));
            }
        }
        let mutations = if plan.len() > 1 {
            many.as_slice()
        } else {
            single.as_slice()
        };
        let options = self.environment.options;
        let pool = versions
            .as_mut()
            .ok_or(Error::Corrupt("snapshot-log missing value owner"))?;
        let ready = pool.prepare(sequence, mutations)?;
        let member_one = if plan.len() == 1 {
            plan.membership_one(ready.heads()[0])?
        } else {
            None
        };
        let member_many = if plan.len() > 1 {
            plan.membership(ready.heads())?
        } else {
            Vec::new()
        };
        let membership = if plan.len() > 1 {
            member_many.as_slice()
        } else {
            member_one.as_slice()
        };
        // Bind these final effects to this exact base and opaque pool
        // preparation once. The tree and derived hash index can then consume
        // the same proof without each repeating ordered searches per key.
        let proof = if membership.is_empty() {
            None
        } else {
            Some(plan.prove_membership(sequence, &ready, membership)?)
        };
        let prepare_index = || match &proof {
            Some(proof) => versioned::prepare_membership_proven(
                &self.base.index,
                view,
                sequence,
                proof,
                options.batch_threshold,
            ),
            None => {
                versioned::prepare_membership(&self.base.index, membership, options.batch_threshold)
            }
        };
        let index = match prepare_index() {
            Ok(index) => index,
            Err(Error::LimitExceeded) if self.base.release_lookup() => prepare_index()?,
            Err(error) => return Err(error),
        };
        let reservation = match index.reserve_metadata(snapshot_budget()) {
            Ok(reservation) => reservation,
            Err(Error::LimitExceeded) if self.base.release_lookup() => {
                index.reserve_metadata(snapshot_budget())?
            }
            Err(error) => return Err(error),
        };
        let mut next_lookup = self
            .base
            .lookup
            .as_ref()
            .filter(|lookup| lookup.is_enabled())
            .cloned();
        let lookup_ready = if membership.is_empty() {
            None
        } else if let Some(lookup) = self
            .base
            .lookup
            .as_ref()
            .filter(|lookup| lookup.is_enabled())
        {
            match lookup.prepare_proven_with_next(
                sequence,
                proof.as_ref().expect("nonempty membership proof"),
                &self.base.index,
                view,
                &index,
            ) {
                Ok(ready) => Some(ready),
                Err(Error::LimitExceeded) => {
                    // A failed derived-index preparation publishes nothing.
                    // Future snapshots retain the authoritative ordered path.
                    lookup.disable();
                    next_lookup = None;
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        if let Some(rebased) = lookup_ready.as_ref().and_then(|ready| ready.next_lookup()) {
            next_lookup = Some(rebased);
        }
        let mut next = Arc::new(Snapshot {
            index,
            catalog: self.catalog.clone(),
            next_database: self.next_database,
            sequence,
            encoded_bytes,
            _reservation: reservation,
            values: None,
            lookup: next_lookup,
        });
        profile::finish(metrics, profile::Stage::Prepare, clock);
        let clock = profile::start();
        encode_effects(
            sequence,
            self.next_database,
            metadata,
            changes,
            payload,
            options,
            journal.uses_compact_frames(),
        )?;
        profile::finish(metrics, profile::Stage::Encode, clock);
        let has_row_changes = !changes.is_empty();
        if journal.remaining_transaction(has_row_changes) < payload.len().saturating_add(4096) {
            if self
                .base
                .encoded_bytes
                .checked_add(payload.len())
                .and_then(|n| n.checked_add(4096))
                .is_none_or(|n| n > options.journal_capacity)
            {
                return Err(Error::LimitExceeded);
            }
            let clock = profile::start();
            encode_state(&self.base, checkpoint, options)?;
            if journal.checkpoint(checkpoint).is_err() {
                *poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            profile::finish(metrics, profile::Stage::Checkpoint, clock);
        }
        if journal.remaining_transaction(has_row_changes) < payload.len() {
            return Err(Error::LimitExceeded);
        }
        let clock = profile::start();
        let actual = match journal.append_transaction(payload, has_row_changes) {
            Ok(seq) => seq,
            Err(_) => {
                *poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
        };
        if actual != sequence {
            *poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        profile::finish(metrics, profile::Stage::Append, clock);
        let clock = profile::start();
        let published = ready.publish();
        if let Some(lookup) = lookup_ready {
            lookup.publish();
        }
        // No future View escapes before all head links are installed. This Arc
        // is still private and uniquely owned, and the assignment allocates
        // nothing after the durable log acknowledgement point.
        Arc::get_mut(&mut next)
            .expect("private versioned publication")
            .values = Some(published);
        self.environment.current.store(next);
        *collection_work = plan.len().saturating_mul(4).saturating_add(64);
        profile::finish(metrics, profile::Stage::Publish, clock);
        Ok(sequence)
    }
    pub fn abort(self) {}
}
impl Drop for WriteTxn<'_> {
    fn drop(&mut self) {
        let clock = profile::start();
        self.writer.changes.clear();
        self.writer.metadata.clear();
        profile::finish(&mut self.writer.metrics, profile::Stage::StagedClear, clock);
    }
}

fn room(out: &mut Vec<u8>, bytes: usize, limit: usize) -> Result<()> {
    if out.len().checked_add(bytes).is_none_or(|n| n > limit) {
        return Err(Error::LimitExceeded);
    }
    out.try_reserve(bytes).map_err(|_| Error::LimitExceeded)
}
fn push(out: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<()> {
    room(out, bytes.len(), limit)?;
    out.extend_from_slice(bytes);
    Ok(())
}
fn encode_state(snapshot: &Snapshot, out: &mut Vec<u8>, options: Options) -> Result<()> {
    out.clear();
    room(out, snapshot.encoded_bytes, options.journal_capacity - 4096)?;
    out.extend_from_slice(STATE);
    out.extend_from_slice(&snapshot.sequence.to_le_bytes());
    out.extend_from_slice(&snapshot.next_database.to_le_bytes());
    out.extend_from_slice(&(snapshot.catalog.len() as u32).to_le_bytes());
    let count = snapshot.index.iter().count();
    out.extend_from_slice(&(count as u64).to_le_bytes());
    for (name, id) in snapshot.catalog.iter() {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&id.to_le_bytes());
    }
    for (db, k, v) in snapshot.index.iter() {
        let v = snapshot.resolve(v)?;
        out.extend_from_slice(&db.to_le_bytes());
        out.extend_from_slice(&(k.len() as u32).to_le_bytes());
        out.extend_from_slice(&(v.len() as u32).to_le_bytes());
        out.extend_from_slice(k);
        out.extend_from_slice(v);
    }
    if out.len() != snapshot.encoded_bytes {
        return Err(Error::Corrupt("snapshot-log snapshot size accounting"));
    }
    Ok(())
}
fn encode_effects(
    sequence: u64,
    next: u64,
    metadata: &[Metadata],
    changes: &[Change],
    out: &mut Vec<u8>,
    options: Options,
    compact: bool,
) -> Result<()> {
    if compact {
        encode_effects_v2(sequence, next, metadata, changes, out, options)
    } else {
        encode_effects_v1(sequence, next, metadata, changes, out, options)
    }
}
fn encode_effects_v1(
    sequence: u64,
    next: u64,
    metadata: &[Metadata],
    changes: &[Change],
    out: &mut Vec<u8>,
    options: Options,
) -> Result<()> {
    out.clear();
    let limit = options.transaction_bytes;
    push(out, EFFECTS, limit)?;
    push(out, &sequence.to_le_bytes(), limit)?;
    push(out, &next.to_le_bytes(), limit)?;
    push(out, &(metadata.len() as u32).to_le_bytes(), limit)?;
    push(out, &(changes.len() as u32).to_le_bytes(), limit)?;
    for m in metadata {
        let (tag, name, id) = match m {
            Metadata::Create(n, id) => (1u8, n, id),
            Metadata::Drop(n, id) => (2u8, n, id),
        };
        push(out, &[tag], limit)?;
        push(out, &(name.len() as u16).to_le_bytes(), limit)?;
        push(out, name, limit)?;
        push(out, &id.to_le_bytes(), limit)?;
    }
    for c in changes {
        push(out, &c.database.to_le_bytes(), limit)?;
        push(out, &(c.key.as_slice().len() as u32).to_le_bytes(), limit)?;
        let n = c
            .value
            .as_ref()
            .map_or(u32::MAX, |v| v.as_slice().len() as u32);
        push(out, &n.to_le_bytes(), limit)?;
        push(out, c.key.as_slice(), limit)?;
        if let Some(v) = &c.value {
            push(out, v.as_slice(), limit)?;
        }
    }
    Ok(())
}
fn legacy_effect_budget(bytes: &mut usize, additional: usize, limit: usize) -> Result<()> {
    *bytes = bytes
        .checked_add(additional)
        .filter(|bytes| *bytes <= limit)
        .ok_or(Error::LimitExceeded)?;
    Ok(())
}
fn encode_effects_v2(
    sequence: u64,
    next: u64,
    metadata: &[Metadata],
    changes: &[Change],
    out: &mut Vec<u8>,
    options: Options,
) -> Result<()> {
    if metadata
        .len()
        .checked_add(changes.len())
        .is_none_or(|count| count > options.max_operations)
    {
        return Err(Error::LimitExceeded);
    }
    out.clear();
    let limit = options.transaction_bytes;
    let mut legacy_bytes = 32;
    if legacy_bytes > limit {
        return Err(Error::LimitExceeded);
    }
    push(out, EFFECTS_V2, limit)?;
    for value in [sequence, next, metadata.len() as u64, changes.len() as u64] {
        effects::put(out, value, limit)?;
    }
    for item in metadata {
        let (tag, name, id) = match item {
            Metadata::Create(name, id) => (1, name, id),
            Metadata::Drop(name, id) => (2, name, id),
        };
        if name.is_empty() || name.len() > 255 || *id == 0 || *id >= next {
            return Err(Error::InvalidInput("snapshot-log metadata encoding bounds"));
        }
        legacy_effect_budget(&mut legacy_bytes, 11 + name.len(), limit)?;
        push(out, &[tag], limit)?;
        effects::put(out, name.len() as u64, limit)?;
        push(out, name, limit)?;
        effects::put(out, *id, limit)?;
    }
    for change in changes {
        let key = change.key.as_slice();
        let value = change.value.as_ref().map(Bytes::as_slice);
        if change.database == 0
            || key.len() > MAX_KEY
            || value.is_some_and(|value| value.len() > MAX_VALUE)
        {
            return Err(Error::InvalidInput("snapshot-log effect encoding bounds"));
        }
        legacy_effect_budget(
            &mut legacy_bytes,
            16 + key.len() + value.map_or(0, <[u8]>::len),
            limit,
        )?;
        effects::put(out, change.database, limit)?;
        effects::put(out, key.len() as u64, limit)?;
        effects::put(out, value.map_or(0, |value| value.len() as u64 + 1), limit)?;
        push(out, key, limit)?;
        if let Some(value) = value {
            push(out, value, limit)?;
        }
    }
    Ok(())
}
struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Input<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(Error::Corrupt("snapshot-log extent"))?;
        let b = self
            .bytes
            .get(self.at..end)
            .ok_or(Error::Corrupt("snapshot-log short input"))?;
        self.at = end;
        Ok(b)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn field(&mut self, compact: bool) -> Result<u64> {
        if compact {
            effects::get(self)
        } else {
            self.u64()
        }
    }
    fn count(&mut self, compact: bool) -> Result<usize> {
        if compact {
            usize::try_from(effects::get(self)?)
                .map_err(|_| Error::Corrupt("snapshot-log count width"))
        } else {
            Ok(self.u32()? as usize)
        }
    }
    fn finish(self) -> Result<()> {
        if self.at != self.bytes.len() {
            return Err(Error::Corrupt("snapshot-log trailing input"));
        }
        Ok(())
    }
}
fn decode_initial(bytes: &[u8], options: Options) -> Result<(Snapshot, Option<PoolWriter>)> {
    if !options.versioned_values {
        return decode_state(bytes, options).map(|snapshot| (snapshot, None));
    }
    let mut r = Input { bytes, at: 0 };
    if r.take(8)? != STATE {
        return Err(Error::Corrupt("snapshot-log checkpoint type"));
    }
    let sequence = r.u64()?;
    let next_database = r.u64()?;
    let db_count = r.u32()? as usize;
    let rows = usize::try_from(r.u64()?).map_err(|_| Error::LimitExceeded)?;
    let limits = options.version_limits()?;
    if db_count > options.max_databases
        || next_database == 0
        || rows > bytes.len() / 16
        || rows > limits.max_heads
    {
        return Err(Error::Corrupt("snapshot-log versioned checkpoint bounds"));
    }
    let mut index = Index::new(options.tree_bytes())?;
    let requested = (std::mem::size_of::<Catalog>() + 64 + CATALOG_ENTRY_BUDGET)
        .checked_add(
            db_count
                .checked_mul(CATALOG_ENTRY_BUDGET + 255)
                .ok_or(Error::LimitExceeded)?,
        )
        .ok_or(Error::LimitExceeded)?;
    let mut catalog_reservation = index.reserve_metadata(requested)?;
    let scratch_bytes = rows
        .checked_mul(
            std::mem::size_of::<(u64, &[u8], &[u8])>()
                + std::mem::size_of::<&[u8]>()
                + std::mem::size_of::<ulmdb_storage::snapshot_versions::Head>()
                + std::mem::size_of::<Change>(),
        )
        .ok_or(Error::LimitExceeded)?;
    let _scratch = index.reserve_metadata(scratch_bytes)?;
    let mut map = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for _ in 0..db_count {
        let n = r.u16()? as usize;
        let name = r.take(n)?;
        let id = r.u64()?;
        if n == 0
            || n > 255
            || id == 0
            || id >= next_database
            || !ids.insert(id)
            || map.insert(name.to_vec(), id).is_some()
        {
            return Err(Error::Corrupt("snapshot-log versioned catalog"));
        }
    }
    let mut records = Vec::new();
    let mut values = Vec::new();
    records
        .try_reserve_exact(rows)
        .map_err(|_| Error::LimitExceeded)?;
    values
        .try_reserve_exact(rows)
        .map_err(|_| Error::LimitExceeded)?;
    let mut previous = None;
    for _ in 0..rows {
        let db = r.u64()?;
        let kl = r.u32()? as usize;
        let vl = r.u32()? as usize;
        let key = r.take(kl)?;
        let value = r.take(vl)?;
        if !ids.contains(&db)
            || kl > MAX_KEY
            || vl > MAX_VALUE
            || previous.is_some_and(|old| old >= (db, key))
        {
            return Err(Error::Corrupt("snapshot-log versioned checkpoint rows"));
        }
        previous = Some((db, key));
        records.push((db, key, value));
        values.push(value);
    }
    r.finish()?;
    let (pool, view, heads) = PoolWriter::seed(sequence, &values, limits)?;
    let mut changes = Vec::new();
    changes
        .try_reserve_exact(1024.min(rows))
        .map_err(|_| Error::LimitExceeded)?;
    for ((db, key, _), head) in records.into_iter().zip(heads) {
        changes.push(versioned::link(db, Bytes::new(key)?, head)?);
        if changes.len() == 1024 {
            index = versioned::prepare_membership(&index, &changes, options.batch_threshold)?;
            changes.clear();
        }
    }
    if !changes.is_empty() {
        index = versioned::prepare_membership(&index, &changes, options.batch_threshold)?;
    }
    catalog_reservation.resize(catalog_budget(&map)?)?;
    let names_bytes = map.keys().map(|n| 10 + n.len()).sum();
    let reservation = index.reserve_metadata(snapshot_budget())?;
    let snapshot = Snapshot {
        index,
        catalog: Arc::new(Catalog {
            map,
            ids,
            names_bytes,
            reservation: catalog_reservation,
        }),
        next_database,
        sequence,
        encoded_bytes: bytes.len(),
        _reservation: reservation,
        values: Some(view),
        lookup: None,
    };
    Ok((snapshot, Some(pool)))
}
fn decode_state(bytes: &[u8], options: Options) -> Result<Snapshot> {
    let mut r = Input { bytes, at: 0 };
    if r.take(8)? != STATE {
        return Err(Error::Corrupt("snapshot-log checkpoint type"));
    }
    let sequence = r.u64()?;
    let next_database = r.u64()?;
    let db_count = r.u32()? as usize;
    let rows = r.u64()?;
    if db_count > options.max_databases || next_database == 0 || rows > bytes.len() as u64 / 16 {
        return Err(Error::Corrupt("snapshot-log checkpoint bounds"));
    }
    let mut index = Index::new(options.tree_bytes())?;
    let metadata_max = (std::mem::size_of::<Catalog>() + 64 + CATALOG_ENTRY_BUDGET)
        .checked_add(
            db_count
                .checked_mul(CATALOG_ENTRY_BUDGET + 255)
                .ok_or(Error::LimitExceeded)?,
        )
        .ok_or(Error::LimitExceeded)?;
    let mut catalog_reservation = index.reserve_metadata(metadata_max)?;
    let mut catalog = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for _ in 0..db_count {
        let len = r.u16()? as usize;
        let name = r.take(len)?;
        let id = r.u64()?;
        if name.is_empty()
            || len > 255
            || id == 0
            || id >= next_database
            || !ids.insert(id)
            || catalog.insert(name.to_vec(), id).is_some()
        {
            return Err(Error::Corrupt("snapshot-log checkpoint catalog"));
        }
    }
    let mut changes = Vec::new();
    let mut previous: Option<(u64, &[u8])> = None;
    for _ in 0..rows {
        let db = r.u64()?;
        let kl = r.u32()? as usize;
        let vl = r.u32()? as usize;
        let k = r.take(kl)?;
        let v = r.take(vl)?;
        if !ids.contains(&db)
            || kl > MAX_KEY
            || vl > MAX_VALUE
            || previous.is_some_and(|old| old >= (db, k))
        {
            return Err(Error::Corrupt("snapshot-log checkpoint rows"));
        }
        previous = Some((db, k));
        changes.push(Change {
            database: db,
            key: Bytes::new(k)?,
            value: Some(Bytes::new(v)?),
        });
        if changes.len() == 1024 {
            index = index.prepare_hybrid(&changes, options.batch_threshold)?;
            changes.clear();
        }
    }
    if !changes.is_empty() {
        index = index.prepare_hybrid(&changes, options.batch_threshold)?;
    }
    r.finish()?;
    catalog_reservation.resize(catalog_budget(&catalog)?)?;
    let names_bytes = catalog.keys().map(|name| 10 + name.len()).sum();
    let reservation = index.reserve_metadata(snapshot_budget())?;
    Ok(Snapshot {
        index,
        catalog: Arc::new(Catalog {
            map: catalog,
            ids,
            names_bytes,
            reservation: catalog_reservation,
        }),
        next_database,
        sequence,
        encoded_bytes: bytes.len(),
        _reservation: reservation,
        values: None,
        lookup: None,
    })
}
fn apply_effects(
    base: Snapshot,
    bytes: &[u8],
    options: Options,
    versions: Option<&mut PoolWriter>,
) -> Result<Snapshot> {
    let mut r = Input { bytes, at: 0 };
    let compact = match r.take(8)? {
        magic if magic == EFFECTS => false,
        magic if magic == EFFECTS_V2 => true,
        _ => return Err(Error::Corrupt("snapshot-log transaction type")),
    };
    let sequence = r.field(compact)?;
    let next_database = r.field(compact)?;
    let metadata = r.count(compact)?;
    let count = r.count(compact)?;
    if sequence != base.sequence.checked_add(1).ok_or(Error::LimitExceeded)?
        || next_database < base.next_database
        || metadata
            .checked_add(count)
            .is_none_or(|n| n > options.max_operations)
        || bytes.len() > options.transaction_bytes
    {
        return Err(Error::Corrupt("snapshot-log transaction bounds"));
    }
    let minimum_encoded = metadata
        .checked_mul(if compact { 4 } else { 12 })
        .and_then(|bytes| {
            count
                .checked_mul(if compact { 3 } else { 16 })
                .and_then(|rows| bytes.checked_add(rows))
        })
        .ok_or(Error::Corrupt("snapshot-log transaction count extent"))?;
    let minimum_legacy = metadata
        .checked_mul(11)
        .and_then(|bytes| {
            count
                .checked_mul(16)
                .and_then(|rows| bytes.checked_add(rows))
        })
        .and_then(|bytes| bytes.checked_add(32))
        .ok_or(Error::Corrupt("snapshot-log transaction decoded extent"))?;
    if minimum_encoded > bytes.len() - r.at || minimum_legacy > options.transaction_bytes {
        return Err(Error::Corrupt("snapshot-log transaction count bounds"));
    }
    let mut legacy_bytes = 32;
    let mut catalog = base.catalog.clone();
    let mut known = if metadata == 0 {
        None
    } else {
        Some(base.catalog.ids.clone())
    };
    let mut had_drop = false;
    let mut expected_database = base.next_database;
    for _ in 0..metadata {
        let tag = r.take(1)?[0];
        let n = if compact {
            r.count(true)?
        } else {
            r.u16()? as usize
        };
        if n == 0 || n > 255 {
            return Err(Error::Corrupt("snapshot-log metadata bounds"));
        }
        let name = r.take(n)?;
        let db = r.field(compact)?;
        if n == 0 || n > 255 || db == 0 || db >= next_database {
            return Err(Error::Corrupt("snapshot-log metadata bounds"));
        }
        legacy_effect_budget(&mut legacy_bytes, 11 + n, options.transaction_bytes)?;
        match tag {
            1 => {
                if db != expected_database
                    || known.as_ref().unwrap().contains(&db)
                    || catalog.len() >= options.max_databases
                    || catalog.contains_key(name)
                {
                    return Err(Error::Corrupt("snapshot-log create database"));
                }
                let edited = edit_catalog(&base.index, &mut catalog, CATALOG_ENTRY_BUDGET + 255)?;
                edited.map.insert(name.to_vec(), db);
                edited.ids.insert(db);
                edited.names_bytes = edited
                    .names_bytes
                    .checked_add(10 + n)
                    .ok_or(Error::LimitExceeded)?;
                edited.reservation.resize(catalog_budget(&edited.map)?)?;
                known.as_mut().unwrap().insert(db);
                expected_database = expected_database
                    .checked_add(1)
                    .ok_or(Error::LimitExceeded)?;
            }
            2 => {
                if catalog.get(name) != Some(&db) {
                    return Err(Error::Corrupt("snapshot-log drop database"));
                }
                let edited = edit_catalog(&base.index, &mut catalog, 0)?;
                edited.map.remove(name);
                edited.ids.remove(&db);
                edited.names_bytes = edited
                    .names_bytes
                    .checked_sub(10 + n)
                    .ok_or(Error::LimitExceeded)?;
                edited.reservation.resize(catalog_budget(&edited.map)?)?;
                had_drop = true;
            }
            _ => return Err(Error::Corrupt("snapshot-log metadata type")),
        }
    }
    if next_database != expected_database {
        return Err(Error::Corrupt("snapshot-log next database identity"));
    }
    let mut changes = Vec::new();
    changes
        .try_reserve(count)
        .map_err(|_| Error::LimitExceeded)?;
    for _ in 0..count {
        let database = r.field(compact)?;
        let kl = r.count(compact)?;
        let vl = if compact {
            let wire = effects::get(&mut r)?;
            if wire == 0 {
                None
            } else {
                Some(
                    usize::try_from(wire - 1)
                        .map_err(|_| Error::Corrupt("snapshot-log value width"))?,
                )
            }
        } else {
            let wire = r.u32()?;
            if wire == u32::MAX {
                None
            } else {
                Some(wire as usize)
            }
        };
        if !known
            .as_ref()
            .unwrap_or(&base.catalog.ids)
            .contains(&database)
            || kl > MAX_KEY
            || vl.is_some_and(|vl| vl > MAX_VALUE)
        {
            return Err(Error::Corrupt("snapshot-log effect bounds"));
        }
        legacy_effect_budget(
            &mut legacy_bytes,
            16 + kl + vl.unwrap_or(0),
            options.transaction_bytes,
        )?;
        let key = r.take(kl)?;
        let value = vl.map(|vl| Bytes::new(r.take(vl)?)).transpose()?;
        changes.push(Change {
            database,
            key: Bytes::new(key)?,
            value,
        });
    }
    r.finish()?;
    if let Some(pool) = versions {
        return replay_versioned(
            base,
            catalog,
            next_database,
            sequence,
            &changes,
            had_drop,
            pool,
            options,
        );
    }
    let index = base
        .index
        .prepare_hybrid(&changes, options.batch_threshold)?;
    if had_drop {
        for (db, _, _) in index.iter() {
            if !catalog.ids.contains(&db) {
                return Err(Error::Corrupt("snapshot-log orphan database rows"));
            }
        }
    }
    let encoded_bytes = STATE_HEADER
        .checked_add(catalog.names_bytes)
        .and_then(|n| n.checked_add(index.encoded_rows_bytes()))
        .ok_or(Error::LimitExceeded)?;
    if encoded_bytes > options.journal_capacity - 4096 {
        return Err(Error::LimitExceeded);
    }
    let reservation = index.reserve_metadata(snapshot_budget())?;
    Ok(Snapshot {
        index,
        catalog,
        next_database,
        sequence,
        encoded_bytes,
        _reservation: reservation,
        values: None,
        lookup: None,
    })
}

fn replay_versioned(
    base: Snapshot,
    catalog: Arc<Catalog>,
    next_database: u64,
    sequence: u64,
    changes: &[Change],
    had_drop: bool,
    pool: &mut PoolWriter,
    options: Options,
) -> Result<Snapshot> {
    let view = base
        .values
        .as_ref()
        .ok_or(Error::Corrupt("snapshot-log missing replay view"))?;
    let old_rows = base
        .encoded_bytes
        .checked_sub(STATE_HEADER + base.catalog.names_bytes)
        .ok_or(Error::Corrupt("snapshot-log replay logical bytes"))?;
    let plan = versioned::Plan::build(&base.index, view, changes, old_rows)?;
    let single = if plan.len() == 1 {
        Some(plan.mutation(0))
    } else {
        None
    };
    let mut many = Vec::new();
    if plan.len() > 1 {
        many.try_reserve_exact(plan.len())
            .map_err(|_| Error::LimitExceeded)?;
        for i in 0..plan.len() {
            many.push(plan.mutation(i));
        }
    }
    let mutations = if plan.len() > 1 {
        many.as_slice()
    } else {
        single.as_slice()
    };
    let ready = pool.prepare(sequence, mutations)?;
    let member_one = if plan.len() == 1 {
        plan.membership_one(ready.heads()[0])?
    } else {
        None
    };
    let member_many = if plan.len() > 1 {
        plan.membership(ready.heads())?
    } else {
        Vec::new()
    };
    let membership = if plan.len() > 1 {
        member_many.as_slice()
    } else {
        member_one.as_slice()
    };
    let index = versioned::prepare_membership(&base.index, membership, options.batch_threshold)?;
    if had_drop && index.iter().any(|(db, _, _)| !catalog.ids.contains(&db)) {
        return Err(Error::Corrupt("snapshot-log orphan versioned rows"));
    }
    let encoded_bytes = STATE_HEADER
        .checked_add(catalog.names_bytes)
        .and_then(|n| n.checked_add(plan.rows_bytes()))
        .ok_or(Error::LimitExceeded)?;
    if encoded_bytes > options.journal_capacity - 4096 {
        return Err(Error::LimitExceeded);
    }
    let reservation = index.reserve_metadata(snapshot_budget())?;
    let values = ready.publish();
    Ok(Snapshot {
        index,
        catalog,
        next_database,
        sequence,
        encoded_bytes,
        _reservation: reservation,
        values: Some(values),
        lookup: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ulmdb-snapshot-log-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn options() -> Options {
        Options {
            index_bytes: 4 << 20,
            journal_capacity: 16384,
            transaction_bytes: 8192,
            max_operations: 1024,
            max_databases: 32,
            batch_threshold: 16,
            versioned_values: false,
            exact_lookup: true,
        }
    }
    #[test]
    fn hybrid_journal_keeps_catalog_frames_and_reopens_data_across_control_commits() {
        for versioned_values in [false, true] {
            let p = path("hybrid-control-data");
            let o = Options {
                versioned_values,
                ..options()
            };
            let env = Environment::create(&p, o).unwrap();
            let mut tx = env.begin_write().unwrap();
            tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
            assert_eq!(tx.commit().unwrap(), 1);
            let catalog = env.stats().unwrap();
            assert_eq!(catalog.journal.packed_records, 0);
            if catalog.journal.format_version != 4 {
                // The packed producer requires persistent4KiB geometry.
                drop(env);
                std::fs::remove_dir_all(p).unwrap();
                continue;
            }
            let original = [7; 64];
            let replacement = [13; 64];
            let mut tx = env.begin_write().unwrap();
            tx.put(b"kv", b"key", &original).unwrap();
            assert_eq!(tx.commit().unwrap(), 2);
            let old = env.begin_read();
            let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
            let pointer = borrowed.as_ptr();
            assert_eq!(env.stats().unwrap().journal.packed_records, 1);

            let mut tx = env.begin_write().unwrap();
            tx.create_database(b"later", DatabaseKind::Kv).unwrap();
            assert_eq!(tx.commit().unwrap(), 3);
            let control = env.stats().unwrap();
            assert_eq!(control.journal.packed_records, 1);
            assert_eq!(control.journal.closed_pages, 1);

            let mut tx = env.begin_write().unwrap();
            tx.create_database(b"mixed", DatabaseKind::Kv).unwrap();
            tx.put(b"mixed", b"m", b"mixed-value").unwrap();
            assert_eq!(tx.commit().unwrap(), 4);
            let mut tx = env.begin_write().unwrap();
            tx.put(b"kv", b"key", &replacement).unwrap();
            assert_eq!(tx.commit().unwrap(), 5);
            let final_stats = env.stats().unwrap();
            assert_eq!(final_stats.journal.packed_records, 3);
            assert_eq!(final_stats.journal.packed_pages, 2);
            assert_eq!(final_stats.journal.reservation_calls, 0);
            assert_eq!(borrowed, original);
            assert_eq!(borrowed.as_ptr(), pointer);

            drop(env);
            let reopened = Environment::open(&p, o).unwrap();
            let read = reopened.begin_read();
            assert_eq!(
                read.get(b"kv", b"key").unwrap(),
                Some(replacement.as_slice())
            );
            assert_eq!(
                read.get(b"mixed", b"m").unwrap(),
                Some(b"mixed-value".as_slice())
            );
            assert!(read.kv(b"later").is_ok());
            assert_eq!(borrowed, original);
            assert_eq!(borrowed.as_ptr(), pointer);
            drop((read, reopened, old));
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    fn state_fixture(
        sequence: u64,
        next: u64,
        catalog: &[(&[u8], u64)],
        rows: &[(u64, &[u8], &[u8])],
    ) -> Vec<u8> {
        let mut bytes = STATE.to_vec();
        bytes.extend_from_slice(&sequence.to_le_bytes());
        bytes.extend_from_slice(&next.to_le_bytes());
        bytes.extend_from_slice(&(catalog.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(rows.len() as u64).to_le_bytes());
        for (name, id) in catalog {
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(name);
            bytes.extend_from_slice(&id.to_le_bytes());
        }
        for (db, key, value) in rows {
            bytes.extend_from_slice(&db.to_le_bytes());
            bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(key);
            bytes.extend_from_slice(value);
        }
        bytes
    }
    fn effect(db: u64, key: &[u8], value: Option<&[u8]>) -> Change {
        Change {
            database: db,
            key: Bytes::new(key).unwrap(),
            value: value.map(|value| Bytes::new(value).unwrap()),
        }
    }
    #[test]
    fn effects_v1_bytes_stay_exact_and_v2_distinguishes_empty_from_delete() {
        let metadata = [Metadata::Create(b"db".to_vec(), 1)];
        let changes = [effect(1, &[255, 0], Some(b"")), effect(1, b"", None)];
        let mut expected = EFFECTS.to_vec();
        expected.extend_from_slice(&129u64.to_le_bytes());
        expected.extend_from_slice(&2u64.to_le_bytes());
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.extend_from_slice(&2u32.to_le_bytes());
        expected.push(1);
        expected.extend_from_slice(&2u16.to_le_bytes());
        expected.extend_from_slice(b"db");
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(&2u32.to_le_bytes());
        expected.extend_from_slice(&0u32.to_le_bytes());
        expected.extend_from_slice(&[255, 0]);
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(&0u32.to_le_bytes());
        expected.extend_from_slice(&u32::MAX.to_le_bytes());
        let mut bytes = Vec::new();
        encode_effects(129, 2, &metadata, &changes, &mut bytes, options(), false).unwrap();
        assert_eq!(bytes, expected);
        encode_effects(129, 2, &metadata, &changes, &mut bytes, options(), true).unwrap();
        let mut expected = EFFECTS_V2.to_vec();
        expected.extend_from_slice(&[
            129, 1, 2, 1, 2, 1, 2, b'd', b'b', 1, 1, 2, 1, 255, 0, 1, 0, 0,
        ]);
        assert_eq!(bytes, expected);
    }
    #[test]
    fn both_effect_codecs_preserve_large_fields_namespaces_and_last_write_order() {
        for versioned_values in [false, true] {
            let o = Options {
                versioned_values,
                ..options()
            };
            let initial = state_fixture(128, 130, &[(b"old", 129)], &[(129, b"", b"previous")]);
            let long_key = [255; 129];
            let value = [3; 257];
            let metadata = [
                Metadata::Create(b"new".to_vec(), 130),
                Metadata::Drop(b"old".to_vec(), 129),
            ];
            let changes = [
                effect(129, b"", None),
                effect(130, &long_key, Some(b"")),
                effect(130, b"", Some(&value)),
                effect(130, b"gone", Some(b"temporary")),
                effect(130, b"gone", None),
                effect(130, b"dup", Some(b"first")),
                effect(130, b"dup", Some(b"last")),
                effect(130, b"dup", None),
                effect(130, b"dup", Some(b"")),
            ];
            let mut signatures = Vec::new();
            for compact in [false, true] {
                let (base, mut pool) = decode_initial(&initial, o).unwrap();
                let old_index = base.index.clone();
                let old_view = base.values.clone();
                let stored = old_index.get(129, b"").unwrap();
                let borrowed = match &old_view {
                    Some(view) => view.get(versioned::decode_handle(stored).unwrap()).unwrap(),
                    None => stored,
                };
                let mut bytes = Vec::new();
                encode_effects(129, 131, &metadata, &changes, &mut bytes, o, compact).unwrap();
                let next = apply_effects(base, &bytes, o, pool.as_mut()).unwrap();
                assert_eq!(next.sequence, 129);
                assert_eq!(next.next_database, 131);
                assert!(!next.catalog.contains_key(b"old".as_slice()));
                assert_eq!(next.catalog.get(b"new".as_slice()), Some(&130));
                assert_eq!(next.value(130, &long_key).unwrap(), Some(b"".as_slice()));
                assert_eq!(next.value(130, b"gone").unwrap(), None);
                assert_eq!(next.value(130, b"dup").unwrap(), Some(b"".as_slice()));
                assert_eq!(borrowed, b"previous");
                signatures.push(
                    next.index
                        .iter()
                        .map(|(db, key, stored)| {
                            (db, key.to_vec(), next.resolve(stored).unwrap().to_vec())
                        })
                        .collect::<Vec<_>>(),
                );
            }
            assert_eq!(signatures[0], signatures[1]);
        }
    }
    #[test]
    fn effects_v2_malformed_fields_and_old_decoded_budget_are_rejected() {
        let o = options();
        let initial = state_fixture(0, 2, &[(b"kv", 1)], &[]);
        let wire = |fields: &[u64]| {
            let mut bytes = EFFECTS_V2.to_vec();
            for &field in fields {
                effects::put(&mut bytes, field, o.transaction_bytes).unwrap();
            }
            bytes
        };
        let valid = wire(&[1, 2, 0, 1, 1, 0, 1]);
        let mut bad = vec![
            wire(&[1, 2, 0, o.max_operations as u64 + 1]),
            wire(&[1, 2, u64::MAX, 0]),
            wire(&[1, 2, 0, 1, 1, 0, u64::MAX]),
            wire(&[1, 2, 0, 1, 1, MAX_KEY as u64 + 1, 0]),
            wire(&[1, 2, 0, 1, 2, 0, 1]),
            wire(&[1, 3, 0, 0]),
            [EFFECTS_V2.as_slice(), &[128]].concat(),
            [EFFECTS_V2.as_slice(), &[129, 0], &valid[9..]].concat(),
            [EFFECTS_V2.as_slice(), &[255; 9], &[2]].concat(),
            [valid.as_slice(), &[0]].concat(),
        ];
        let mut wrong_type = valid.clone();
        wrong_type[0] ^= 1;
        bad.push(wrong_type);
        let mut wrong_meta = wire(&[1, 3, 1, 0]);
        wrong_meta.extend_from_slice(&[3, 1, b'x', 2]);
        bad.push(wrong_meta);
        for bytes in bad {
            let (base, _) = decode_initial(&initial, o).unwrap();
            assert!(
                apply_effects(base, &bytes, o, None).is_err(),
                "accepted malformed {bytes:?}"
            );
        }
        let small = Options {
            transaction_bytes: 40,
            ..o
        };
        let (base, _) = decode_initial(&initial, small).unwrap();
        assert!(valid.len() < small.transaction_bytes);
        assert!(apply_effects(base, &valid, small, None).is_err());
        let mut out = Vec::new();
        assert!(
            encode_effects(
                1,
                2,
                &[],
                &[effect(1, b"", Some(b""))],
                &mut out,
                small,
                true
            )
            .is_err()
        );
    }
    #[test]
    fn mixed_semantic_versions_reopen_rotate_and_keep_aborted_and_old_values() {
        for versioned_values in [false, true] {
            let p = path("effects-mixed");
            let o = Options {
                versioned_values,
                ..options()
            };
            let initial = state_fixture(0, 1, &[], &[]);
            let mut journal = Journal::create(&p, o.journal_capacity, &initial).unwrap();
            assert!(journal.uses_compact_frames());
            let mut bytes = Vec::new();
            encode_effects(
                1,
                2,
                &[Metadata::Create(b"kv".to_vec(), 1)],
                &[
                    effect(1, b"", Some(b"")),
                    effect(1, b"x", Some(b"original")),
                ],
                &mut bytes,
                o,
                false,
            )
            .unwrap();
            journal.append(&bytes).unwrap();
            drop(journal);
            let env = Environment::open(&p, o).unwrap();
            let old = env.begin_read();
            let borrowed = old.get(b"kv", b"x").unwrap().unwrap();
            drop(env);
            let (mut journal, _, frames) = Journal::open(&p, o.journal_capacity).unwrap();
            assert_eq!(&frames[0][..8], EFFECTS);
            encode_effects(
                2,
                2,
                &[],
                &[
                    effect(1, b"", None),
                    effect(1, b"x", Some(b"updated")),
                    effect(1, b"new", Some(b"")),
                ],
                &mut bytes,
                o,
                true,
            )
            .unwrap();
            journal.append(&bytes).unwrap();
            drop(journal);
            let env = Environment::open(&p, o).unwrap();
            assert_eq!(env.begin_read().get(b"kv", b"").unwrap(), None);
            assert_eq!(
                env.begin_read().get(b"kv", b"new").unwrap(),
                Some(b"".as_slice())
            );
            {
                let mut tx = env.begin_write().unwrap();
                tx.put(b"kv", b"x", b"discarded").unwrap();
                tx.abort();
            }
            assert_eq!(
                env.begin_read().get(b"kv", b"x").unwrap(),
                Some(b"updated".as_slice())
            );
            let mut state = Vec::new();
            encode_state(&env.current.load(), &mut state, o).unwrap();
            assert_eq!(&state[..8], STATE);
            drop(env);
            let (mut journal, _, frames) = Journal::open(&p, o.journal_capacity).unwrap();
            assert_eq!(&frames[0][..8], EFFECTS);
            assert_eq!(&frames[1][..8], EFFECTS_V2);
            journal.checkpoint(&state).unwrap();
            encode_effects(
                3,
                2,
                &[],
                &[effect(1, b"x", Some(b"after rotation"))],
                &mut bytes,
                o,
                false,
            )
            .unwrap();
            journal.append(&bytes).unwrap();
            drop(journal);
            let env = Environment::open(&p, o).unwrap();
            assert_eq!(env.stats().unwrap().sequence, 3);
            assert_eq!(
                env.begin_read().get(b"kv", b"x").unwrap(),
                Some(b"after rotation".as_slice())
            );
            assert_eq!(borrowed, b"original");
            drop(env);
            drop(old);
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn actual_transactions_borrowed_snapshots_abort_cursor_and_reopen() {
        for versioned_values in [false, true] {
            let p = path("native");
            let o = Options {
                versioned_values,
                ..options()
            };
            let env = Environment::create(&p, o).unwrap();
            let mut tx = env.begin_write().unwrap();
            tx.create_database(b"a", DatabaseKind::Kv).unwrap();
            tx.create_database(b"b", DatabaseKind::Kv).unwrap();
            tx.put(b"a", b"", b"zero").unwrap();
            tx.put(b"a", b"a", b"original").unwrap();
            tx.put(b"b", b"x", &[0xa5; 257]).unwrap();
            assert_eq!(tx.get(b"a", b"a").unwrap(), Some(b"original".as_slice()));
            assert_eq!(tx.commit().unwrap(), 1);
            let old = env.begin_read();
            let view = old.kv(b"a").unwrap();
            let borrowed = view.get(b"a").unwrap().unwrap();
            let pointer = borrowed.as_ptr();
            {
                let mut tx = env.begin_write().unwrap();
                tx.put(b"a", b"a", b"discarded").unwrap();
                tx.abort();
            }
            assert_eq!(
                env.begin_read().get(b"a", b"a").unwrap(),
                Some(b"original".as_slice())
            );
            let mut tx = env.begin_write().unwrap();
            tx.put(b"a", b"a", b"next").unwrap();
            tx.put(b"a", b"b", b"second").unwrap();
            tx.delete(b"a", b"").unwrap();
            tx.put(b"b", b"x", b"also-next").unwrap();
            tx.commit().unwrap();
            assert_eq!(borrowed, b"original");
            assert_eq!(borrowed.as_ptr(), pointer);
            assert_eq!(view.get(b"").unwrap(), Some(b"zero".as_slice()));
            let now = env.begin_read();
            let rows: Vec<_> = now
                .kv(b"a")
                .unwrap()
                .cursor(Bound::Included(b"a"), Bound::Included(b"b"), true)
                .unwrap()
                .map(|x| x.unwrap().0.to_vec())
                .collect();
            assert_eq!(rows, vec![b"b".to_vec(), b"a".to_vec()]);
            drop(env);
            let reopened = Environment::open(&p, o).unwrap();
            assert_eq!(
                reopened.begin_read().get(b"a", b"a").unwrap(),
                Some(b"next".as_slice())
            );
            assert_eq!(
                reopened.begin_read().get(b"b", b"x").unwrap(),
                Some(b"also-next".as_slice())
            );
            assert_eq!(borrowed, b"original");
            drop(reopened);
            drop(now);
            drop(old);
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn journal_reuses_banks_while_old_index_roots_survive() {
        for versioned_values in [false, true] {
            let p = path("bounded");
            let o = Options {
                versioned_values,
                ..options()
            };
            let env = Environment::create(&p, o).unwrap();
            let mut tx = env.begin_write().unwrap();
            tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
            for i in 0u64..32 {
                tx.put(b"kv", &i.to_be_bytes(), &0u64.to_be_bytes())
                    .unwrap();
            }
            tx.commit().unwrap();
            let initial = env.begin_read();
            let mut pins = Vec::new();
            for n in 1u64..=400 {
                let mut tx = env.begin_write().unwrap();
                for i in 0u64..8 {
                    tx.put(b"kv", &i.to_be_bytes(), &n.to_be_bytes()).unwrap();
                }
                tx.commit().unwrap();
                if n % 10 == 0 {
                    pins.push((n, env.begin_read()));
                }
            }
            assert_eq!(
                initial.get(b"kv", &0u64.to_be_bytes()).unwrap(),
                Some(0u64.to_be_bytes().as_slice())
            );
            for (n, pin) in &pins {
                assert_eq!(
                    pin.get(b"kv", &0u64.to_be_bytes()).unwrap(),
                    Some(n.to_be_bytes().as_slice())
                );
            }
            let before = env.stats().unwrap();
            assert!(before.journal.checkpoints >= 3);
            assert!(before.index.charged_bytes <= o.index_bytes);
            drop(pins);
            drop(initial);
            let after = env.stats().unwrap();
            assert!(after.index.charged_bytes < before.index.charged_bytes);
            drop(env);
            let reopened = Environment::open(&p, o).unwrap();
            assert_eq!(
                reopened
                    .begin_read()
                    .get(b"kv", &0u64.to_be_bytes())
                    .unwrap(),
                Some(400u64.to_be_bytes().as_slice())
            );
            drop(reopened);
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn limits_fail_before_publication_and_failed_transaction_cannot_commit() {
        for versioned_values in [false, true] {
            let p = path("limits");
            let o = Options {
                versioned_values,
                ..options()
            };
            let env = Environment::create(&p, o).unwrap();
            let mut tx = env.begin_write().unwrap();
            tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
            tx.put(b"kv", b"a", b"old").unwrap();
            tx.commit().unwrap();
            let before = env.stats().unwrap();
            let mut tx = env.begin_write().unwrap();
            tx.put(b"kv", b"a", b"new").unwrap();
            assert!(matches!(
                tx.put(b"kv", b"big", &vec![0; 9000]),
                Err(Error::LimitExceeded)
            ));
            assert!(tx.commit().is_err());
            let after = env.stats().unwrap();
            assert_eq!(after.sequence, before.sequence);
            assert_eq!(after.journal.append_frames, before.journal.append_frames);
            assert_eq!(
                env.begin_read().get(b"kv", b"a").unwrap(),
                Some(b"old".as_slice())
            );
            drop(env);
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn drop_and_recreate_database_has_distinct_identity_and_replays() {
        for versioned_values in [false, true] {
            let p = path("catalog");
            let o = Options {
                versioned_values,
                ..options()
            };
            let env = Environment::create(&p, o).unwrap();
            let mut tx = env.begin_write().unwrap();
            tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
            tx.put(b"kv", b"old", b"v").unwrap();
            tx.commit().unwrap();
            let old = env.begin_read();
            let mut tx = env.begin_write().unwrap();
            tx.put(b"kv", b"pending", b"gone").unwrap();
            tx.drop_database(b"kv").unwrap();
            tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
            tx.put(b"kv", b"new", b"kept").unwrap();
            tx.commit().unwrap();
            assert_eq!(old.get(b"kv", b"old").unwrap(), Some(b"v".as_slice()));
            assert!(env.begin_read().get(b"kv", b"old").unwrap().is_none());
            drop(env);
            let reopened = Environment::open(&p, o).unwrap();
            assert_eq!(
                reopened.begin_read().get(b"kv", b"new").unwrap(),
                Some(b"kept".as_slice())
            );
            assert!(
                reopened
                    .begin_read()
                    .get(b"kv", b"pending")
                    .unwrap()
                    .is_none()
            );
            drop(reopened);
            drop(old);
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn metadata_only_retained_snapshots_consume_the_shared_quota() {
        for versioned_values in [false, true] {
            let p = path("metadata-quota");
            let mut o = Options {
                versioned_values,
                ..options()
            };
            o.index_bytes = 64 << 10;
            let env = Environment::create(&p, o).unwrap();
            let mut pins = Vec::new();
            let mut refused = false;
            for _ in 0..1000 {
                let before = env.stats().unwrap().sequence;
                let exists = env.begin_read().kv(b"schema").is_ok();
                let mut tx = env.begin_write().unwrap();
                let edited = if exists {
                    tx.drop_database(b"schema")
                } else {
                    tx.create_database(b"schema", DatabaseKind::Kv)
                };
                match edited.and_then(|_| tx.commit()) {
                    Ok(_) => pins.push(env.begin_read()),
                    Err(Error::LimitExceeded) => {
                        assert_eq!(env.stats().unwrap().sequence, before);
                        refused = true;
                        break;
                    }
                    Err(e) => panic!("unexpected metadata transaction failure: {e:?}"),
                }
            }
            assert!(refused, "retained metadata must not bypass the index quota");
            let peak = env.stats().unwrap().index.charged_bytes;
            drop(pins);
            assert!(env.stats().unwrap().index.charged_bytes < peak);
            let exists = env.begin_read().kv(b"schema").is_ok();
            let mut tx = env.begin_write().unwrap();
            if exists {
                tx.drop_database(b"schema").unwrap();
            } else {
                tx.create_database(b"schema", DatabaseKind::Kv).unwrap();
            }
            tx.commit().unwrap();
            drop(env);
            let reopened = Environment::open(&p, o).unwrap();
            drop(reopened);
            std::fs::remove_dir_all(p).unwrap();
        }
    }
    #[test]
    fn both_edit_routes_preserve_noops_and_read_your_writes() {
        for versioned_values in [false, true] {
            for threshold in [1, usize::MAX] {
                let p = path("hybrid-semantics");
                let mut o = Options {
                    versioned_values,
                    ..options()
                };
                o.batch_threshold = threshold;
                let env = Environment::create(&p, o).unwrap();
                let mut tx = env.begin_write().unwrap();
                tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
                tx.put(b"kv", b"a", b"same").unwrap();
                let seq = tx.commit().unwrap();
                let before = env.stats().unwrap().journal.append_frames;
                let mut tx = env.begin_write().unwrap();
                tx.put(b"kv", b"a", b"same").unwrap();
                assert_eq!(tx.commit().unwrap(), seq);
                assert_eq!(env.stats().unwrap().journal.append_frames, before);
                let mut restored = env.begin_write().unwrap();
                restored.put(b"kv", b"a", b"temporary").unwrap();
                restored.put(b"kv", b"a", b"same").unwrap();
                assert_eq!(restored.commit().unwrap(), seq + 1);
                assert_eq!(env.stats().unwrap().journal.append_frames, before + 1);
                let old = env.begin_read();
                let mut tx = env.begin_write().unwrap();
                tx.put(b"kv", b"a", b"one").unwrap();
                tx.put(b"kv", b"a", b"two").unwrap();
                assert_eq!(tx.get(b"kv", b"a").unwrap(), Some(b"two".as_slice()));
                tx.delete(b"kv", b"a").unwrap();
                assert!(tx.get(b"kv", b"a").unwrap().is_none());
                tx.put(b"kv", b"a", b"last").unwrap();
                tx.commit().unwrap();
                assert_eq!(old.get(b"kv", b"a").unwrap(), Some(b"same".as_slice()));
                drop(env);
                let reopened = Environment::open(&p, o).unwrap();
                assert_eq!(
                    reopened.begin_read().get(b"kv", b"a").unwrap(),
                    Some(b"last".as_slice())
                );
                drop(reopened);
                drop(old);
                std::fs::remove_dir_all(p).unwrap();
            }
        }
    }

    #[test]
    fn versioned_replacements_share_key_root_and_reclaim_after_readers() {
        let p = path("versioned-roots");
        let o = Options {
            versioned_values: true,
            ..options()
        };
        let env = Environment::create(&p, o).unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
        tx.put(b"kv", b"key", b"first").unwrap();
        tx.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
        let pointer = borrowed.as_ptr();
        let original_head = versioned::head(&old.snapshot.index, 1, b"key").unwrap();
        for n in 1u64..=300 {
            let mut tx = env.begin_write().unwrap();
            tx.put(b"kv", b"key", &n.to_be_bytes()).unwrap();
            tx.commit().unwrap();
            let now = env.current.load_full();
            assert!(now.index.same_state(&old.snapshot.index));
            assert_eq!(
                versioned::head(&now.index, 1, b"key").unwrap(),
                original_head
            );
        }
        assert_eq!(borrowed, b"first");
        assert_eq!(borrowed.as_ptr(), pointer);
        let before = env.stats().unwrap().versions.unwrap().allocated_versions;
        assert!(before >= 301);
        drop(old);
        env.collect_runtime(usize::MAX).unwrap();
        assert!(env.stats().unwrap().versions.unwrap().allocated_versions < before);
        let previous = env.begin_read();
        let mut tx = env.begin_write().unwrap();
        tx.delete(b"kv", b"key").unwrap();
        tx.commit().unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.put(b"kv", b"key", b"reborn").unwrap();
        tx.commit().unwrap();
        assert_ne!(
            versioned::head(&env.current.load().index, 1, b"key").unwrap(),
            original_head
        );
        assert_eq!(
            previous.get(b"kv", b"key").unwrap(),
            Some(300u64.to_be_bytes().as_slice())
        );
        drop(previous);
        drop(env);
        // Checkpoints/effects encode logical bytes, so reopening may choose
        // either in-memory representation without changing the file format.
        let plain = Environment::open(
            &p,
            Options {
                versioned_values: false,
                ..o
            },
        )
        .unwrap();
        assert_eq!(
            plain.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"reborn".as_slice())
        );
        drop(plain);
        let versioned = Environment::open(&p, o).unwrap();
        assert_eq!(
            versioned.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"reborn".as_slice())
        );
        drop(versioned);
        std::fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn versioned_value_quota_refuses_before_io_and_recovers_after_gc() {
        let p = path("versioned-quota");
        let o = Options {
            versioned_values: true,
            index_bytes: 64 << 10,
            ..options()
        };
        let env = Environment::create(&p, o).unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
        tx.put(b"kv", b"key", &[0; 2048]).unwrap();
        tx.commit().unwrap();
        let pin = env.begin_read();
        let mut refused = false;
        for n in 1u8..100 {
            let before = env.stats().unwrap();
            let mut tx = env.begin_write().unwrap();
            tx.put(b"kv", b"key", &[n; 2048]).unwrap();
            match tx.commit() {
                Ok(_) => (),
                Err(Error::LimitExceeded) => {
                    let after = env.stats().unwrap();
                    assert_eq!(after.sequence, before.sequence);
                    assert_eq!(after.journal.append_frames, before.journal.append_frames);
                    refused = true;
                    break;
                }
                Err(e) => panic!("unexpected versioned quota result: {e:?}"),
            }
        }
        assert!(refused);
        assert_eq!(pin.get(b"kv", b"key").unwrap(), Some([0; 2048].as_slice()));
        drop(pin);
        env.collect_runtime(usize::MAX).unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.put(b"kv", b"key", &[255; 2048]).unwrap();
        tx.commit().unwrap();
        drop(env);
        let reopened = Environment::open(&p, o).unwrap();
        assert_eq!(
            reopened.begin_read().get(b"kv", b"key").unwrap(),
            Some([255; 2048].as_slice())
        );
        drop(reopened);
        std::fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn derived_lookup_releases_capacity_for_authoritative_write_and_old_reader() {
        let p = path("lookup-priority");
        let o = Options {
            versioned_values: true,
            index_bytes: 2 << 20,
            ..options()
        };
        let env = Environment::create(&p, o).unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for n in 0u64..64 {
            tx.put(b"kv", &n.to_be_bytes(), &[7; 64]).unwrap();
        }
        tx.commit().unwrap();
        let old = env.begin_read();
        assert!(env.stats().unwrap().exact_lookup);
        let borrowed = old.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
        let pointer = borrowed.as_ptr();
        let stats = env.stats().unwrap();
        let pressure = old
            .snapshot
            .index
            .reserve_metadata(
                stats.index.limit_bytes - stats.index.charged_bytes - snapshot_budget() + 1,
            )
            .unwrap();
        // Preparing the new Snapshot initially exceeds the limit. Revoking
        // the derived index must recover its node capacity before the write.
        let mut tx = env.begin_write().unwrap();
        tx.put(b"kv", &0u64.to_be_bytes(), &[9; 64]).unwrap();
        tx.commit().unwrap();
        assert!(!env.stats().unwrap().exact_lookup);
        assert_eq!(borrowed, &[7; 64]);
        assert_eq!(borrowed.as_ptr(), pointer);
        assert_eq!(
            old.get(b"kv", &0u64.to_be_bytes()).unwrap(),
            Some([7; 64].as_slice())
        );
        assert_eq!(
            env.begin_read().get(b"kv", &0u64.to_be_bytes()).unwrap(),
            Some([9; 64].as_slice())
        );
        drop(pressure);
        drop(old);
        drop(env);
        let reopened = Environment::open(&p, o).unwrap();
        assert!(reopened.stats().unwrap().exact_lookup);
        assert_eq!(
            reopened
                .begin_read()
                .get(b"kv", &0u64.to_be_bytes())
                .unwrap(),
            Some([9; 64].as_slice())
        );
        drop(reopened);
        std::fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn scan_cache_preserves_returned_borrows_across_advance_and_commit() {
        let p = path("scan-cache-borrows");
        let o = Options {
            versioned_values: true,
            journal_capacity: 128 << 10,
            transaction_bytes: 64 << 10,
            ..options()
        };
        let env = Environment::create(&p, o).unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for n in 0u64..300 {
            tx.put(b"kv", &n.to_be_bytes(), &[n as u8; 64]).unwrap();
        }
        tx.commit().unwrap();
        let old = env.begin_read();
        let view = old.kv(b"kv").unwrap();
        let mut cursor = view
            .cursor(
                Bound::Included(&10u64.to_be_bytes()),
                Bound::Excluded(&299u64.to_be_bytes()),
                false,
            )
            .unwrap();
        let first = cursor.next().unwrap().unwrap().1;
        let pointer = first.as_ptr();
        for n in 11u64..299 {
            let (key, value) = cursor.next().unwrap().unwrap();
            assert_eq!(key, n.to_be_bytes());
            assert_eq!(value, &[n as u8; 64]);
        }
        assert!(cursor.next().is_none());
        let mut tx = env.begin_write().unwrap();
        tx.put(b"kv", &10u64.to_be_bytes(), &[255; 64]).unwrap();
        tx.commit().unwrap();
        assert_eq!(first, &[10; 64]);
        assert_eq!(first.as_ptr(), pointer);
        drop(cursor);
        drop(old);
        drop(env);
        std::fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn bulk_hash_base_publishes_with_values_and_preserves_prior_snapshot() {
        let p = path("bulk-hash-base");
        let o = Options {
            versioned_values: true,
            index_bytes: 16 << 20,
            journal_capacity: 2 << 20,
            transaction_bytes: 1 << 20,
            max_operations: 8192,
            ..options()
        };
        let env = Environment::create(&p, o).unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.create_database(b"kv", DatabaseKind::Kv).unwrap();
        tx.commit().unwrap();
        let empty = env.begin_read();
        let mut tx = env.begin_write().unwrap();
        for n in 0u64..4096 {
            tx.put(b"kv", &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
        }
        tx.commit().unwrap();
        assert!(empty.get(b"kv", &0u64.to_be_bytes()).unwrap().is_none());
        let before = env.begin_read();
        let stats = env.stats().unwrap();
        assert!(stats.exact_lookup);
        assert_eq!(stats.lookup.unwrap().base_entries, 4096);
        assert_eq!(stats.lookup.unwrap().delta_nodes, 0);
        let borrowed = before.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
        let mut tx = env.begin_write().unwrap();
        tx.put(b"kv", &0u64.to_be_bytes(), &99u64.to_le_bytes())
            .unwrap();
        tx.commit().unwrap();
        let after = env.begin_read();
        assert!(Arc::ptr_eq(
            before.snapshot.lookup.as_ref().unwrap(),
            after.snapshot.lookup.as_ref().unwrap()
        ));
        assert_eq!(borrowed, &0u64.to_le_bytes());
        assert_eq!(
            after.get(b"kv", &0u64.to_be_bytes()).unwrap(),
            Some(99u64.to_le_bytes().as_slice())
        );
        drop(empty);
        drop(before);
        drop(after);
        drop(env);
        let reopened = Environment::open(&p, o).unwrap();
        assert!(reopened.stats().unwrap().exact_lookup);
        assert_eq!(
            reopened
                .begin_read()
                .get(b"kv", &0u64.to_be_bytes())
                .unwrap(),
            Some(99u64.to_le_bytes().as_slice())
        );
        drop(reopened);
        std::fs::remove_dir_all(p).unwrap();
    }
}
