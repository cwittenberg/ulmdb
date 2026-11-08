use super::{ReadTxn, RedoEncoding, RedoFormat, Snapshot, WriteTxn, journal::Journal};
use crate::{Error, Result};
use arc_swap::ArcSwap;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use ulmdb_storage::resident::{Config, Pool, Stats, wal_codec::Boot};
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Fixed anonymous maps. Checkpointed owners reuse retired Tiny maps only
    /// after the final reader releases them, advancing the slot generation.
    pub pool: Config,
    pub wal_capacity: u64,
    /// Read allowance per live Collect; cumulative across historical retirement
    /// audits plus the selected graph on cold open. A longer valid history may
    /// need reopening with a larger allowance; cold audits are never skipped.
    /// With the opt-in prefix cache, this counts full-traversal reads; generation
    /// and empty-source probes use CollectCacheLimits::max_validations instead.
    pub graph_read_budget: u64,
    pub auto_collect_below: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            pool: Config {
                kv_pages: 512,
                tiny_pages: 256,
                max_reserved_bytes: 256 * 1024 * 1024,
                max_metadata_bytes: 32 * 1024 * 1024,
                max_views: 128,
            },
            wal_capacity: 64 * 1024 * 1024,
            graph_read_budget: 16_000_000,
            auto_collect_below: 128,
        }
    }
}
pub struct Environment {
    pub(super) current: ArcSwap<Snapshot>,
    pub(super) writer: Mutex<Writer>,
    pub(super) pool: Arc<Pool>,
    pub(super) options: Options,
    _fence: ulmdb_storage::WriterFence,
    directory: PathBuf,
    pub(super) wal_capacity: AtomicU64,
    creator: u32,
    redo_format: RedoFormat,
    redo_encoding: RedoEncoding,
}
pub(super) struct Writer {
    #[cfg(feature = "resident-prefix-audit-cache")]
    pub prefix_audit_cache: Option<super::collect_cache::Cache>,
    pub journal: Journal,
    pub poisoned: bool,
    pub collections: u64,
    pub recipe_workspace: ulmdb_storage::resident::wal_codec_v2::RecipeWorkspace,
    pub incremental: Option<super::graph_certificate::Certificate>,
    pub incremental_limits: Option<super::IncrementalLimits>,
    pub incremental_metrics: super::IncrementalMetrics,
    #[cfg(feature = "resident-private-index")]
    pub private_cache: Arc<ulmdb_storage::resident::PrivateCache>,
}
impl Environment {
    /// Actual compiled KV comparison strategy; observe outside serving queries.
    pub const fn word_search_enabled(&self) -> bool {
        ulmdb_kv::word_search_enabled()
    }
    /// Actual compiled cold/Collect audit strategy, outside serving queries.
    pub const fn single_pass_audit_enabled(&self) -> bool {
        cfg!(feature = "resident-single-pass-audit")
    }
    /// Observe the compiled storage implementation outside measured operations.
    pub const fn direct_page_patch_enabled(&self) -> bool {
        Pool::direct_page_patch_enabled()
    }
    pub const fn scalar_prefix_enabled(&self) -> bool {
        ulmdb_prefix::scalar_family_enabled()
    }

    pub const fn private_workspace_enabled(&self) -> bool {
        cfg!(feature = "resident-private-index")
    }
    /// Construction-time admitted capacities plus current aggregate quota charge.
    /// Capacity hints are not live ownership/RSS; a poisoned lease flag can stay
    /// set after buffers drop. Call outside a held WriteTxn.
    pub fn private_workspace_stats(
        &self,
    ) -> Result<ulmdb_storage::resident::PrivateWorkspaceStats> {
        #[cfg(feature = "resident-private-index")]
        {
            self.writer
                .lock()
                .map_err(|_| Error::Busy)?
                .private_cache
                .stats()
        }
        #[cfg(not(feature = "resident-private-index"))]
        {
            Ok(Default::default())
        }
    }

