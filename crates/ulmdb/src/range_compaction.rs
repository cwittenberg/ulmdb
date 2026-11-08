//! Explicit Range6 graph compaction and whole-pack reclamation. No auto policy.
use crate::{DatabaseKind, Environment, Error, Result, environment::Snapshot};
use std::{
    cell::Cell,
    ops::Bound,
    sync::{Arc, atomic::Ordering},
};
use ulmdb_core::storage::{ReadStore, Reference, WriteStore};
use ulmdb_storage::{Arena, SegmentTable};
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RangeCompactReport {
    pub sequence: u64,
    pub commit: u128,
    pub previous_generation: u128,
    pub generation: u128,
    pub source_bytes: u64,
    pub compacted_bytes: u64,
}
struct Source<'a> {
    inner: &'a SegmentTable,
    remaining: Cell<u64>,
}
impl ReadStore for Source<'_> {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn read(&self, r: Reference, n: usize) -> Result<&[u8]> {
        self.remaining.set(
            self.remaining
                .get()
                .checked_sub(n as u64)
                .ok_or(Error::LimitExceeded)?,
        );
        self.inner.read(r, n)
    }
}
struct Target {
    inner: Arena,
    remaining: u64,
}
impl ReadStore for Target {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn read(&self, r: Reference, n: usize) -> Result<&[u8]> {
        self.inner.read(r, n)
    }
}
impl WriteStore for Target {
    fn allocate(&mut self, b: &[u8], a: usize) -> Result<Reference> {
        self.remaining = self
            .remaining
            .checked_sub(b.len().checked_add(a).ok_or(Error::LimitExceeded)? as u64)
            .ok_or(Error::LimitExceeded)?;
        self.inner.allocate(b, a)
    }
    fn rewrite(&mut self, r: Reference, b: &[u8], a: usize) -> Result<Reference> {
        self.remaining = self
            .remaining
            .checked_sub(b.len().checked_add(a).ok_or(Error::LimitExceeded)? as u64)
            .ok_or(Error::LimitExceeded)?;
        self.inner.rewrite(r, b, a)
    }
}
impl Environment {
    /// Rebuild live trees in physical inorder into an independent Range6 asset.
    /// Sequence, commit, parent and database IDs survive; physical generation and
    /// pack ordinal have a genuine new anchored namespace. No callbacks needed.
    /// Budget caps both source inventory and output; read work is at most128x.
    pub fn compact_range(&self, rewrite_budget_bytes: u64) -> Result<RangeCompactReport> {
        if rewrite_budget_bytes < 4096 || rewrite_budget_bytes > 268_435_456 {
            return Err(Error::InvalidInput("Range compaction budget"));
        }
        self.range
            .as_ref()
            .ok_or(Error::InvalidInput("not a Range owner"))?
            .try_lock()
            .map_err(|_| Error::Busy)?
            .check_lifecycle_owner()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        let checkpoint = base
            .range_checkpoint
            .as_ref()
            .ok_or(Error::InvalidInput("Range compaction requires format6"))?;
        if !checkpoint.matches_manifest(&base.manifest)
            || !checkpoint.matches_directory(&self.directory)?
        {
            return Err(Error::StaleHandle);
        }
        let bytes = base.manifest.segments.iter().try_fold(0u64, |n, e| {
            n.checked_add(e.length).ok_or(Error::LimitExceeded)
        })?;
        if bytes > rewrite_budget_bytes {
            return Err(Error::LimitExceeded);
        }
        crate::backup::validate_for_admission(&base.segments, &base.manifest)?;
        let source = Source {
            inner: &base.segments,
            remaining: Cell::new(
                rewrite_budget_bytes
                    .checked_mul(128)
                    .ok_or(Error::LimitExceeded)?,
            ),
        };
        let empty = SegmentTable::default()
            .with_page_size(base.manifest.page_size)?
            .with_value_limit(base.manifest.value_limit)?;
        let mut target = Target {
            inner: Arena::new(empty, base.manifest.next_segment)?,
            remaining: rewrite_budget_bytes,
        };
        let mut manifest = base.manifest.clone_metadata();
        let mut catalog = std::collections::BTreeMap::new();
        let mut encoded = ulmdb_kv::SortedBuilder::new();
        if let Some(marker) = ulmdb_kv::get(&source, base.manifest.catalog, b"")? {
            encoded.push(&mut target, b"", marker)?;
        }
        for (name, old) in &base.catalog {
            let root = match old.kind {
                DatabaseKind::Kv => {
                    ulmdb_kv::rebuild_inorder(
                        &mut target,
                        ulmdb_kv::Cursor::new(
                            &source,
                            old.root,
                            Bound::Unbounded,
                            Bound::Unbounded,
                            false,
                        )?,
                    )?
                    .root
                }
                DatabaseKind::Duplicates { width } => {
                    crate::duplicates::copy_buckets(&source, old.root, &mut target, width)?.0
                }
                DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                    let address = if old.kind == DatabaseKind::Ipv4 {
                        crate::Address::V4(0)
                    } else {
                        crate::Address::V6(0)
                    };
                    ulmdb_prefix::build_sorted(
                        &mut target,
                        ulmdb_prefix::subnets(
                            &source,
                            old.root,
                            crate::Prefix::new(address, 0)
                                .map_err(|_| Error::Corrupt("Range prefix family"))?,
                        )?,
                    )?
                    .root
                }
            };
            let next = crate::catalog::Database {
                root,
                ..old.clone()
            };
            encoded.push(
                &mut target,
                name,
                &next.encode_for_profile(manifest.page_size, manifest.value_limit),
            )?;
            catalog.insert(name.clone(), next);
        }
        manifest.catalog = encoded.finish(&mut target)?.root;
        // Allocate the entire activation owner and catalog before selection.
        let mut snapshot = Arc::new(Snapshot {
            range_backed: true,
            external: None,
            physical_pin: None,
            range_checkpoint: None,
            owner: base.owner,
            manifest: base.manifest.clone_metadata(),
            segments: SegmentTable::default(),
            catalog,
            admission: None,
        });
        let mut store = self
            .range
            .as_ref()
            .unwrap()
            .try_lock()
            .map_err(|_| Error::Busy)?;
        let state = match store.rotate_checked(
            &self.directory,
            checkpoint,
            target.inner,
            manifest,
            &self.residency,
            |t, m| crate::backup::validate_for_admission(t, m),
        ) {
            Ok(s) => s,
            Err(e) => {
                if store.is_poisoned() {
                    self.poisoned.store(true, Ordering::Release);
                }
                return Err(e);
            }
        };
        let report = RangeCompactReport {
            sequence: state.manifest.sequence,
            commit: state.manifest.commit,
            previous_generation: base.manifest.generation,
            generation: state.manifest.generation,
            source_bytes: bytes,
            compacted_bytes: state.manifest.segments.iter().map(|e| e.length).sum(),
        };
        let inner = Arc::get_mut(&mut snapshot).expect("private Range rotation activation");
        inner.manifest = state.manifest;
        inner.segments = state.segments;
        inner.range_checkpoint = state.checkpoint;
        inner.admission = Some(state.admission);
        if self.activate(snapshot).is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        Ok(report)
    }
    /// Remove complete obsolete packs under genuine exclusive asset leases.
    /// Named retention is scanned and checked by Storage, not a caller keep-list.
    pub fn reclaim_range(
        &self,
        maximum_assets: usize,
    ) -> Result<ulmdb_storage::range::RangeReclaimReport> {
        self.range
            .as_ref()
            .ok_or(Error::InvalidInput("not a Range owner"))?
            .try_lock()
            .map_err(|_| Error::Busy)?
            .check_lifecycle_owner()?;
        let _writer = self.writer.try_lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        let source = base
            .range_checkpoint
            .as_ref()
            .ok_or(Error::InvalidInput("Range reclaim requires format6"))?;
        self.range
            .as_ref()
            .unwrap()
            .try_lock()
            .map_err(|_| Error::Busy)?
            .reclaim_rotated(&self.directory, source, maximum_assets)
    }
}
