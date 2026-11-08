//! Serialized-writer physical relocation. Old mappings/files remain retained.
use crate::{
    Address, DatabaseKind, Environment, Error, Prefix, Result, catalog::Database,
    environment::Snapshot,
};
use std::{
    collections::BTreeMap,
    ops::Bound,
    sync::{Arc, atomic::Ordering},
};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_storage::{Manifest, SegmentBuilder, SegmentTable};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionReport {
    pub sequence: u64,
    pub commit: u128,
    pub previous_generation: u128,
    pub generation: u128,
    pub before_segments: usize,
    pub after_segments: usize,
    pub before_segment_bytes: u64,
    pub after_segment_bytes: u64,
    pub databases: usize,
    pub kv_pairs: u64,
    pub prefix_values: u64,
    pub duplicate_pairs: u64,
}
fn bytes(manifest: &Manifest) -> Result<u64> {
    manifest.segments.iter().try_fold(0u64, |n, e| {
        n.checked_add(e.length).ok_or(Error::LimitExceeded)
    })
}
fn all<S: ReadStore>(store: &S, root: Reference) -> Result<ulmdb_kv::Cursor<'_, S>> {
    ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)
}
fn whole(kind: DatabaseKind) -> Result<Prefix> {
    Prefix::new(
        if kind == DatabaseKind::Ipv4 {
            Address::V4(0)
        } else {
            Address::V6(0)
        },
        0,
    )
    .map_err(|_| Error::Corrupt("invalid compaction prefix family"))
}
impl Environment {
    /// Rebuild one complete committed snapshot while existing readers continue.
    /// Blocks other writers. Writer-private memory rolls across bounded arenas;
    /// live data may span multiple fresh immutable segments.
    /// Physical generation changes, while logical sequence/commit remain stable.
    pub fn compact(&self) -> Result<CompactionReport> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        self.require_writable()?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        crate::backup::validate_file_for_admission(&base.segments, &base.manifest)?;
        let mut arena = SegmentBuilder::new_with_geometry_and_transform(
            &self.directory,
            base.manifest.next_segment,
            base.segments.read_ahead(),
            base.manifest.page_size,
            base.segments.transform().cloned(),
        )?
        .with_value_limit(base.manifest.value_limit);
        let mut catalog = BTreeMap::new();
        let mut catalog_builder = ulmdb_kv::SortedBuilder::new();
        if let Some(bytes) =
            (crate::catalog_profile::read_file(&base.segments, base.manifest.catalog)?).marker()
        {
            catalog_builder.push(&mut arena, b"", &bytes)?;
        }
        let (mut kv_pairs, mut prefix_values, mut duplicate_pairs) = (0u64, 0u64, 0u64);
        for (name, old) in &base.catalog {
            let mut next = Database {
                ordering: old.ordering,
                id: old.id,
                kind: old.kind,
                root: Reference::NULL,
            };
            match old.kind {
                DatabaseKind::Kv => {
                    let built = if base.segments.is_chunked() {
                        ulmdb_kv::rebuild_inorder(
                            &mut arena,
                            crate::chunked::stream::Pairs::new(&base.segments, old.root),
                        )?
                    } else {
                        ulmdb_kv::rebuild_inorder(&mut arena, all(&base.segments, old.root)?)?
                    };
                    next.root = built.root;
                    kv_pairs = kv_pairs
                        .checked_add(built.records)
                        .ok_or(Error::LimitExceeded)?;
                }
                DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                    let built = if base.segments.is_chunked() {
                        ulmdb_prefix::build_sorted(
                            &mut arena,
                            crate::chunked::stream::Subnets::new(
                                &base.segments,
                                old.root,
                                whole(old.kind)?,
                            ),
                        )?
                    } else {
                        ulmdb_prefix::build_sorted(
                            &mut arena,
                            ulmdb_prefix::subnets(&base.segments, old.root, whole(old.kind)?)?,
                        )?
                    };
                    next.root = built.root;
                    prefix_values = prefix_values
                        .checked_add(built.records)
                        .ok_or(Error::LimitExceeded)?;
                }
                DatabaseKind::Duplicates { width } => {
                    let (new, count) = if base.segments.is_chunked() {
                        crate::duplicates::copy_buckets_scoped(
                            &base.segments,
                            old.root,
                            &mut arena,
                            width,
                        )?
                    } else {
                        crate::duplicates::copy_buckets(
                            &base.segments,
                            old.root,
                            &mut arena,
                            width,
                        )?
                    };
                    next.root = new;
                    duplicate_pairs = duplicate_pairs
                        .checked_add(count)
                        .ok_or(Error::LimitExceeded)?;
                }
            }
            catalog_builder.push(
                &mut arena,
                name,
                &next.encode_for_profile(base.manifest.page_size, base.manifest.value_limit),
            )?;
            catalog.insert(name.clone(), next);
        }
        let root = catalog_builder.finish(&mut arena)?.root;
        let mut manifest = base.manifest.clone_metadata();
        manifest.format_minor = base.manifest.format_minor.max(2);
        manifest.previous = base.manifest.generation;
        manifest.generation = ulmdb_storage::new_identity()?;
        manifest.catalog = root;
        manifest.next_database = manifest
            .next_database
            .max(self.database_ids.load(Ordering::Relaxed));
        // Seal errors precede descriptor/pointer publication and do not poison
        // the writer. Any produced orphan file is left for separate reclamation.
        let (segments, next_segment) = arena.finish()?;
        manifest.next_segment = next_segment;
        manifest.segments = segments.entries().into();
        if manifest.format_minor == 4 {
            // A full metadata rebuild releases old node-origin cohort pins.
            manifest.prepare_indexed(None)?;
        }
        crate::backup::validate_file_for_admission(&segments, &manifest)?;
        equivalent(&base, &segments, manifest.catalog, &catalog)?;
        let report = CompactionReport {
            sequence: manifest.sequence,
            commit: manifest.commit,
            previous_generation: base.manifest.generation,
            generation: manifest.generation,
            before_segments: base.manifest.segments.len(),
            after_segments: manifest.segments.len(),
            before_segment_bytes: bytes(&base.manifest)?,
            after_segment_bytes: bytes(&manifest)?,
            databases: catalog.len(),
            kv_pairs,
            prefix_values,
            duplicate_pairs,
        };
        let segments = if base.segments.is_chunked() {
            drop(segments);
            crate::environment::cached_candidate_segments(
                &self.directory,
                &manifest,
                &base.segments,
                crate::Durability::Full,
            )?
        } else {
            segments
        };
        let mut snapshot = Snapshot {
            range_backed: false,
            external: None,
            physical_pin: None,
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: None,
            owner: base.owner,
            manifest,
            segments,
            catalog,
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
pub(crate) fn equivalent(
    base: &Snapshot,
    after: &SegmentTable,
    catalog_root: Reference,
    catalog: &BTreeMap<Vec<u8>, Database>,
) -> Result<()> {
    let source_capacity = catalog_matches(&base.segments, base.manifest.catalog, &base.catalog)?;
    let target_capacity = catalog_matches(after, catalog_root, catalog)?;
    if source_capacity != target_capacity {
        return Err(Error::Corrupt("compacted catalog profile changed"));
    }
    if base.catalog.len() != catalog.len() {
        return Err(Error::Corrupt("compacted database count changed"));
    }
    for (name, old) in &base.catalog {
        let new = catalog
            .get(name)
            .ok_or(Error::Corrupt("compacted database missing"))?;
        if old.id != new.id || old.kind != new.kind || old.ordering != new.ordering {
            return Err(Error::Corrupt("compacted database identity changed"));
        }
        match old.kind {
            DatabaseKind::Kv => {
                if base.segments.is_chunked() || after.is_chunked() {
                    let mut a = crate::chunked::stream::Pairs::new(&base.segments, old.root);
                    let mut b = crate::chunked::stream::Pairs::new(after, new.root);
                    loop {
                        match (a.next().transpose()?, b.next().transpose()?) {
                            (None, None) => break,
                            (Some(a), Some(b)) if a == b => {}
                            _ => return Err(Error::Corrupt("compacted KV content changed")),
                        }
                    }
                    continue;
                }
                let mut a = all(&base.segments, old.root)?;
                let mut b = all(after, new.root)?;
                loop {
                    match (a.next().transpose()?, b.next().transpose()?) {
                        (None, None) => break,
                        (Some(a), Some(b)) if a == b => {}
                        _ => return Err(Error::Corrupt("compacted KV content changed")),
                    }
                }
            }
            DatabaseKind::Duplicates { width } => {
                if base.segments.is_chunked() || after.is_chunked() {
                    crate::duplicates::equivalent_buckets_scoped(
                        &base.segments,
                        old.root,
                        after,
                        new.root,
                        width,
                    )?;
                } else {
                    crate::duplicates::equivalent_buckets(
                        &base.segments,
                        old.root,
                        after,
                        new.root,
                        width,
                    )?;
                }
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                let query = whole(old.kind)?;
                if base.segments.is_chunked() || after.is_chunked() {
                    let mut a =
                        crate::chunked::stream::Subnets::new(&base.segments, old.root, query);
                    let mut b = crate::chunked::stream::Subnets::new(after, new.root, query);
                    loop {
                        match (a.next().transpose()?, b.next().transpose()?) {
                            (None, None) => break,
                            (Some(a), Some(b)) if a == b => {}
                            _ => return Err(Error::Corrupt("compacted prefix content changed")),
                        }
                    }
                    continue;
                }
                let mut a = ulmdb_prefix::subnets(&base.segments, old.root, query)?;
                let mut b = ulmdb_prefix::subnets(after, new.root, query)?;
                loop {
                    match (a.next().transpose()?, b.next().transpose()?) {
                        (None, None) => break,
                        (Some(a), Some(b)) if a == b => {}
                        _ => return Err(Error::Corrupt("compacted prefix content changed")),
                    }
                }
            }
        }
    }
    Ok(())
}

// Check the actual manifest-reachable catalog, rather than relying solely on
// the in-process metadata used while constructing its encoded records.
fn catalog_matches(
    store: &SegmentTable,
    root: Reference,
    expected: &BTreeMap<Vec<u8>, Database>,
) -> Result<crate::CatalogCapacity> {
    fn check<I, K, V>(
        mut actual: I,
        expected: &BTreeMap<Vec<u8>, Database>,
        page: crate::PageSize,
        value_limit: crate::ValueLimit,
    ) -> Result<crate::CatalogCapacity>
    where
        I: Iterator<Item = Result<(K, V)>>,
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let first = actual.next().transpose()?;
        let (capacity, mut pending) = match first {
            Some((key, value)) if key.as_ref().is_empty() => {
                (crate::CatalogCapacity::decode_marker(value.as_ref())?, None)
            }
            other => (crate::CatalogCapacity::DEFAULT, other),
        };
        if expected.len() > capacity.get() {
            return Err(Error::Corrupt("compacted catalog exceeds profile"));
        }
        for (name, db) in expected {
            let (key, value) = match pending.take() {
                Some(row) => row,
                None => actual
                    .next()
                    .transpose()?
                    .ok_or(Error::Corrupt("compacted catalog record missing"))?,
            };
            if key.as_ref() != name || value.as_ref() != db.encode_for_profile(page, value_limit) {
                return Err(Error::Corrupt("compacted catalog metadata disagrees"));
            }
        }
        if let Some(extra) = actual.next() {
            extra?;
            return Err(Error::Corrupt("extra compacted catalog record"));
        }
        if pending.is_some() {
            return Err(Error::Corrupt("extra compacted catalog record"));
        }
        Ok(capacity)
    }
    if store.is_chunked() {
        check(
            crate::chunked::stream::Pairs::new(store, root),
            expected,
            store.page_geometry(),
            store.value_limit(),
        )
    } else {
        check(
            all(store, root)?,
            expected,
            crate::PageSize::new(store.page_size())?,
            crate::ValueLimit::new(store.max_value_size())?,
        )
    }
}