    pub fn create(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        Self::create_with_redo(path, options, RedoFormat::LiteralV1)
    }
    pub fn create_with_redo(
        path: impl AsRef<Path>,
        options: Options,
        redo_format: RedoFormat,
    ) -> Result<Self> {
        Self::create_with_encoding(path, options, redo_format, RedoEncoding::Owned)
    }
    pub fn create_with_encoding(
        path: impl AsRef<Path>,
        options: Options,
        redo_format: RedoFormat,
        redo_encoding: RedoEncoding,
    ) -> Result<Self> {
        Self::create_with_encoding_and_transform(path, options, redo_format, redo_encoding, None)
    }
    pub fn create_with_transform(
        path: impl AsRef<Path>,
        options: Options,
        context: Arc<ulmdb_storage::transform::TransformContext>,
    ) -> Result<Self> {
        Self::create_with_encoding_and_transform(
            path,
            options,
            RedoFormat::LiteralV1,
            RedoEncoding::Owned,
            Some(context),
        )
    }
    pub fn create_with_encoding_and_transform(
        path: impl AsRef<Path>,
        options: Options,
        redo_format: RedoFormat,
        redo_encoding: RedoEncoding,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        if redo_format == RedoFormat::LiteralV1 && redo_encoding != RedoEncoding::Owned {
            return Err(Error::InvalidInput("borrowed encoding requires RL2"));
        }
        let directory = path.as_ref();
        validate_options(options)?;
        ulmdb_storage::create_layout(directory)?;
        let fence = crate::Environment::lock(directory)?;
        reject_foreign(directory)?;
        let boot = Boot {
            uuid: ulmdb_storage::new_identity()?.to_le_bytes(),
            epoch: 1,
            commit: ulmdb_storage::new_identity()?,
            kv_pages: options.pool.kv_pages as u32,
            tiny_pages: options.pool.tiny_pages as u32,
        };
        ulmdb_storage::transform::create_configuration(directory, boot.uuid, transform.as_ref())?;
        let pool = Pool::create_with_transform(options.pool, transform.clone())?;
        let image = pool.empty()?;
        #[cfg(feature = "resident-private-index")]
        let private_cache = ulmdb_storage::resident::PrivateCache::create(&pool)?;
        let journal = Journal::create_with_transform(
            &directory.join("RESIDENT.wal"),
            boot,
            options.wal_capacity,
            redo_format,
            transform,
        )?;
        let snapshot = Snapshot {
            image,
            catalog: Default::default(),
            root: journal.state().root(),
            next_database: journal.state().next_database(),
            receipt: journal.state().selected(),
            commit: journal.state().commit(),
        };
        Ok(Self {
            current: ArcSwap::from_pointee(snapshot),
            writer: Mutex::new(Writer {
                #[cfg(feature = "resident-prefix-audit-cache")]
                prefix_audit_cache: None,
                journal,
                poisoned: false,
                collections: 0,
                recipe_workspace: Default::default(),
                incremental: None,
                incremental_limits: None,
                incremental_metrics: Default::default(),
                #[cfg(feature = "resident-private-index")]
                private_cache,
            }),
            pool,
            options,
            _fence: fence,
            directory: directory.to_path_buf(),
            wal_capacity: AtomicU64::new(options.wal_capacity),
            creator: std::process::id(),
            redo_format,
            redo_encoding,
        })
    }
    pub fn open(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        Self::open_inner(path.as_ref(), options, None, RedoEncoding::Owned)
    }
    pub fn open_with_redo(
        path: impl AsRef<Path>,
        options: Options,
        format: RedoFormat,
    ) -> Result<Self> {
        Self::open_inner(path.as_ref(), options, Some(format), RedoEncoding::Owned)
    }
    pub fn open_with_encoding(
        path: impl AsRef<Path>,
        options: Options,
        format: RedoFormat,
        encoding: RedoEncoding,
    ) -> Result<Self> {
        Self::open_inner(path.as_ref(), options, Some(format), encoding)
    }
    fn open_inner(
        directory: &Path,
        options: Options,
        requested: Option<RedoFormat>,
        redo_encoding: RedoEncoding,
    ) -> Result<Self> {
        Self::open_inner_with_transform(directory, options, requested, redo_encoding, None)
    }
    pub fn open_with_transform(
        path: impl AsRef<Path>,
        options: Options,
        context: Arc<ulmdb_storage::transform::TransformContext>,
    ) -> Result<Self> {
        Self::open_inner_with_transform(
            path.as_ref(),
            options,
            None,
            RedoEncoding::Owned,
            Some(context),
        )
    }
    pub fn open_with_encoding_and_transform(
        path: impl AsRef<Path>,
        options: Options,
        format: RedoFormat,
        encoding: RedoEncoding,
        context: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::open_inner_with_transform(path.as_ref(), options, Some(format), encoding, context)
    }
    fn open_inner_with_transform(
        directory: &Path,
        options: Options,
        requested: Option<RedoFormat>,
        redo_encoding: RedoEncoding,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        validate_options(options)?;
        let fence = crate::Environment::lock(directory)?;
        reject_foreign(directory)?;
        let configured =
            ulmdb_storage::transform::check_configuration(directory, transform.as_ref())?;
        let actual = super::journal::detect_with_transform(
            &directory.join("RESIDENT.wal"),
            transform.as_ref(),
        )?;
        if requested
            .is_some_and(|r| (r == RedoFormat::LiteralV1) != (actual == RedoFormat::LiteralV1))
        {
            return Err(Error::InvalidInput(
                "resident requested WAL family mismatch",
            ));
        }
        let redo_format = requested.unwrap_or(actual);
        if redo_format == RedoFormat::LiteralV1 && redo_encoding != RedoEncoding::Owned {
            return Err(Error::InvalidInput("borrowed encoding requires RL2"));
        }
        let (journal, recovered) = match actual {
            RedoFormat::LiteralV1 => {
                let (j, r) =
                    ulmdb_storage::resident::journal::Journal::open_checked_with_transform(
                        &directory.join("RESIDENT.wal"),
                        transform.clone(),
                        |r| Recovered::new(super::recover::rebuild(options, r)?),
                    )?;
                (Journal::Literal(j), r)
            }
            RedoFormat::CompactV2 | RedoFormat::FullPostimageV2 => {
                let (j, r) =
                    ulmdb_storage::resident::journal_v2::Journal::open_checked_with_transform(
                        &directory.join("RESIDENT.wal"),
                        transform.clone(),
                        |r| Recovered::new(super::recover::rebuild_v2(options, r)?),
                    )?;
                (Journal::Compact(j), r)
            }
        };
        if configured.is_some_and(|uuid| uuid != journal.boot().uuid) {
            return Err(Error::CryptoMismatch);
        }
        journal.prune_checkpoints(&directory.join("RESIDENT.wal"))?;
        let Recovered {
            pool,
            snapshot,
            #[cfg(feature = "resident-private-index")]
            private_cache,
        } = recovered;
        Ok(Self {
            current: ArcSwap::from_pointee(snapshot),
            writer: Mutex::new(Writer {
                #[cfg(feature = "resident-prefix-audit-cache")]
                prefix_audit_cache: None,
                journal,
                poisoned: false,
                collections: 0,
                recipe_workspace: Default::default(),
                incremental: None,
                incremental_limits: None,
                incremental_metrics: Default::default(),
                #[cfg(feature = "resident-private-index")]
                private_cache,
            }),
            pool,
            options,
            _fence: fence,
            directory: directory.to_path_buf(),
            wal_capacity: AtomicU64::new(options.wal_capacity),
            creator: std::process::id(),
            redo_format,
            redo_encoding,
        })
    }
    pub fn redo_format(&self) -> RedoFormat {
        self.redo_format
    }
    pub fn redo_encoding(&self) -> RedoEncoding {
        self.redo_encoding
    }
    /// Writer workspace RAM, excluded from serving snapshots/read metadata.
    pub fn redo_workspace_bytes(&self) -> Result<(usize, usize)> {
        let w = self.writer.lock().map_err(|_| Error::Busy)?;
        let frame = match &w.journal {
            Journal::Literal(_) => 0,
            Journal::Compact(j) => j.retained_encode_bytes(),
        };
        Ok((frame, w.recipe_workspace.retained_bytes()))
    }
    pub fn begin_read(&self) -> ReadTxn {
        ReadTxn {
            snapshot: self.current.load_full(),
        }
    }
    pub(super) fn check_process(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident environment"));
        }
        Ok(())
    }
    pub fn begin_write(&self) -> Result<WriteTxn<'_>> {
        self.check_process()?;
        let mut writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if writer.poisoned || writer.journal.is_poisoned() || self.pool.stats()?.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        self.rotate_if_needed(&mut writer)?;
        if self.options.pool.tiny_pages > 0
            && self.options.auto_collect_below > 0
            && self.pool.stats()?.unused_tiny_pages < self.options.pool.tiny_pages.div_ceil(4)
        {
            self.collect_locked(&mut writer)?;
            if !self.pool.checkpoint_tiny_reuse_enabled() {
                let capacity = writer.journal.capacity();
                self.checkpoint_locked(&mut writer, capacity)?;
            }
        }
        // All collection IO/work is inside begin_write, hence inside whole-transaction
        // timing. Physical maintenance may advance LSN while business identity stays.
        if self.options.auto_collect_below > 0
            && self.pool.stats()?.free_pages < self.options.auto_collect_below
        {
            self.collect_locked(&mut writer)?;
        }
        let base = self.current.load_full();
        #[cfg(not(feature = "resident-private-index"))]
        let arena = self.pool.begin(base.image.clone())?;
        #[cfg(feature = "resident-private-index")]
        let arena = self
            .pool
            .begin_with_private_cache(base.image.clone(), &writer.private_cache)?;
        let catalog = super::catalog::Edit::new(&base.catalog);
        let next_database = base.next_database;
        let catalog_capacity = crate::catalog_profile::read(&base.image, base.root)?;
        Ok(WriteTxn {
            env: self,
            guard: writer,
            base,
            arena: Some(arena),
            catalog,
            next_database,
            failed: false,
            overflow_databases: Default::default(),
            catalog_capacity,
            catalog_profile_changed: false,
        })
    }
    pub fn overflow_stats(&self) -> ulmdb_storage::resident::OverflowStats {
        self.pool.overflow_stats()
    }
    pub fn sequence(&self) -> u64 {
        self.current.load().receipt.sequence
    }
    pub fn physical_lsn(&self) -> u64 {
        self.current.load().receipt.lsn
    }
    pub fn options(&self) -> Options {
        Options {
            wal_capacity: self.wal_capacity(),
            ..self.options
        }
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn pool_stats(&self) -> Result<Stats> {
        self.pool.stats()
    }
    pub fn wal_bytes(&self) -> u64 {
        let s = self.current.load();
        s.receipt.offset + s.receipt.length
    }
    pub fn wal_physical_bytes(&self) -> Result<u64> {
        let writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        writer.journal.physical_bytes()
    }
    pub fn wal_capacity(&self) -> u64 {
        self.wal_capacity.load(Ordering::Acquire)
    }
}
pub(super) fn validate_options(o: Options) -> Result<()> {
    if o.pool.kv_pages == 0
        || o.pool
            .kv_pages
            .checked_add(o.pool.tiny_pages)
            .is_none_or(|n| n > ulmdb_storage::resident::wal_codec::MAX_SLOTS)
        || o.wal_capacity < 196608
        || o.wal_capacity > 268_435_456
        || !o.wal_capacity.is_multiple_of(4096)
        || o.graph_read_budget == 0
        || o.auto_collect_below > o.pool.kv_pages
    {
        return Err(Error::InvalidInput("resident pilot geometry/budget"));
    }
    Ok(())
}
fn reject_foreign(path: &Path) -> Result<()> {
    if path.join("CURRENT").exists() || path.join("HYBRID.wal").exists() {
        return Err(Error::InvalidInput(
            "resident format cannot open or upgrade another engine",
        ));
    }
    Ok(())
}

struct Recovered {
    pool: Arc<Pool>,
    snapshot: Snapshot,
    #[cfg(feature = "resident-private-index")]
    private_cache: Arc<ulmdb_storage::resident::PrivateCache>,
}
impl Recovered {
    fn new((pool, snapshot): (Arc<Pool>, Snapshot)) -> Result<Self> {
        // Recovery invokes this inside its validation callback: capacity/budget
        // rejection occurs before every head-heal/clear/readiness mutation.
        #[cfg(feature = "resident-private-index")]
        let private_cache = ulmdb_storage::resident::PrivateCache::create(&pool)?;
        Ok(Self {
            pool,
            snapshot,
            #[cfg(feature = "resident-private-index")]
            private_cache,
        })
    }
}
