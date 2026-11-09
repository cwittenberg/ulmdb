//! Complete graph checks with one temporary pin scope per decoded node. Only
//! owned path bounds/catalogue metadata survive a step; never the full graph.
use super::*;
use std::{cell::Cell, rc::Rc};
use ulmdb_core::{object_graph::ObjectKind, storage::ReadStore};
use ulmdb_kv::ordering::Comparator;
pub(crate) trait ScopedReadStore: ReadStore {
    type Scope<'a>: ReadStore
    where
        Self: 'a;
    fn scope(&self) -> Result<Self::Scope<'_>>;
}
impl ScopedReadStore for ChunkedSegmentTable {
    type Scope<'a> = Self;
    fn scope(&self) -> Result<Self> {
        Ok(ChunkedSegmentTable::scope(self))
    }
}
impl ScopedReadStore for ulmdb_storage::SegmentTable {
    type Scope<'a> = Self;
    fn scope(&self) -> Result<Self> {
        self.fresh_read_scope()
    }
}
#[derive(Clone, Copy, Debug)]
pub struct AdmissionLimits {
    pub reads: u64,
    pub nodes: u64,
    pub metadata_bytes: usize,
}
impl AdmissionLimits {
    pub(crate) fn validate(self) -> Result<()> {
        if self.reads == 0
            || self.nodes == 0
            || self.metadata_bytes == 0
            || self.metadata_bytes > 256 << 20
        {
            return Err(Error::InvalidInput("chunk admission limits"));
        }
        Ok(())
    }
}
impl Default for AdmissionLimits {
    fn default() -> Self {
        Self {
            reads: 1_000_000_000,
            nodes: 16_000_000,
            metadata_bytes: 64 << 20,
        }
    }
}
struct Work {
    reads: Cell<u64>,
    nodes: Cell<u64>,
    metadata: Cell<usize>,
    cap: usize,
}
impl Work {
    fn visit(&self) -> Result<()> {
        self.nodes.set(
            self.nodes
                .get()
                .checked_sub(1)
                .ok_or(Error::LimitExceeded)?,
        );
        Ok(())
    }
    fn enter(self: &Rc<Self>, size: usize) -> Result<Charge> {
        let n = self
            .metadata
            .get()
            .checked_add(size)
            .ok_or(Error::LimitExceeded)?;
        if n > self.cap {
            return Err(Error::LimitExceeded);
        }
        self.metadata.set(n);
        Ok(Charge(self.clone(), size))
    }
}
struct Charge(Rc<Work>, usize);
impl Drop for Charge {
    fn drop(&mut self) {
        self.0.metadata.set(self.0.metadata.get() - self.1)
    }
}
struct Checked<'a, S: ReadStore, H: ScopedReadStore> {
    table: &'a S,
    headers: &'a H,
    work: &'a Work,
    key: usize,
    value: usize,
}
impl<S: ReadStore, H: ScopedReadStore> ReadStore for Checked<'_, S, H> {
    fn page_size(&self) -> usize {
        self.table.page_size()
    }
    fn max_key_size(&self) -> usize {
        self.key
    }
    fn max_value_size(&self) -> usize {
        self.value
    }
    fn payload_length(&self, at: Reference) -> Result<usize> {
        // The scalar query returns no borrowed bytes. Release its temporary
        // header window immediately, rather than pinning every distant overflow
        // header for the lifetime of a containing page. Preserve the original
        // read + extent-check admission charge.
        self.work.reads.set(
            self.work
                .reads
                .get()
                .checked_sub(2)
                .ok_or(Error::LimitExceeded)?,
        );
        self.headers.scope()?.payload_length(at)
    }
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        self.work.reads.set(
            self.work
                .reads
                .get()
                .checked_sub(1)
                .ok_or(Error::LimitExceeded)?,
        );
        self.table.validate_extent(at, length)
    }
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        self.work.reads.set(
            self.work
                .reads
                .get()
                .checked_sub(1)
                .ok_or(Error::LimitExceeded)?,
        );
        self.table.read(at, len)
    }
}
#[derive(Clone, Copy)]
enum Role {
    Catalog,
    Kv,
    Duplicates(Option<u32>),
    Values(Option<u32>),
}
fn bind(
    spec: OrderingSpec,
    callback: Option<&crate::ordering::CustomComparator>,
    structural_custom: bool,
) -> Result<Option<BoundComparator>> {
    if spec == OrderingSpec::Custom && callback.is_none() && structural_custom {
        Ok(None)
    } else {
        BoundComparator::bind(spec, callback).map(Some)
    }
}
struct Walker<'a, S: ScopedReadStore> {
    table: &'a S,
    manifest: &'a Manifest,
    work: Rc<Work>,
    catalog: BTreeMap<Vec<u8>, Database>,
    ids: BTreeSet<u64>,
    capacity: crate::CatalogCapacity,
}
impl<S: ScopedReadStore> Walker<'_, S> {
    #[allow(clippy::too_many_arguments)]
    fn tree(
        &mut self,
        at: Reference,
        role: Role,
        order: Option<&BoundComparator>,
        values: Option<&BoundComparator>,
        max_value: usize,
        level: Option<u16>,
        lower: Option<&[u8]>,
        upper: Option<&[u8]>,
        depth: usize,
    ) -> Result<u64> {
        if at.is_null() {
            if level.is_some() {
                return Err(Error::Corrupt("null child in chunk graph"));
            }
            return Ok(0);
        }
        if depth >= 64 {
            return Err(Error::Corrupt("chunk tree depth"));
        }
        self.work.visit()?;
        let key = if matches!(role, Role::Values(_)) {
            65536
        } else {
            self.table.max_key_size()
        };
        let local = {
            let scope = self.table.scope()?;
            let checked = Checked {
                table: &scope,
                headers: self.table,
                work: &self.work,
                key,
                value: max_value,
            };
            ulmdb_kv::object_graph::inspect_page_bounded(
                &checked,
                at,
                matches!(role, Role::Catalog | Role::Duplicates(_)),
                key,
                max_value,
                order,
                self.work
                    .cap
                    .checked_sub(self.work.metadata.get())
                    .ok_or(Error::LimitExceeded)?,
            )?
        };
        let ObjectKind::KvPage { level: actual } = local.object.kind else {
            return Err(Error::Corrupt("chunk graph page type"));
        };
        if level.is_some_and(|l| l != actual) {
            return Err(Error::Corrupt("chunk graph child level"));
        }
        if let Some(c) = order {
            if local
                .minimum
                .as_deref()
                .is_some_and(|m| lower.is_some_and(|l| c.compare(m, l).is_lt()))
                || local
                    .maximum
                    .as_deref()
                    .is_some_and(|m| upper.is_some_and(|u| !c.compare(m, u).is_lt()))
            {
                return Err(Error::Corrupt("chunk graph ancestor bounds"));
            }
        }
        let size = 512usize
            .checked_add(
                local
                    .payloads
                    .capacity()
                    .checked_mul(std::mem::size_of::<ulmdb_core::object_graph::PayloadExtent>())
                    .ok_or(Error::LimitExceeded)?,
            )
            .and_then(|n| n.checked_add(local.minimum.as_ref().map_or(0, Vec::len)))
            .and_then(|n| n.checked_add(local.maximum.as_ref().map_or(0, Vec::len)))
            .ok_or(Error::LimitExceeded)?
            .checked_add(local.children.iter().try_fold(0usize, |n, c| {
                n.checked_add(
                    128 + c.lower.as_ref().map_or(0, Vec::capacity)
                        + c.upper.as_ref().map_or(0, Vec::capacity),
                )
                .ok_or(Error::LimitExceeded)
            })?)
            .ok_or(Error::LimitExceeded)?
            .checked_add(
                local
                    .rows
                    .iter()
                    .map(|(k, v, _, _)| k.capacity() + v.capacity() + 256)
                    .sum::<usize>(),
            )
            .ok_or(Error::LimitExceeded)?;
        let _charge = self.work.enter(size)?;
        if let Role::Values(width) = role {
            if local.maximum_value != 0
                || (local.minimum_key_bytes != usize::MAX
                    && width.is_some_and(|w| {
                        local.minimum_key_bytes != w as usize
                            || local.maximum_key_bytes != w as usize
                    }))
            {
                return Err(Error::Corrupt("chunk duplicate width/value"));
            }
        }
        for (name, bytes, _, _) in &local.rows {
            match role {
                Role::Catalog => {
                    if name.is_empty() {
                        if !self.catalog.is_empty()
                            || self.capacity != crate::CatalogCapacity::DEFAULT
                        {
                            return Err(Error::Corrupt("duplicate or late catalog profile"));
                        }
                        self.capacity = crate::CatalogCapacity::decode_marker(bytes)?;
                        continue;
                    }
                    if name.len() > 255 || self.catalog.len() >= self.capacity.get() {
                        return Err(Error::Corrupt("chunk catalogue name/count"));
                    }
                    let next = self
                        .work
                        .metadata
                        .get()
                        .checked_add(name.len() + std::mem::size_of::<Database>() + 128)
                        .ok_or(Error::LimitExceeded)?;
                    if next > self.work.cap {
                        return Err(Error::LimitExceeded);
                    }
                    self.work.metadata.set(next);
                    let db = Database::decode_for_profile(
                        bytes,
                        self.manifest.page_size,
                        self.manifest.value_limit,
                    )?;
                    if db.id >= self.manifest.next_database
                        || !self.ids.insert(db.id)
                        || self.catalog.insert(name.clone(), db).is_some()
                    {
                        return Err(Error::Corrupt("chunk catalogue identity"));
                    }
                }
                Role::Duplicates(width) => {
                    match crate::duplicates::bucket::Bucket::decode_with(
                        bytes,
                        width,
                        values.map(|c| c as &dyn Comparator),
                    )? {
                        crate::duplicates::bucket::Bucket::Inline(_) => (),
                        crate::duplicates::bucket::Bucket::Tree { root, count } => {
                            let rows = self.tree(
                                root,
                                Role::Values(width),
                                values,
                                None,
                                0,
                                None,
                                None,
                                None,
                                0,
                            )?;
                            if rows != count {
                                return Err(Error::Corrupt("chunk duplicate count"));
                            }
                        }
                    }
                }
                _ => (),
            }
        }
        let mut count = local.records as u64;
        for child in &local.children {
            let lo = child.lower.as_deref().or(lower);
            let hi = child.upper.as_deref().or(upper);
            count = count
                .checked_add(
                    self.tree(
                        child.reference,
                        role,
                        order,
                        values,
                        max_value,
                        Some(
                            actual
                                .checked_sub(1)
                                .ok_or(Error::Corrupt("chunk leaf children"))?,
                        ),
                        lo,
                        hi,
                        depth + 1,
                    )?,
                )
                .ok_or(Error::LimitExceeded)?;
        }
        Ok(count)
    }
    fn prefix(
        &mut self,
        root: Reference,
        kind: DatabaseKind,
        value_limit: usize,
        depth: usize,
    ) -> Result<u64> {
        if root.is_null() {
            return Ok(0);
        }
        if depth > 128 {
            return Err(Error::Corrupt("chunk radix depth"));
        }
        self.work.visit()?;
        let local = {
            let scope = self.table.scope()?;
            let checked = Checked {
                table: &scope,
                headers: self.table,
                work: &self.work,
                key: self.table.max_key_size(),
                value: value_limit,
            };
            ulmdb_prefix::object_graph::inspect_node(&checked, root)?
        };
        crate::read::check_family(kind, local.prefix.network())
            .map_err(|_| Error::Corrupt("chunk radix family"))?;
        if local
            .payload
            .is_some_and(|p| p.object.length.saturating_sub(16) > value_limit as u64)
        {
            return Err(Error::Corrupt("chunk radix value limit"));
        }
        let mut count = u64::from(local.payload.is_some());
        for (child, _) in local.children {
            count = count
                .checked_add(self.prefix(child, kind, value_limit, depth + 1)?)
                .ok_or(Error::LimitExceeded)?;
        }
        Ok(count)
    }
}
pub(crate) fn admit<S: ScopedReadStore>(
    table: &S,
    manifest: &Manifest,
    limits: AdmissionLimits,
) -> Result<BTreeMap<Vec<u8>, Database>> {
    admit_with_policy(table, manifest, limits, true, &BTreeMap::new())
}
pub(crate) fn admit_with_policy<S: ScopedReadStore>(
    table: &S,
    manifest: &Manifest,
    limits: AdmissionLimits,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, DatabaseComparators>,
) -> Result<BTreeMap<Vec<u8>, Database>> {
    if let Some(observer) = ulmdb_core::work::current_observer() {
        return crate::backup::controlled_verify::admit_scoped_with_observer(
            table, manifest, limits, structural_custom, callbacks, observer.as_ref(),
        );
    }
    admit_with_policy_unobserved(table, manifest, limits, structural_custom, callbacks)
}
pub(crate) fn admit_with_policy_unobserved<S: ScopedReadStore>(
    table: &S,
    manifest: &Manifest,
    limits: AdmissionLimits,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, DatabaseComparators>,
) -> Result<BTreeMap<Vec<u8>, Database>> {
    limits.validate()?;
    let bytes = manifest.segments.iter().try_fold(0u64, |n, e| {
        n.checked_add(e.length).ok_or(Error::LimitExceeded)
    })?;
    let reads = bytes
        .checked_mul(64)
        .and_then(|n| n.checked_add(4096))
        .ok_or(Error::LimitExceeded)?
        .min(limits.reads);
    let nodes = bytes
        .checked_div(manifest.page_size.get() as u64)
        .and_then(|n| n.checked_add(1))
        .and_then(|n| n.checked_mul(64))
        .ok_or(Error::LimitExceeded)?
        .min(limits.nodes);
    let mut walker = Walker {
        table,
        manifest,
        work: Rc::new(Work {
            reads: Cell::new(reads),
            nodes: Cell::new(nodes),
            metadata: Cell::new(0),
            cap: limits.metadata_bytes,
        }),
        catalog: BTreeMap::new(),
        ids: BTreeSet::new(),
        capacity: Default::default(),
    };
    walker.tree(
        manifest.catalog,
        Role::Catalog,
        Some(&BoundComparator::Bytewise),
        None,
        80,
        None,
        None,
        None,
        0,
    )?;
    let catalog = std::mem::take(&mut walker.catalog);
    for (name, db) in &catalog {
        let default = DatabaseComparators::default();
        let callbacks = callbacks.get(name).unwrap_or(&default);
        let keys = bind(db.ordering.keys, callbacks.keys.as_ref(), structural_custom)?;
        let values = bind(
            db.ordering.duplicates,
            callbacks.duplicates.as_ref(),
            structural_custom,
        )?;
        // Each descriptor carries its own historical payload ceiling, independently
        // of the environment's later extension. Retrieve that80-byte row in a short scope.
        let limit = {
            let scope = table.scope()?;
            let checked = Checked {
                table: &scope,
                work: &walker.work,
                headers: table,
                key: table.max_key_size(),
                value: 80,
            };
            let bytes = ulmdb_kv::get(&checked, manifest.catalog, name)?
                .ok_or(Error::Corrupt("chunk descriptor absent"))?;
            Database::declared_value_limit(bytes)?.get()
        };
        match db.kind {
            DatabaseKind::Kv => {
                walker.tree(
                    db.root,
                    Role::Kv,
                    keys.as_ref(),
                    None,
                    limit,
                    None,
                    None,
                    None,
                    0,
                )?;
            }
            DatabaseKind::Duplicates { width } => {
                walker.tree(
                    db.root,
                    Role::Duplicates(width),
                    keys.as_ref(),
                    values.as_ref(),
                    512,
                    None,
                    None,
                    None,
                    0,
                )?;
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                if !db.root.is_null() && walker.prefix(db.root, db.kind, limit, 0)? == 0 {
                    return Err(Error::Corrupt("empty chunk radix root"));
                }
            }
        }
    }
    Ok(catalog)
}
