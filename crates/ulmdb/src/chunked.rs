//! Opt-in bounded decoded-window reader for authenticated file snapshots.
//! The ordinary raw mapping path is unchanged. Pins last until transaction drop;
//! a fully pinned cache fails LimitExceeded instead of invalidating a borrow.
use crate::{
    Address, DatabaseKind, Error, Prefix, Result,
    catalog::Database,
    ordering::{BoundComparator, DatabaseComparators, OrderingSpec},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    path::Path,
    sync::Arc,
};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_storage::{
    Manifest,
    chunk_cache::ChunkCache,
    chunk_table::{ChunkedSegmentTable, PinLimits},
    coordination::PhysicalPin,
};
pub(crate) mod admission;
pub(crate) mod stream;
pub use admission::AdmissionLimits;
pub use ulmdb_storage::chunk_table::PinLimits as ChunkPinLimits;
#[derive(Clone)]
pub struct CacheOptions {
    pub cache: Arc<ChunkCache>,
    pub pins: PinLimits,
    pub admission: AdmissionLimits,
}
impl CacheOptions {
    pub(crate) fn from_table(table: &ulmdb_storage::SegmentTable) -> Option<Self> {
        let cache = table.chunk_cache()?.clone();
        let pins = table.chunk_pin_limits()?;
        let admission = table
            .chunk_budget()
            .map(|(reads, nodes, metadata_bytes)| AdmissionLimits {
                reads,
                nodes,
                metadata_bytes,
            })
            .unwrap_or_default();
        Some(Self {
            cache,
            pins,
            admission,
        })
    }
    pub fn new(cache: Arc<ChunkCache>) -> Self {
        Self {
            cache,
            pins: Default::default(),
            admission: Default::default(),
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.pins.validate()?;
        self.admission.validate()
    }
}
pub struct ChunkedReadTxn {
    table: ChunkedSegmentTable,
    manifest: Manifest,
    catalog: BTreeMap<Vec<u8>, Database>,
    _pin: PhysicalPin,
}
impl ChunkedReadTxn {
    pub fn open_coordinated_snapshot(
        path: impl AsRef<Path>,
        cache: Arc<ChunkCache>,
    ) -> Result<Self> {
        Self::open_with_options(
            path,
            cache,
            PinLimits::default(),
            AdmissionLimits::default(),
            None,
        )
    }
    pub fn open_with_options(
        path: impl AsRef<Path>,
        cache: Arc<ChunkCache>,
        pins: PinLimits,
        admission: AdmissionLimits,
        context: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        admission.validate()?;
        pins.validate()?;
        let path = path.as_ref().canonicalize()?;
        let capture = ulmdb_storage::coordination::Coordination::capture_directory_with_transform(
            &path,
            Default::default(),
            context.clone(),
        )?;
        let capture = crate::OwnedFileCapture::current(&path, capture)?;
        capture.admit_chunked(cache, pins, admission, context)
    }
    pub(crate) fn from_pin(
        directory: &Path,
        pin: PhysicalPin,
        cache: Arc<ChunkCache>,
        pins: PinLimits,
        admission: AdmissionLimits,
        context: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        admission.validate()?;
        pins.validate()?;
        if !pin.matches_directory(directory)? {
            return Err(Error::InvalidInput("chunk snapshot pin namespace"));
        }
        let manifest = pin
            .root()
            .manifest()
            .clone()
            .with_transform_context(context.clone());
        let configured =
            ulmdb_storage::transform::check_configuration(directory, context.as_ref())?;
        if configured.is_some_and(|id| id != manifest.uuid) {
            return Err(Error::CryptoMismatch);
        }
        // Check the lazy inventory count before a slice coercion can build
        // its dense metadata representation. No source fd/hash work on excess.
        if manifest.segments.len() > pins.sources {
            return Err(Error::LimitExceeded);
        }
        let table = ChunkedSegmentTable::load(
            directory,
            &manifest.segments,
            cache,
            pins,
            manifest.page_size,
            manifest.value_limit,
            context,
        )?;
        let catalog = admission::admit(&table, &manifest, admission)?;
        if !pin.matches_directory(directory)? {
            return Err(Error::Corrupt("chunk snapshot namespace changed"));
        }
        Ok(Self {
            table,
            manifest,
            catalog,
            _pin: pin,
        })
    }
    pub fn sequence(&self) -> u64 {
        self.manifest.sequence
    }
    pub fn commit_identity(&self) -> u128 {
        self.manifest.commit
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn database_kind(&self, name: &[u8]) -> Result<DatabaseKind> {
        Ok(self.database(name)?.kind)
    }
    pub fn cache_stats(&self) -> Result<ulmdb_storage::chunk_cache::Stats> {
        self.table.cache().stats()
    }
    pub fn pinned_requests(&self) -> Result<usize> {
        self.table.pinned_requests()
    }
    fn database(&self, name: &[u8]) -> Result<&Database> {
        self.catalog.get(name).ok_or(Error::MissingDatabase)
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.get_with_ordering(name, key, &Default::default())
    }
    pub fn get_with_ordering(
        &self,
        name: &[u8],
        key: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<Option<&[u8]>> {
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if key.len() > self.table.max_key_size() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        ulmdb_kv::get_with(
            &self.table,
            db.root,
            key,
            &BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?,
        )
    }
    pub fn range<'a>(
        &'a self,
        name: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<ulmdb_kv::Cursor<'a, ChunkedSegmentTable, BoundComparator>> {
        self.range_with_ordering(name, lower, upper, reverse, &Default::default())
    }
    pub fn range_with_ordering<'a>(
        &'a self,
        name: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
        callbacks: &DatabaseComparators,
    ) -> Result<ulmdb_kv::Cursor<'a, ChunkedSegmentTable, BoundComparator>> {
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        ulmdb_kv::Cursor::with_order(
            &self.table,
            db.root,
            lower,
            upper,
            reverse,
            BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?,
        )
    }
    pub fn longest_match(&self, name: &[u8], address: Address) -> Result<Option<(Prefix, &[u8])>> {
        let db = self.database(name)?;
        crate::read::check_family(db.kind, address)?;
        ulmdb_prefix::get(&self.table, db.root, address)
    }
    pub fn exact_prefix(&self, name: &[u8], prefix: Prefix) -> Result<Option<(Prefix, &[u8])>> {
        let db = self.database(name)?;
        crate::read::check_family(db.kind, prefix.network())?;
        ulmdb_prefix::exact(&self.table, db.root, prefix)
    }
    pub fn duplicates(
        &self,
        name: &[u8],
    ) -> Result<crate::DuplicatesRead<'_, ChunkedSegmentTable>> {
        self.duplicates_with_ordering(name, &Default::default())
    }
    pub fn duplicates_with_ordering(
        &self,
        name: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<crate::DuplicatesRead<'_, ChunkedSegmentTable>> {
        let db = self.database(name)?;
        crate::DuplicatesRead::chunked_order(
            &self.table,
            db.root,
            db.kind,
            BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?,
            BoundComparator::bind(db.ordering.duplicates, callbacks.duplicates.as_ref())?,
        )
    }
}
