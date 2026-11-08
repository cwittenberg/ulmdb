//! Checked reachable typed-object statistics, distinct from mapped inventory.
//!
//! These calls traverse the selected graph and never invoke application
//! comparison callbacks. Builtin order/width checks remain available; Custom
//! trees receive structural checks, not a semantic ordering certificate.
//! All extents describe decoded native allocations. Optional file transforms
//! can encode the same segment into a different disk length; these counters
//! are not encoded file sizes, filesystem allocated blocks or OS resident RSS.
use crate::{
    DatabaseHandle, DatabaseKind, Environment, Error, ReadTxn, Result, WriteTxn,
    ordering::{BoundComparator, DatabaseOrdering, OrderingSpec},
};
use std::collections::BTreeMap;
use ulmdb_core::{
    object_graph::{ObjectId, ObjectKind},
    storage::{ReadStore, Reference},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DatabaseStats {
    pub page_size: usize,
    /// Height of the actual root B-tree; radix roots use typed-node path length.
    pub depth: u32,
    /// Largest B-tree height, including duplicate-value subtrees.
    pub max_tree_depth: u32,
    pub branch_pages: u64,
    pub leaf_pages: u64,
    /// Unique logical storage-page spans intersecting reachable payload extents.
    /// Native payloads can share a page: this is not ceil(value-size/page-size).
    pub overflow_pages: u64,
    pub overflow_objects: u64,
    /// Complete reachable payload headers + stored data; excludes alignment gaps.
    pub overflow_bytes: u64,
    pub tree_bytes: u64,
    pub radix_nodes: u64,
    pub radix_bytes: u64,
    /// KV rows, duplicate pairs, or valued prefixes. Duplicates count every pair.
    pub entries: u64,
    /// Unique reachable typed allocations; excludes inventory headers, unreachable
    /// historical bytes, metadata descriptors, and allocator alignment gaps.
    pub reachable_bytes: u64,
}
#[derive(Clone, Copy)]
pub(crate) struct Budget {
    visits: u64,
    objects: u64,
}
impl Budget {
    pub(crate) fn from_bytes(bytes: u64, page_size: usize) -> Result<Self> {
        Ok(Self {
            visits: bytes
                .checked_div(page_size as u64)
                .and_then(|n| n.checked_add(1))
                .and_then(|n| n.checked_mul(64))
                .ok_or(Error::LimitExceeded)?,
            objects: bytes
                .checked_div(16)
                .and_then(|n| n.checked_add(1))
                .ok_or(Error::LimitExceeded)?,
        })
    }
}
struct Collector {
    objects: BTreeMap<Reference, ObjectId>,
    remaining: u64,
    max_objects: u64,
    page_size: usize,
    max_tree_depth: u32,
}
impl Collector {
    fn new(size: usize, budget: Budget) -> Self {
        Self {
            objects: BTreeMap::new(),
            remaining: budget.visits,
            max_objects: budget.objects,
            page_size: size,
            max_tree_depth: 0,
        }
    }
    fn object(&mut self, object: ObjectId) -> Result<()> {
        if object.reference.segment == 0
            || !object.reference.offset.is_multiple_of(16)
            || object.length == 0
        {
            return Err(Error::Corrupt("statistics object identity/extent"));
        }
        let end = object
            .reference
            .offset
            .checked_add(object.length)
            .ok_or(Error::Corrupt("statistics object overflow"))?;
        if let Some(prior) = self.objects.get(&object.reference) {
            return if *prior == object {
                Ok(())
            } else {
                Err(Error::Corrupt(
                    "statistics conflicting object types/extents",
                ))
            };
        }
        if self.objects.len() as u64 >= self.max_objects {
            return Err(Error::LimitExceeded);
        }
        if let Some((at, prior)) = self.objects.range(..object.reference).next_back() {
            if at.segment == object.reference.segment
                && at
                    .offset
                    .checked_add(prior.length)
                    .ok_or(Error::Corrupt("statistics extent overflow"))?
                    > object.reference.offset
            {
                return Err(Error::Corrupt("statistics overlapping allocations"));
            }
        }
        if let Some((at, _)) = self.objects.range(object.reference..).next() {
            if at.segment == object.reference.segment && at.offset < end {
                return Err(Error::Corrupt("statistics overlapping allocations"));
            }
        }
        self.objects.insert(object.reference, object);
        Ok(())
    }
    fn visit(&mut self) -> Result<()> {
        self.remaining = self.remaining.checked_sub(1).ok_or(Error::LimitExceeded)?;
        Ok(())
    }
    fn finish(self, entries: u64, depth: u32) -> Result<DatabaseStats> {
        let mut report = DatabaseStats {
            page_size: self.page_size,
            entries,
            depth,
            max_tree_depth: self.max_tree_depth,
            ..Default::default()
        };
        let mut payload_span: Option<(u64, u64, u64)> = None;
        for object in self.objects.values() {
            report.reachable_bytes = report
                .reachable_bytes
                .checked_add(object.length)
                .ok_or(Error::LimitExceeded)?;
            match object.kind {
                ObjectKind::KvPage { level } => {
                    if level == 0 {
                        report.leaf_pages += 1
                    } else {
                        report.branch_pages += 1
                    }
                    report.tree_bytes = report
                        .tree_bytes
                        .checked_add(object.length)
                        .ok_or(Error::LimitExceeded)?;
                }
                ObjectKind::Payload => {
                    report.overflow_objects += 1;
                    report.overflow_bytes = report
                        .overflow_bytes
                        .checked_add(object.length)
                        .ok_or(Error::LimitExceeded)?;
                    let first = object.reference.offset / self.page_size as u64;
                    let last = (object
                        .reference
                        .offset
                        .checked_add(object.length)
                        .ok_or(Error::LimitExceeded)?
                        - 1)
                        / self.page_size as u64;
                    match &mut payload_span {
                        Some((segment, _, end))
                            if *segment == object.reference.segment
                                && first <= end.checked_add(1).ok_or(Error::LimitExceeded)? =>
                        {
                            *end = (*end).max(last)
                        }
                        _ => {
                            if let Some((_, first, last)) = payload_span {
                                report.overflow_pages = report
                                    .overflow_pages
                                    .checked_add(last - first + 1)
                                    .ok_or(Error::LimitExceeded)?;
                            }
                            payload_span = Some((object.reference.segment, first, last));
                        }
                    }
                }
                ObjectKind::RadixNode { .. } => {
                    report.radix_nodes += 1;
                    report.radix_bytes = report
                        .radix_bytes
                        .checked_add(object.length)
                        .ok_or(Error::LimitExceeded)?;
                }
                ObjectKind::CatalogDescriptor { .. }
                | ObjectKind::CatalogProfile
                | ObjectKind::ResidentExtentSource { .. } => {
                    return Err(Error::Corrupt("statistics standalone catalog descriptor"));
                }
            }
        }
        if let Some((_, first, last)) = payload_span {
            report.overflow_pages = report
                .overflow_pages
                .checked_add(last - first + 1)
                .ok_or(Error::LimitExceeded)?;
        }
        Ok(report)
    }
}
fn stored_width(spec: OrderingSpec, bytes: &[u8]) -> Result<()> {
    use ulmdb_kv::ordering::Comparator;
    if spec != OrderingSpec::Custom {
        BoundComparator::bind(spec, None)?
            .validate(bytes)
            .map_err(|_| Error::Corrupt("statistics stored integer width"))?;
    }
    Ok(())
}
#[derive(Clone, Copy)]
enum TreeKind {
    Kv,
    Duplicates {
        width: Option<u32>,
        values: OrderingSpec,
    },
    DuplicateValues {
        width: Option<u32>,
    },
    Catalog,
}
fn walk<S: ReadStore>(
    store: &S,
    root: Reference,
    keys: OrderingSpec,
    kind: TreeKind,
    collector: &mut Collector,
) -> Result<(u64, u32)> {
    use ulmdb_kv::object_graph::Event;
    let max = collector.remaining;
    let mut entries = 0u64;
    let visitor = |event: Event<'_>| -> Result<()> {
        match event {
            Event::Object(object) => {
                collector.visit()?;
                collector.object(object)?;
            }
            Event::Payload(payload) => collector.object(payload.object)?,
            Event::Edge(_) => {}
            Event::Leaf { key, value, .. } => {
                stored_width(keys, key)?;
                match kind {
                    TreeKind::Kv | TreeKind::Catalog => {
                        if key.len() > store.max_key_size() || value.len() > store.max_value_size()
                        {
                            return Err(Error::Corrupt("statistics KV limit"));
                        }
                        entries = entries.checked_add(1).ok_or(Error::LimitExceeded)?;
                    }
                    TreeKind::DuplicateValues { width } => {
                        if key.len() > 65536
                            || !value.is_empty()
                            || width.is_some_and(|w| key.len() != w as usize)
                        {
                            return Err(Error::Corrupt("statistics duplicate value width/data"));
                        }
                        entries = entries.checked_add(1).ok_or(Error::LimitExceeded)?;
                    }
                    TreeKind::Duplicates { width, values } => {
                        if key.len() > store.max_key_size() {
                            return Err(Error::Corrupt("statistics duplicate key limit"));
                        }
                        use crate::duplicates::bucket::Bucket;
                        let bucket = Bucket::decode_with(value, width, None)?;
                        let count = match bucket {
                            Bucket::Inline(inline) => {
                                for i in 0..inline.count {
                                    stored_width(values, inline.value(i))?;
                                }
                                inline.count as u64
                            }
                            Bucket::Tree { root, count } => {
                                let (actual, _) = walk(
                                    store,
                                    root,
                                    values,
                                    TreeKind::DuplicateValues { width },
                                    collector,
                                )?;
                                if actual != count {
                                    return Err(Error::Corrupt("statistics duplicate count"));
                                }
                                count
                            }
                        };
                        entries = entries.checked_add(count).ok_or(Error::LimitExceeded)?;
                    }
                }
            }
        }
        Ok(())
    };
    let audit = if keys == OrderingSpec::Custom {
        ulmdb_kv::object_graph::visit_tree_structure(store, root, max, visitor)?
    } else {
        ulmdb_kv::object_graph::visit_tree_with(
            store,
            root,
            max,
            &BoundComparator::bind(keys, None)?,
            visitor,
        )?
    };
    collector.max_tree_depth = collector.max_tree_depth.max(audit.height.into());
    Ok((entries, audit.height.into()))
}
pub(crate) fn measure<S: ReadStore>(
    store: &S,
    root: Reference,
    kind: DatabaseKind,
    ordering: DatabaseOrdering,
    budget: Budget,
) -> Result<DatabaseStats> {
    let mut collector = Collector::new(store.page_size(), budget);
    let (entries, depth) = match kind {
        DatabaseKind::Kv => walk(store, root, ordering.keys, TreeKind::Kv, &mut collector)?,
        DatabaseKind::Duplicates { width } => walk(
            store,
            root,
            ordering.keys,
            TreeKind::Duplicates {
                width,
                values: ordering.duplicates,
            },
            &mut collector,
        )?,
        DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
            use ulmdb_prefix::object_graph::Event;
            let family = if kind == DatabaseKind::Ipv4 { 4 } else { 6 };
            let mut entries = 0u64;
            let mut depths = BTreeMap::new();
            if !root.is_null() {
                depths.insert(root, 1u32);
            }
            let mut depth = if root.is_null() { 0 } else { 1 };
            ulmdb_prefix::object_graph::visit_tree(store, root, budget.visits, |event| {
                match event {
                    Event::Object(object) => {
                        collector.visit()?;
                        if object.kind != (ObjectKind::RadixNode { family }) {
                            return Err(Error::Corrupt("statistics prefix family"));
                        }
                        collector.object(object)?;
                    }
                    Event::Payload(payload) => {
                        if payload.data_length > store.max_value_size() as u64 {
                            return Err(Error::Corrupt("statistics prefix value limit"));
                        }
                        collector.object(payload.object)?;
                        entries = entries.checked_add(1).ok_or(Error::LimitExceeded)?;
                    }
                    Event::Edge(edge) => {
                        if matches!(edge.to.kind, ObjectKind::RadixNode { .. }) {
                            let child = depths
                                .get(&edge.from.reference)
                                .copied()
                                .ok_or(Error::Corrupt("statistics prefix path"))?
                                .checked_add(1)
                                .ok_or(Error::LimitExceeded)?;
                            depth = depth.max(child);
                            depths.insert(edge.to.reference, child);
                        }
                    }
                }
                Ok(())
            })?;
            (entries, depth)
        }
    };
    collector.finish(entries, depth)
}
impl ReadTxn {
    fn physical_budget(&self) -> Result<Budget> {
        Budget::from_bytes(
            self.snapshot
                .manifest
                .segments
                .iter()
                .try_fold(0u64, |n, s| {
                    n.checked_add(s.length).ok_or(Error::LimitExceeded)
                })?,
            self.snapshot.manifest.page_size.get(),
        )
    }
    pub fn database_stats(&self, name: &[u8]) -> Result<DatabaseStats> {
        let db = self.database(name)?;
        measure(
            &self.snapshot.segments,
            db.root,
            db.kind,
            db.ordering,
            self.physical_budget()?,
        )
    }
    pub fn database_stats_handle(&self, handle: &DatabaseHandle) -> Result<DatabaseStats> {
        if self.open_database(handle.name())? != *handle {
            return Err(Error::StaleHandle);
        }
        self.database_stats(handle.name())
    }
    /// Actual native catalog pages, separate from a facade's synthesized MAIN rows.
    pub fn catalog_physical_stats(&self) -> Result<DatabaseStats> {
        let mut c = Collector::new(
            self.snapshot.manifest.page_size.get(),
            self.physical_budget()?,
        );
        let (n, d) = walk(
            &self.snapshot.segments,
            self.snapshot.manifest.catalog,
            OrderingSpec::Bytewise,
            TreeKind::Catalog,
            &mut c,
        )?;
        c.finish(n, d)
    }
}
impl WriteTxn<'_> {
    pub(crate) fn physical_budget(&self) -> Result<Budget> {
        let bytes = self
            .base
            .manifest
            .segments
            .iter()
            .try_fold(self.arena.dirty_bytes() as u64 + 4096, |n, s| {
                n.checked_add(s.length).ok_or(Error::LimitExceeded)
            })?;
        Budget::from_bytes(bytes, self.base.manifest.page_size.get())
    }
    pub fn database_stats(&self, name: &[u8]) -> Result<DatabaseStats> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        measure(
            &self.arena,
            db.root,
            db.kind,
            db.ordering,
            self.physical_budget()?,
        )
    }
    pub fn database_stats_handle(&self, handle: &DatabaseHandle) -> Result<DatabaseStats> {
        if self.open_database(handle.name())? != *handle {
            return Err(Error::StaleHandle);
        }
        self.database_stats(handle.name())
    }
}
impl Environment {
    pub fn database_stats(&self, name: &[u8]) -> Result<DatabaseStats> {
        self.begin_read().database_stats(name)
    }
}
