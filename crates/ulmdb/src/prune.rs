//! Trace complete typed graphs, then publish a smaller inventory without copies.
use crate::{Environment, Error, Result, environment::Snapshot};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    sync::{Arc, atomic::Ordering},
};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_storage::SegmentTable;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PruneReport {
    pub changed: bool,
    pub sequence: u64,
    pub commit: u128,
    pub previous_generation: u128,
    pub generation: u128,
    pub before_segments: usize,
    pub after_segments: usize,
    pub before_segment_bytes: u64,
    pub after_segment_bytes: u64,
}
struct TrackingStore<'a> {
    inner: &'a SegmentTable,
    segments: RefCell<BTreeSet<u64>>,
}
impl ReadStore for TrackingStore<'_> {
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        let bytes = self.inner.read(at, length)?;
        self.segments.borrow_mut().insert(at.segment);
        Ok(bytes)
    }
}
struct TrackingScope<'a> {
    inner: SegmentTable,
    segments: &'a RefCell<BTreeSet<u64>>,
}
impl ReadStore for TrackingScope<'_> {
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        let bytes = self.inner.read(at, length)?;
        self.segments.borrow_mut().insert(at.segment);
        Ok(bytes)
    }
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        self.inner.validate_extent(at, length)?;
        self.segments.borrow_mut().insert(at.segment);
        Ok(())
    }
}
impl crate::chunked::admission::ScopedReadStore for TrackingStore<'_> {
    type Scope<'a>
        = TrackingScope<'a>
    where
        Self: 'a;
    fn scope(&self) -> Result<TrackingScope<'_>> {
        Ok(TrackingScope {
            inner: self.inner.fresh_read_scope()?,
            segments: &self.segments,
        })
    }
}
// Private, consumed within this writer-fenced call. Construction pins the full
// validated source and observed IDs; no external caller can issue/reuse a token.
struct ProjectionCertificate<'a> {
    base: &'a Snapshot,
    live: BTreeSet<u64>,
    work: Option<crate::backup::ValidationWork>,
}
impl<'a> ProjectionCertificate<'a> {
    fn trace(base: &'a Snapshot) -> Result<Self> {
        let tracking = TrackingStore {
            inner: &base.segments,
            segments: RefCell::new(BTreeSet::new()),
        };
        let work = if let Some(cache) = crate::chunked::CacheOptions::from_table(&base.segments) {
            crate::chunked::admission::admit(&tracking, &base.manifest, cache.admission)?;
            None
        } else {
            Some(crate::backup::validate_for_admission_measured(
                &tracking,
                &base.manifest,
            )?)
        };
        let live = tracking.segments.into_inner();
        if live.iter().any(|id| {
            base.manifest
                .segments
                .binary_search_by_key(id, |entry| entry.id)
                .is_err()
        }) {
            return Err(Error::Corrupt(
                "traced segment is absent from snapshot inventory",
            ));
        }
        Ok(Self { base, live, work })
    }
    fn project(self) -> Result<(SegmentTable, ulmdb_storage::Manifest)> {
        // select shares the exact original immutable Mapping owners. All root,
        // catalog, counter and format fields remain copied from the same base.
        let segments = self.base.segments.select(&self.live)?;
        let mut manifest = self.base.manifest.clone_metadata();
        manifest.segments = segments.entries().into();
        // The original second traversal has *smaller* inventory-derived limits.
        // Reuse validation only if exact read work AND every per-tree audit fit.
        // Otherwise retain the complete prior validator and its failure outcome.
        if !self
            .work
            .map(|work| work.fits(&manifest))
            .transpose()?
            .unwrap_or(false)
        {
            crate::backup::validate_file_for_admission(&segments, &manifest)?;
        }
        Ok((segments, manifest))
    }
}
fn total(entries: &[ulmdb_storage::SegmentEntry]) -> Result<u64> {
    entries.iter().try_fold(0u64, |sum, e| {
        sum.checked_add(e.length).ok_or(Error::LimitExceeded)
    })
}
impl Environment {
    /// Remove wholly unreachable segments from the selected inventory. This
    /// traces every typed page/cell/payload through the complete maintenance
    /// validator, including branch separators feeding empty descendants.
    /// No records are rebuilt, no mappings reloaded and no data files deleted.
    pub fn prune_inventory(&self) -> Result<PruneReport> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        let projection = ProjectionCertificate::trace(&base)?;
        let before_bytes = total(&base.manifest.segments)?;
        let mut report = PruneReport {
            changed: false,
            sequence: base.manifest.sequence,
            commit: base.manifest.commit,
            previous_generation: base.manifest.generation,
            generation: base.manifest.generation,
            before_segments: base.manifest.segments.len(),
            after_segments: base.manifest.segments.len(),
            before_segment_bytes: before_bytes,
            after_segment_bytes: before_bytes,
        };
        if projection.live.len() == base.manifest.segments.len() {
            return Ok(report);
        }
        let (segments, mut manifest) = projection.project()?;
        manifest.previous = base.manifest.generation;
        manifest.generation = ulmdb_storage::new_identity()?;
        manifest.next_database = manifest
            .next_database
            .max(self.database_ids.load(Ordering::Relaxed));
        if manifest.format_minor == 4 {
            // Maintenance charges a full metadata rebuild to release old pins.
            manifest.prepare_indexed(None)?;
        }
        report.changed = true;
        report.generation = manifest.generation;
        report.after_segments = manifest.segments.len();
        report.after_segment_bytes = total(&manifest.segments)?;
        let mut snapshot = Snapshot {
            range_backed: false,
            external: None,
            physical_pin: None,
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: None,
            owner: base.owner,
            manifest,
            segments,
            catalog: base.catalog.clone(),
            admission: None,
        };
        self.admit_snapshot(&mut snapshot)?;
        let publication = self.prepare_file_snapshot(&mut snapshot, None)?;
        let snapshot = Arc::new(snapshot);
        if publication.publish().is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        if self.activate(snapshot).is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        Ok(report)
    }
}
