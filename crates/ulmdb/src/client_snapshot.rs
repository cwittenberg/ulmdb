//! Independent snapshots for cooperating clients of immutable file environments.
//!
//! This entry point is separate from ordinary environment opens: it acquires no
//! writer capability, modifies no files and does not perform journal recovery.
use crate::{Error, ReadTxn, Result, WriteTxn};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use ulmdb_storage::{ResidencyController, ResidencyPolicy};

pub(crate) fn snapshot_owner() -> Result<u128> {
    // Environment owners occupy the low u64 namespace. Keep independently
    // opened snapshots disjoint without changing the ordinary owner allocator.
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map(|id| (1u128 << 127) | u128::from(id))
        .map_err(|_| Error::LimitExceeded)
}

impl ReadTxn {
    /// Requested per-VMA advice. This is not a guarantee about OS cache behavior.
    pub fn read_ahead(&self) -> crate::ReadAhead {
        self.snapshot.segments.read_ahead()
    }
    /// Open a read-only catalog projection with an empty KV database if absent.
    ///
    /// Compatibility facades can expose a default database without creating it
    /// on disk. Existing databases retain their real kind and identity. The
    /// virtual entry cannot be exported as a writer capability; opening an
    /// ordinary environment still observes only persisted catalog entries.
    pub fn open_snapshot_with_empty_kv(path: impl AsRef<Path>, name: &[u8]) -> Result<Self> {
        if name.is_empty() || name.len() > 255 {
            return Err(Error::InvalidInput(
                "virtual database name must have 1..255 bytes",
            ));
        }
        Self::open_snapshot(path)?.project_empty_kv(name)
    }
    /// Project an empty KV entry without creating it on disk or granting writer
    /// authority. Independently owned snapshots only; shared snapshots fail Busy.
    pub fn project_empty_kv(mut self, name: &[u8]) -> Result<Self> {
        if name.is_empty() || name.len() > 255 {
            return Err(Error::InvalidInput(
                "virtual database name must have 1..255 bytes",
            ));
        }
        let snapshot = Arc::get_mut(&mut self.snapshot).ok_or(Error::Busy)?;
        snapshot
            .catalog
            .entry(name.to_vec())
            .or_insert(crate::catalog::Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id: u64::MAX,
                kind: crate::DatabaseKind::Kv,
                root: ulmdb_core::storage::Reference::NULL,
            });
        Ok(self)
    }

    /// Return this snapshot's database names in unsigned byte order.
    pub fn database_names(&self) -> Result<Vec<Vec<u8>>> {
        self.snapshot.check_external()?;
        Ok(self.snapshot.catalog.keys().cloned().collect())
    }

    /// Open the currently published immutable snapshot without a writer lock.
    ///
    /// A concurrent writer may be active: this captures the committed CURRENT
    /// once, validates its selected files and reachable graph, and pins their
    /// mappings independently of any [`crate::Environment`]. Later commits do
    /// not change returned values. Legacy, Cohort and Indexed file environments
    /// are supported. The experimental `range-lifecycle` feature also permits
    /// strictly read-only format6 Range capture: no healing/trimming, moving
    /// heads may return Busy and malformed heads fail closed. Format5 is rejected.
    ///
    /// Cooperating writers must preserve selected immutable files while this
    /// call loads them. In particular, serialize native reclamation, pruning or
    /// restoration with opening a snapshot. This API does not register a
    /// cross-process maintenance lease. Once loaded, the snapshot owns its
    /// mappings; its database handles are scoped to this particular open.
    pub fn open_snapshot(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_snapshot_with_read_ahead(path, crate::ReadAhead::Normal)
    }
    pub fn open_snapshot_with_read_ahead(
        path: impl AsRef<Path>,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::open_snapshot_with_transform_options(path, read_ahead, None)
    }
    pub fn open_snapshot_with_transform_options(
        path: impl AsRef<Path>,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::open_snapshot_with_cache_options(path, read_ahead, transform, None)
    }
    pub fn open_snapshot_with_cache_options(
        path: impl AsRef<Path>,
        read_ahead: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<Self> {
        if let Some(cache) = &cache {
            cache.validate()?;
            if ulmdb_storage::range::is_selected(path.as_ref())? {
                return Err(Error::InvalidInput("chunk cache does not support Range"));
            }
        }
        read_ahead.validate()?;
        let directory = path.as_ref();
        let configured =
            ulmdb_storage::transform::check_configuration(directory, transform.as_ref())?;
        if transform.is_some() && ulmdb_storage::range::is_selected(directory)? {
            return Err(Error::InvalidInput("Range transforms unsupported"));
        }
        #[cfg(feature = "range-lifecycle")]
        if ulmdb_storage::range::is_selected(directory)? {
            if read_ahead != crate::ReadAhead::Normal {
                return Err(Error::InvalidInput(
                    "Range random mapping advice unsupported",
                ));
            }
            let controller = ResidencyController::new(ResidencyPolicy::Demand)?;
            let state = ulmdb_storage::range::open_snapshot_checked(
                directory,
                &controller,
                |table, manifest| crate::backup::validate_for_admission(table, manifest),
            )?;
            let mut snapshot = crate::environment::snapshot_from_parts(
                state.segments,
                state.manifest,
                snapshot_owner()?,
            )?;
            snapshot.range_backed = true;
            snapshot.range_checkpoint = state.checkpoint;
            snapshot.admission = Some(state.admission);
            return Ok(Self {
                snapshot: Arc::new(snapshot),
            });
        }
        #[cfg(not(feature = "range-lifecycle"))]
        if ulmdb_storage::range::is_selected(directory)? {
            return Err(Error::InvalidInput(
                "independent snapshots require an immutable file environment",
            ));
        }
        let manifest = ulmdb_storage::read_current_with_transform(directory, transform.clone())?;
        if configured.is_some_and(|uuid| uuid != manifest.uuid) {
            return Err(Error::CryptoMismatch);
        }
        let owner = snapshot_owner()?;
        let mut snapshot = match &cache {
            Some(cache) => crate::environment::load_snapshot_with_cache(
                directory, manifest, owner, read_ahead, transform, cache,
            )?,
            None => crate::environment::load_snapshot_with_transform(
                directory, manifest, owner, read_ahead, transform,
            )?,
        };
        if cache.is_none() {
            crate::backup::validate_for_admission(&snapshot.segments, &snapshot.manifest)?;
        }
        let controller = ResidencyController::new(ResidencyPolicy::Demand)?;
        snapshot.admission = Some(controller.admit(&snapshot.segments)?);
        Ok(Self {
            snapshot: Arc::new(snapshot),
        })
    }
}

impl WriteTxn<'_> {
    /// Return the writer's current database names in unsigned byte order.
    /// Private creations and removals are reflected until commit or rollback.
    pub fn database_names(&self) -> Result<Vec<Vec<u8>>> {
        self.live()?;
        Ok(self.catalog.keys().cloned().collect())
    }
}
