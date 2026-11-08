//! RAM-W1 observational bootstrap, never incremental/reclaim authority.
//! This prototype deliberately pays the existing full audit AND typed walks.
//! Returned data borrows the exact View; no opaque/public reference is authority.
use super::{ReadTxn, audit, catalog_codec};
use crate::{DatabaseKind, Error, Result};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
};
use ulmdb_core::{
    object_graph::{Edge, EdgeKind, ObjectId, PayloadExtent},
    storage::{ReadStore, Reference},
};
use ulmdb_storage::{
    hybrid::redo::WalReceipt,
    resident::{VersionKey, View},
};
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub reads: u64,
    pub objects: usize,
    pub edges: usize,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RootRole {
    Catalog,
    Database {
        name: Vec<u8>,
        id: u64,
        kind: DatabaseKind,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootEntry {
    pub role: RootRole,
    pub reference: Reference,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReadBinding {
    pub reference: Reference,
    pub length: usize,
    pub version: VersionKey,
}
/// Logical object identity is separate from its checked containing allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectBinding {
    pub object: ObjectId,
    pub allocation: VersionKey,
    /// Complete physical allocation containing this logical object. Descriptor
    /// records are checked 48/64-byte embedded values; other objects contain self.
    pub container: ObjectId,
}
pub struct Bootstrap<'a> {
    pub roots: Vec<RootEntry>,
    pub objects: BTreeMap<ObjectId, ObjectBinding>,
    pub edges: BTreeSet<Edge>,
    /// Actual read extents include empty payload endpoints. These can name an
    /// adjacent version even when the complete payload belongs to its predecessor.
    pub read_bindings: BTreeSet<ReadBinding>,
    pub live: BTreeSet<VersionKey>,
    pub baseline_reads: u64,
    pub typed_reads: u64,
    pub receipt: WalReceipt,
    pub commit: u128,
    pub next_database: u64,
    pub catalog_capacity: crate::CatalogCapacity,
    _view: &'a View,
}
struct Trace<'a> {
    view: &'a View,
    remaining: u64,
    reads: Cell<u64>,
    bindings: RefCell<BTreeSet<ReadBinding>>,
}
impl ReadStore for Trace<'_> {
    fn max_key_size(&self) -> usize {
        65536
    }
    fn max_duplicate_key_size(&self) -> usize {
        4080
    }
    fn max_duplicate_value_size(&self) -> usize {
        65536
    }
    fn read(&self, reference: Reference, length: usize) -> Result<&[u8]> {
        let reads = self.reads.get();
        if reads == self.remaining {
            return Err(Error::LimitExceeded);
        }
        let (bytes, version, sources) = self.view.read_with_sources_limited(
            reference,
            length,
            (self.remaining - reads - 1) as usize,
        )?;
        self.reads.set(reads + 1 + sources.len() as u64);
        for source in sources {
            self.bindings.borrow_mut().insert(ReadBinding {
                reference: source.reference,
                length: source.length,
                version: source.version,
            });
        }
        self.bindings.borrow_mut().insert(ReadBinding {
            reference,
            length,
            version,
        });
        Ok(bytes)
    }
}
fn visit_ordered<S: ReadStore, V: FnMut(ulmdb_kv::object_graph::Event<'_>) -> Result<()>>(
    store: &S,
    root: Reference,
    budget: u64,
    spec: crate::ordering::OrderingSpec,
    visitor: V,
) -> Result<ulmdb_kv::TreeAudit> {
    if spec == crate::ordering::OrderingSpec::Custom {
        ulmdb_kv::object_graph::visit_tree_structure(store, root, budget, visitor)
    } else {
        let order = crate::ordering::BoundComparator::bind(spec, None)?;
        ulmdb_kv::object_graph::visit_tree_with(store, root, budget, &order, visitor)
    }
}
struct Builder<'a> {
    view: &'a View,
    limits: Limits,
    objects: BTreeMap<ObjectId, ObjectBinding>,
    edges: BTreeSet<Edge>,
}
impl Builder<'_> {
    fn object(&mut self, object: ObjectId) -> Result<()> {
        let length = usize::try_from(object.length).map_err(|_| Error::LimitExceeded)?;
        let allocation = self.view.allocation_key(object.reference, length)?;
        if !self.objects.contains_key(&object) && self.objects.len() == self.limits.objects {
            return Err(Error::LimitExceeded);
        }
        self.objects.insert(
            object,
            ObjectBinding {
                object,
                allocation,
                container: object,
            },
        );
        Ok(())
    }
    fn descriptor(&mut self, object: ObjectId, container: ObjectId) -> Result<()> {
        let parent = self
            .objects
            .get(&container)
            .ok_or(Error::Corrupt("typed descriptor allocation missing"))?;
        let end = object
            .reference
            .offset
            .checked_add(object.length)
            .ok_or(Error::LimitExceeded)?;
        let parent_end = container
            .reference
            .offset
            .checked_add(container.length)
            .ok_or(Error::LimitExceeded)?;
        if object.reference.segment != container.reference.segment
            || object.reference.offset < container.reference.offset
            || end > parent_end
        {
            return Err(Error::Corrupt("typed descriptor outside allocation"));
        }
        if !self.objects.contains_key(&object) && self.objects.len() == self.limits.objects {
            return Err(Error::LimitExceeded);
        }
        let allocation = parent.allocation;
        self.objects.insert(
            object,
            ObjectBinding {
                object,
                allocation,
                container,
            },
        );
        Ok(())
    }
    fn edge(&mut self, edge: Edge) -> Result<()> {
        if !self.edges.contains(&edge) && self.edges.len() == self.limits.edges {
            return Err(Error::LimitExceeded);
        }
        self.edges.insert(edge);
        Ok(())
    }
    fn payload(&mut self, payload: PayloadExtent) -> Result<()> {
        self.object(payload.object)?;
        let (_, _, sources) = self
            .view
            .read_with_sources(payload.object.reference, payload.object.length as usize)?;
        for (ordinal, source) in sources.iter().enumerate() {
            let object = ObjectId {
                reference: source.reference,
                length: source.length as u64,
                kind: ulmdb_core::object_graph::ObjectKind::ResidentExtentSource {
                    kind: source.kind,
                },
            };
            self.object(object)?;
            self.edge(Edge {
                from: payload.object,
                to: object,
                kind: EdgeKind::ExtentSource {
                    ordinal: ordinal as u32,
                },
            })?;
        }
        Ok(())
    }
    fn kv(&mut self, event: ulmdb_kv::object_graph::Event<'_>) -> Result<()> {
        match event {
            ulmdb_kv::object_graph::Event::Object(o) => self.object(o),
            ulmdb_kv::object_graph::Event::Edge(e) => self.edge(e),
            ulmdb_kv::object_graph::Event::Payload(p) => self.payload(p),
            ulmdb_kv::object_graph::Event::Leaf { .. } => Ok(()),
        }
    }
}
impl ReadTxn {
    /// Cold diagnostic API. No default collection/recovery/read code uses it.
    /// Its aggregate budget includes full baseline-audit reads and extra walks;
    /// exact allocation binding is deliberately stronger than a readable subspan.
    pub fn bootstrap_objects(&self, limits: Limits) -> Result<Bootstrap<'_>> {
        let s = &self.snapshot;
        bootstrap(
            &s.image,
            s.root,
            s.next_database,
            s.receipt,
            s.commit,
            limits,
        )
    }
}
pub(super) fn bootstrap(
    view: &View,
    root: Reference,
    next_database: u64,
    receipt: WalReceipt,
    commit: u128,
    limits: Limits,
) -> Result<Bootstrap<'_>> {
    // Preserve the existing full validator and its failure precedence FIRST.
    let baseline = audit::audit(view, root, next_database, limits.reads)?;
    let trace = Trace {
        view,
        remaining: limits
            .reads
            .checked_sub(baseline.reads)
            .ok_or(Error::LimitExceeded)?,
        reads: Cell::new(0),
        bindings: RefCell::new(BTreeSet::new()),
    };
    let mut builder = Builder {
        view,
        limits,
        objects: BTreeMap::new(),
        edges: BTreeSet::new(),
    };
    let mut roots = vec![RootEntry {
        role: RootRole::Catalog,
        reference: root,
    }];
    let mut descriptors = BTreeMap::new();
    ulmdb_kv::object_graph::visit_tree(&trace, root, limits.reads, |event| {
        if let ulmdb_kv::object_graph::Event::Leaf {
            page,
            index,
            value_reference,
            value_container,
            key,
            value,
        } = event
        {
            if key.is_empty() {
                crate::CatalogCapacity::decode_marker(value)?;
                let object = ObjectId {
                    reference: value_reference,
                    length: value.len() as u64,
                    kind: ulmdb_core::object_graph::ObjectKind::CatalogProfile,
                };
                builder.descriptor(object, value_container)?;
                builder.edge(Edge {
                    from: page,
                    to: object,
                    kind: EdgeKind::CatalogDescriptor { cell: index },
                })?;
                return builder.kv(event);
            }
            let db = catalog_codec::decode(value)?;
            let object = ObjectId {
                reference: value_reference,
                length: value.len() as u64,
                kind: ulmdb_core::object_graph::ObjectKind::CatalogDescriptor { database: db.id },
            };
            builder.descriptor(object, value_container)?;
            builder.edge(Edge {
                from: page,
                to: object,
                kind: EdgeKind::CatalogDescriptor { cell: index },
            })?;
            if descriptors.insert(key.to_vec(), (object, db)).is_some() {
                return Err(Error::Corrupt("typed catalog duplicate"));
            }
        }
        builder.kv(event)
    })?;
    if descriptors.len() != baseline.catalog.len() {
        return Err(Error::Corrupt("typed catalog count mismatch"));
    }
    for (name, (page, db)) in &descriptors {
        let checked = baseline
            .catalog
            .get(name)
            .ok_or(Error::Corrupt("typed catalog mismatch"))?;
        if (checked.id, checked.kind, checked.root, checked.ordering)
            != (db.id, db.kind, db.root, db.ordering)
        {
            return Err(Error::Corrupt("typed catalog changed"));
        }
        roots.push(RootEntry {
            role: RootRole::Database {
                name: name.clone(),
                id: db.id,
                kind: db.kind,
            },
            reference: db.root,
        });
        match db.kind {
            DatabaseKind::Kv => {
                visit_ordered(&trace, db.root, limits.reads, db.ordering.keys, |e| {
                    builder.kv(e)
                })?;
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                ulmdb_prefix::object_graph::visit_tree(
                    &trace,
                    db.root,
                    limits.reads,
                    |e| match e {
                        ulmdb_prefix::object_graph::Event::Object(o) => builder.object(o),
                        ulmdb_prefix::object_graph::Event::Edge(e) => builder.edge(e),
                        ulmdb_prefix::object_graph::Event::Payload(p) => builder.payload(p),
                    },
                )?;
            }
            DatabaseKind::Duplicates { width } => {
                visit_ordered(&trace, db.root, limits.reads, db.ordering.keys, |event| {
                    builder.kv(event)?;
                    if let ulmdb_kv::object_graph::Event::Leaf {
                        index,
                        value,
                        value_container,
                        ..
                    } = event
                    {
                        if let crate::duplicates::bucket::Bucket::Tree { root, .. } =
                            crate::duplicates::bucket::Bucket::decode_with(value, width, None)?
                        {
                            visit_ordered(
                                &trace,
                                root,
                                limits.reads,
                                db.ordering.duplicates,
                                |e| builder.kv(e),
                            )?;
                            let to = builder
                                .objects
                                .keys()
                                .find(|o| {
                                    o.reference == root
                                        && matches!(
                                            o.kind,
                                            ulmdb_core::object_graph::ObjectKind::KvPage { .. }
                                        )
                                })
                                .copied()
                                .ok_or(Error::Corrupt("typed duplicate root missing"))?;
                            builder.edge(Edge {
                                from: value_container,
                                to,
                                kind: EdgeKind::DuplicateRoot { cell: index },
                            })?;
                        }
                    }
                    Ok(())
                })?;
            }
        }
        if !db.root.is_null() {
            let to = builder
                .objects
                .keys()
                .find(|o| o.reference == db.root)
                .copied()
                .ok_or(Error::Corrupt("typed root object missing"))?;
            builder.edge(Edge {
                from: *page,
                to,
                kind: EdgeKind::CatalogRoot { database: db.id },
            })?;
        }
    }
    for edge in &builder.edges {
        if !builder.objects.contains_key(&edge.from) || !builder.objects.contains_key(&edge.to) {
            return Err(Error::Corrupt("typed edge object missing"));
        }
    }
    let typed_reads = trace.reads.get();
    let read_bindings = trace.bindings.into_inner();
    let live = read_bindings
        .iter()
        .map(|r| r.version)
        .collect::<BTreeSet<_>>();
    if live != baseline.live {
        return Err(Error::Corrupt("typed/full-audit live projection mismatch"));
    }
    Ok(Bootstrap {
        roots,
        objects: builder.objects,
        edges: builder.edges,
        read_bindings,
        live,
        baseline_reads: baseline.reads,
        typed_reads,
        receipt,
        commit,
        next_database,
        catalog_capacity: baseline.catalog_capacity,
        _view: view,
    })
}

#[cfg(test)]
mod tests;
