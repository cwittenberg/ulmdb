//! Experimental owned incremental graph. Only private source-bound issuers.
#![allow(dead_code)] // Removed when the opt-in writer hook is wired.
mod expand;
mod map;
#[cfg(test)]
mod tests;
use super::{Snapshot, catalog_codec, object_graph};
use crate::{DatabaseKind, Error, Prefix, Result};
use map::Map;
use std::{collections::BTreeSet, sync::Arc};
use ulmdb_core::{
    object_graph::{ObjectId, ObjectKind},
    storage::{ReadStore, Reference},
};
use ulmdb_storage::{
    hybrid::redo::WalReceipt,
    resident::{Prepared, VersionKey, View},
};
/// Bounds cold bootstrap/ordinary incremental preparation and accounted retained
/// state. Explicit Collect retains Environment.graph_read_budget for its full
/// audit; its rebind reports work but is not capped by max_work/max_reads here.
/// Accounted metadata excludes malloc/RSS and simultaneous old/pending state;
/// bounded local decoder scratch and persistent path copies are additional.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_objects: usize,
    pub max_edges: usize,
    pub max_reads: u64,
    pub max_work: u64,
    pub max_metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_objects: 32768,
            max_edges: 131072,
            max_reads: 2_000_000,
            max_work: 2_000_000,
            max_metadata_bytes: 64 << 20,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RootSpec {
    pub name: Vec<u8>,
    pub id: u64,
    pub kind: DatabaseKind,
    pub ordering: crate::ordering::DatabaseOrdering,
    pub root: Reference,
    pub value_limit: usize,
}
#[derive(Clone, Debug)]
pub(super) struct RootChange {
    pub before: Option<RootSpec>,
    pub after: Option<RootSpec>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Role {
    Catalog,
    Kv(u8),
    Duplicates(Option<u32>, u8, u8),
    DuplicateValues(Option<u32>, u8),
    V4,
    V6,
    Payload,
    Descriptor,
    Profile,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Key {
    at: Reference,
    role: Role,
}
#[derive(Clone, Debug, Eq, PartialEq)]
enum Summary {
    Kv {
        maximum_value: usize,
        maximum_duplicate: usize,
        entries: u64,
        level: u16,
        minimum: Option<Vec<u8>>,
        maximum: Option<Vec<u8>>,
    },
    Prefix(Prefix, usize),
    Payload,
    Descriptor(RootSpec),
    Profile(crate::CatalogCapacity),
}
#[derive(Clone)]
struct Info {
    object: ObjectId,
    allocation: VersionKey,
    edges: Vec<Key>,
    reads: Vec<object_graph::ReadBinding>,
    versions: Vec<VersionKey>,
    summary: Summary,
    cost: usize,
}
impl Info {
    fn accounted_cost(&self) -> usize {
        let extra = match &self.summary {
            Summary::Kv {
                minimum, maximum, ..
            } => {
                minimum.as_ref().map_or(0, Vec::capacity)
                    + maximum.as_ref().map_or(0, Vec::capacity)
            }
            Summary::Descriptor(r) => r.name.capacity(),
            _ => 0,
        };
        std::mem::size_of::<Self>()
            + self.edges.capacity() * std::mem::size_of::<Key>()
            + self.reads.capacity() * (std::mem::size_of::<object_graph::ReadBinding>() + 64)
            + self.versions.capacity() * std::mem::size_of::<VersionKey>()
            + extra
    }
}
#[derive(Clone)]
struct State {
    info: Arc<Info>,
    incoming: u64,
    active: bool,
}
/// Diagnostic counts, not instruction/time shares. active_objects counts
/// role-qualified graph nodes (catalogue/user aliases may share physical bytes).
/// Metadata is the retained per-certificate accounting, not aggregate peak/RSS.
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub expanded_objects: u64,
    pub checked_reads: u64,
    pub work: u64,
    pub endpoint_revalidations: u64,
    pub metadata_accounted_bytes: usize,
    pub active_objects: usize,
    pub active_edges: usize,
}
/// All maps are persistent AVL roots; clone copies a fixed set of Arcs only.
#[derive(Clone)]
pub(super) struct Certificate {
    view: View,
    root: Reference,
    next_database: u64,
    capacity: crate::CatalogCapacity,
    receipt: WalReceipt,
    commit: u128,
    limits: Limits,
    nodes: Map<Key, State>,
    roots: Map<u64, RootSpec>,
    live: Map<VersionKey, u64>,
    all: Map<VersionKey, ()>,
    dead: Map<VersionKey, ()>,
    endpoints: Map<u32, Arc<Vec<Key>>>,
    descriptors: Map<u64, Arc<Vec<Key>>>,
    info_bytes: usize,
    edge_count: usize,
}
pub(super) struct Pending {
    certificate: Certificate,
    retired: Vec<VersionKey>,
    copy_sources: Vec<VersionKey>,
    parent: WalReceipt,
    parent_commit: u128,
    metrics: Metrics,
    collect: bool,
}
pub(super) struct ReadyCertificate(Certificate);
struct Engine {
    c: Certificate,
    new: BTreeSet<VersionKey>,
    cold: bool,
    metrics: Metrics,
    zero: Vec<Key>,
    touched_ids: BTreeSet<u64>,
}
fn role(kind: DatabaseKind, ordering: crate::ordering::DatabaseOrdering) -> Result<Role> {
    ordering.validate(kind)?;
    match kind {
        DatabaseKind::Kv => Ok(Role::Kv(ordering.keys.code())),
        DatabaseKind::Duplicates { width } => Ok(Role::Duplicates(
            width,
            ordering.keys.code(),
            ordering.duplicates.code(),
        )),
        DatabaseKind::Ipv4 => Ok(Role::V4),
        DatabaseKind::Ipv6 => Ok(Role::V6),
    }
}
fn version(at: Reference) -> Result<VersionKey> {
    ulmdb_storage::resident::reference_version(at)
}

fn key(at: Reference, role: Role) -> Option<Key> {
    (!at.is_null()).then_some(Key { at, role })
}
impl Certificate {
    pub(super) fn bootstrap(base: &Snapshot, limits: Limits) -> Result<Self> {
        // Cold full validation/oracle is explicit. It is never called by prepare.
        let audited = object_graph::bootstrap(
            &base.image,
            base.root,
            base.next_database,
            base.receipt,
            base.commit,
            object_graph::Limits {
                reads: limits.max_reads,
                objects: limits.max_objects,
                edges: limits.max_edges,
            },
        )?;
        let mut c = Self {
            view: base.image.clone(),
            root: base.root,
            next_database: base.next_database,
            capacity: audited.catalog_capacity,
            receipt: base.receipt,
            commit: base.commit,
            limits,
            nodes: Map::default(),
            roots: Map::default(),
            live: Map::default(),
            all: Map::default(),
            dead: Map::default(),
            endpoints: Map::default(),
            descriptors: Map::default(),
            info_bytes: 0,
            edge_count: 0,
        };
        for v in base.image.members() {
            c.all.insert(v, ());
            c.dead.insert(v, ());
        }
        for (name, db) in &base.catalog {
            c.roots.insert(
                db.id,
                RootSpec {
                    value_limit: catalog_codec::value_limit(
                        ulmdb_kv::get(&base.image, base.root, name)?
                            .ok_or(Error::Corrupt("graph base descriptor absent"))?,
                    )?,
                    name: name.clone(),
                    id: db.id,
                    kind: db.kind,
                    ordering: db.ordering,
                    root: db.root,
                },
            );
        }
        let touched_ids = c.roots.entries().into_iter().map(|(id, _)| id).collect();
        let mut e = Engine {
            c,
            new: BTreeSet::new(),
            cold: true,
            metrics: Metrics::default(),
            zero: Vec::new(),
            touched_ids,
        };
        if let Some(k) = key(base.root, Role::Catalog) {
            e.ensure(k, 0)?;
            e.increment(k)?;
        }
        for (_, r) in e.c.roots.entries() {
            if let Some(k) = key(r.root, role(r.kind, r.ordering)?) {
                e.ensure(k, 0)?;
                e.increment(k)?;
            }
        }
        e.check_descriptors()?;
        e.check_budget()?;
        let observed =
            e.c.live
                .entries()
                .into_iter()
                .map(|(k, _)| k)
                .collect::<BTreeSet<_>>();
        if observed != audited.live {
            return Err(Error::Corrupt("bootstrap graph projection mismatch"));
        }
        Ok(e.c)
    }
    pub(super) fn prepare(
        &self,
        base: &Snapshot,
        candidate: &Prepared,
        changes: &[RootChange],
        root: Reference,
        next_database: u64,
        max_retire: usize,
    ) -> Result<Pending> {
        if !self.view.same_snapshot(&base.image)
            || self.root != base.root
            || self.receipt != base.receipt
            || self.commit != base.commit
            || self.next_database != base.next_database
            || !candidate.was_based_on(&base.image)
        {
            return Err(Error::Corrupt("incremental graph source/parent mismatch"));
        }
        if next_database < self.next_database {
            return Err(Error::Corrupt("graph database counter rollback"));
        }
        let mut initial_work = 0u64;
        let mut new = BTreeSet::new();
        let mut sources = BTreeSet::new();
        let mut changed_slots = BTreeSet::new();
        for s in candidate.slots() {
            initial_work = initial_work.checked_add(1).ok_or(Error::LimitExceeded)?;
            if initial_work > self.limits.max_work {
                return Err(Error::LimitExceeded);
            }
            new.insert(s.key);
            changed_slots.insert(s.key.slot);
            if let Some(k) = s.copy_source {
                sources.insert(k);
            }
        }
        let mut retired = Vec::new();
        // O(selected + protected sources), never materialize the entire dead map.
        let mut cursor = self.dead.clone();
        while retired.len() < max_retire {
            let Some((k, _)) = cursor.first() else {
                break;
            };
            initial_work = initial_work.checked_add(1).ok_or(Error::LimitExceeded)?;
            if initial_work > self.limits.max_work {
                return Err(Error::LimitExceeded);
            }
            let k = *k;
            cursor.remove(&k);
            if !sources.contains(&k) {
                retired.push(k);
                changed_slots.insert(k.slot);
            }
        }
        retired.sort_by_key(|k| k.slot);
        let image = candidate.view().without_versions(&retired)?;
        let mut c = self.clone();
        c.view = image;
        c.root = root;
        c.next_database = next_database;
        for k in &retired {
            c.all.remove(k);
            c.dead.remove(k);
        }
        for k in &new {
            c.all.insert(*k, ());
            c.dead.insert(*k, ());
        }
        let mut e = Engine {
            c,
            new,
            cold: false,
            metrics: Metrics {
                work: initial_work,
                ..Metrics::default()
            },
            zero: Vec::new(),
            touched_ids: BTreeSet::new(),
        };
        let (capacity, reads) =
            crate::catalog_profile::read_bounded(candidate.view(), root, e.c.limits.max_reads)?;
        e.c.capacity = capacity;
        e.metrics.checked_reads = reads;
        // Refresh the narrow reverse endpoint subscriptions BEFORE root transitions.
        for slot in changed_slots {
            if let Some(subscribers) = e.c.endpoints.get(&slot).cloned() {
                for k in subscribers.iter() {
                    e.refresh_endpoint(*k)?;
                }
            }
        }
        let mut root_changes = Vec::new();
        let mut seen = BTreeSet::new();
        for change in changes {
            let id = change
                .before
                .as_ref()
                .or(change.after.as_ref())
                .ok_or(Error::InvalidInput("empty root delta"))?
                .id;
            if !seen.insert(id)
                || change.after.as_ref().is_some_and(|r| r.id != id)
                || e.c.roots.get(&id) != change.before.as_ref()
            {
                return Err(Error::Corrupt("graph root delta source mismatch"));
            }
            if let Some(after) = &change.after {
                if after.id == 0
                    || after.id >= next_database
                    || after.name.is_empty()
                    || after.name.len() > 255
                {
                    return Err(Error::Corrupt("graph root identity/name"));
                }
                role(after.kind, after.ordering)?;
                e.c.roots.insert(id, after.clone());
            } else {
                e.c.roots.remove(&id);
            }
            root_changes.push((change.before.clone(), change.after.clone()));
            e.touched_ids.insert(id);
        }
        if e.c.roots.len() > e.c.capacity.get() {
            return Err(Error::LimitExceeded);
        }
        // Add first, then remove, so unchanged shared subgraphs never deactivate.
        if root != self.root
            && let Some(k) = key(root, Role::Catalog)
        {
            e.ensure(k, 0)?;
            e.increment(k)?;
        }
        for (_, after) in &root_changes {
            if let Some(r) = after
                && let Some(k) = key(r.root, role(r.kind, r.ordering)?)
            {
                e.ensure(k, 0)?;
                e.increment(k)?;
            }
        }
        if root != self.root
            && let Some(k) = key(self.root, Role::Catalog)
        {
            e.decrement(k)?;
        }
        for (before, _) in &root_changes {
            if let Some(r) = before
                && let Some(k) = key(r.root, role(r.kind, r.ordering)?)
            {
                e.decrement(k)?;
            }
        }
        e.drain()?;
        e.check_descriptors()?;
        e.check_budget()?;
        e.metrics.active_objects = e.c.nodes.len();
        e.metrics.active_edges = e.c.edge_count;
        e.metrics.metadata_accounted_bytes = e.accounted()?;
        Ok(Pending {
            certificate: e.c,
            retired,
            copy_sources: sources.into_iter().collect(),
            parent: self.receipt,
            parent_commit: self.commit,
            metrics: e.metrics,
            collect: false,
        })
    }
    pub(super) fn prepare_collection(
        &self,
        base: &Snapshot,
        retired: &[VersionKey],
        checked_live: &BTreeSet<VersionKey>,
    ) -> Result<Pending> {
        if !self.matches(base)
            || checked_live.len() != self.live.len()
            || checked_live.iter().any(|k| self.live.get(k).is_none())
        {
            return Err(Error::Corrupt("collection certificate/full-audit mismatch"));
        }
        let mut c = self.clone();
        c.view = self.view.without_versions(retired)?;
        for k in retired {
            if c.dead.get(k).is_none() || c.live.get(k).is_some() {
                return Err(Error::Corrupt("collection removes certified live version"));
            }
            c.dead.remove(k);
            c.all.remove(k);
        }
        let metadata = c
            .info_bytes
            .checked_add(
                (c.nodes.len() + c.roots.len() + c.all.len())
                    .checked_mul(1024)
                    .ok_or(Error::LimitExceeded)?,
            )
            .ok_or(Error::LimitExceeded)?;
        let metrics = Metrics {
            metadata_accounted_bytes: metadata,
            active_objects: c.nodes.len(),
            active_edges: c.edge_count,
            work: (checked_live.len() + retired.len()) as u64,
            ..Metrics::default()
        };
        Ok(Pending {
            certificate: c,
            retired: retired.to_vec(),
            copy_sources: Vec::new(),
            parent: self.receipt,
            parent_commit: self.commit,
            metrics,
            collect: true,
        })
    }
    // Checkpoint changes only the physical replay anchor. Caller checked matches
    // before publication and preserves the exact image/root/logical identity.
    pub(super) fn checkpoint_rebind(&mut self, receipt: WalReceipt) {
        self.receipt = receipt;
    }
    pub(super) fn matches(&self, base: &Snapshot) -> bool {
        self.view.same_snapshot(&base.image)
            && self.receipt == base.receipt
            && self.root == base.root
            && self.next_database == base.next_database
            && self.commit == base.commit
    }
    pub(super) fn live_versions(&self) -> BTreeSet<VersionKey> {
        self.live.entries().into_iter().map(|(k, _)| k).collect()
    }
}
impl Pending {
    pub(super) fn image(&self) -> &View {
        &self.certificate.view
    }
    pub(super) fn retired(&self) -> &[VersionKey] {
        &self.retired
    }
    pub(super) fn copy_sources(&self) -> &[VersionKey] {
        &self.copy_sources
    }
    pub(super) fn metrics(&self) -> Metrics {
        self.metrics
    }
    /// Must be called with the actual immutable header from Journal's pre-IO
    /// callback; canonical receipt bytes/digest stay the Journal's authority.
    pub(super) fn validate_record(
        &self,
        actual: &ulmdb_storage::resident::wal_codec_v2::Record,
    ) -> Result<()> {
        use ulmdb_storage::resident::wal_codec_v2::Kind;
        let expected_kind = if self.collect {
            Kind::Collect
        } else {
            Kind::Write
        };
        let sequence = if self.collect {
            self.parent.sequence
        } else {
            self.parent
                .sequence
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?
        };
        let commit = if self.collect {
            self.parent_commit
        } else {
            self.parent_commit
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?
        };
        if actual.root != self.certificate.root
            || actual.next_database != self.certificate.next_database
            || actual.retired != self.retired
            || actual.parent != self.parent
            || actual.parent_commit != self.parent_commit
            || actual.kind != expected_kind
            || actual.sequence != sequence
            || actual.lsn != self.parent.lsn.checked_add(1).ok_or(Error::LimitExceeded)?
            || actual.commit != commit
        {
            return Err(Error::Corrupt("graph actual journal header mismatch"));
        }
        Ok(())
    }
    pub(super) fn bind(mut self, receipt: WalReceipt, commit: u128) -> Result<ReadyCertificate> {
        let sequence = if self.collect {
            self.parent.sequence
        } else {
            self.parent
                .sequence
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?
        };
        if receipt.sequence != sequence
            || receipt.lsn != self.parent.lsn.checked_add(1).ok_or(Error::LimitExceeded)?
            || receipt.offset != self.parent.end()?
            || commit
                != if self.collect {
                    self.parent_commit
                } else {
                    self.parent_commit
                        .checked_add(1)
                        .ok_or(Error::LimitExceeded)?
                }
        {
            return Err(Error::Corrupt("graph future receipt binding"));
        }
        self.certificate.receipt = receipt;
        self.certificate.commit = commit;
        Ok(ReadyCertificate(self.certificate))
    }
}
impl ReadyCertificate {
    pub(super) fn activate(self) -> Certificate {
        self.0
    }
}
impl Engine {
    fn work(&mut self) -> Result<()> {
        self.metrics.work = self
            .metrics
            .work
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        if self.metrics.work > self.c.limits.max_work {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
    fn read(&mut self, at: Reference, len: usize) -> Result<(Vec<u8>, object_graph::ReadBinding)> {
        self.metrics.checked_reads = self
            .metrics
            .checked_reads
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        if self.metrics.checked_reads > self.c.limits.max_reads {
            return Err(Error::LimitExceeded);
        }
        let (b, v) = self.c.view.read_with_key(at, len)?;
        Ok((
            b.to_vec(),
            object_graph::ReadBinding {
                reference: at,
                length: len,
                version: v,
            },
        ))
    }
    fn accounted(&self) -> Result<usize> {
        self.c
            .info_bytes
            .checked_add(
                (self.c.nodes.len() + self.c.roots.len() + self.c.all.len())
                    .checked_mul(1024)
                    .ok_or(Error::LimitExceeded)?,
            )
            .ok_or(Error::LimitExceeded)
    }
    fn check_budget(&self) -> Result<()> {
        if self.c.nodes.len() > self.c.limits.max_objects
            || self.c.edge_count > self.c.limits.max_edges
            || self.accounted()? > self.c.limits.max_metadata_bytes
        {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
    fn live_delta(&mut self, v: VersionKey, add: bool) -> Result<()> {
        let n = self.c.live.get(&v).copied().unwrap_or(0);
        if add {
            if self.c.all.get(&v).is_none() {
                return Err(Error::Corrupt("graph read version outside candidate"));
            }
            self.c
                .live
                .insert(v, n.checked_add(1).ok_or(Error::LimitExceeded)?);
            self.c.dead.remove(&v);
        } else {
            if n == 0 {
                return Err(Error::Corrupt("graph projection underflow"));
            }
            if n == 1 {
                self.c.live.remove(&v);
                self.c.dead.insert(v, ());
            } else {
                self.c.live.insert(v, n - 1);
            }
        }
        Ok(())
    }
    fn subscription(&mut self, k: Key, info: &Info, add: bool) -> Result<()> {
        let slots = info
            .reads
            .iter()
            .filter(|r| {
                r.length == 0 && (r.reference.offset & ((1 << 28) - 1)).is_multiple_of(4096)
            })
            .filter_map(|r| version(r.reference).ok().map(|v| v.slot))
            .collect::<BTreeSet<_>>();
        for s in slots {
            let charge = self.c.endpoints.get(&s).map_or(0, |v| v.len());
            for _ in 0..=charge {
                self.work()?;
            }
            let mut list = self
                .c
                .endpoints
                .get(&s)
                .map_or_else(Vec::new, |v| v.as_ref().clone());
            if add {
                if !list.contains(&k) {
                    list.push(k);
                }
            } else {
                list.retain(|q| *q != k);
            }
            if list.is_empty() {
                self.c.endpoints.remove(&s);
            } else {
                self.c.endpoints.insert(s, Arc::new(list));
            }
        }
        Ok(())
    }
    fn descriptor(&mut self, k: Key, info: &Info, add: bool) -> Result<()> {
        if let Summary::Descriptor(r) = &info.summary {
            let charge = self.c.descriptors.get(&r.id).map_or(0, |v| v.len());
            for _ in 0..=charge {
                self.work()?;
            }
            let mut list = self
                .c
                .descriptors
                .get(&r.id)
                .map_or_else(Vec::new, |v| v.as_ref().clone());
            if add {
                if !list.contains(&k) {
                    list.push(k);
                }
            } else {
                list.retain(|x| *x != k);
            }
            if list.is_empty() {
                self.c.descriptors.remove(&r.id);
            } else {
                self.c.descriptors.insert(r.id, Arc::new(list));
            }
            self.touched_ids.insert(r.id);
        }
        Ok(())
    }
    fn increment(&mut self, k: Key) -> Result<()> {
        self.work()?;
        let mut s = self
            .c
            .nodes
            .get(&k)
            .cloned()
            .ok_or(Error::Corrupt("graph edge absent"))?;
        s.incoming = s.incoming.checked_add(1).ok_or(Error::LimitExceeded)?;
        if !s.active {
            s.active = true;
            for v in &s.info.versions {
                self.live_delta(*v, true)?;
            }
            self.subscription(k, &s.info, true)?;
            self.descriptor(k, &s.info, true)?;
            let edges = s.info.edges.clone();
            self.c.nodes.insert(k, s);
            for child in edges {
                self.increment(child)?;
            }
        } else {
            self.c.nodes.insert(k, s);
        }
        Ok(())
    }
    fn decrement(&mut self, k: Key) -> Result<()> {
        self.work()?;
        let mut s = self
            .c
            .nodes
            .get(&k)
            .cloned()
            .ok_or(Error::Corrupt("graph removed edge absent"))?;
        s.incoming = s
            .incoming
            .checked_sub(1)
            .ok_or(Error::Corrupt("graph incoming underflow"))?;
        if s.incoming == 0 {
            self.zero.push(k);
        }
        self.c.nodes.insert(k, s);
        Ok(())
    }
    fn drain(&mut self) -> Result<()> {
        while let Some(k) = self.zero.pop() {
            self.work()?;
            let Some(s) = self.c.nodes.get(&k).cloned() else {
                continue;
            };
            if s.incoming != 0 || !s.active {
                continue;
            }
            for v in &s.info.versions {
                self.live_delta(*v, false)?;
            }
            self.subscription(k, &s.info, false)?;
            self.descriptor(k, &s.info, false)?;
            for child in &s.info.edges {
                self.decrement(*child)?;
            }
            self.c.info_bytes -= s.info.cost;
            self.c.edge_count -= s.info.edges.len();
            self.c.nodes.remove(&k);
        }
        Ok(())
    }
    fn check_descriptors(&self) -> Result<()> {
        for id in &self.touched_ids {
            let list = self.c.descriptors.get(id);
            match self.c.roots.get(id) {
                None => {
                    if list.is_some() {
                        return Err(Error::Corrupt("removed root still in catalogue"));
                    }
                }
                Some(root) => {
                    if let Some(node) = key(root.root, role(root.kind, root.ordering)?)
                        .and_then(|k| self.c.nodes.get(&k))
                    {
                        let maximum = match &node.info.summary {
                            Summary::Kv {
                                maximum_value,
                                maximum_duplicate,
                                ..
                            } => {
                                if matches!(root.kind, DatabaseKind::Duplicates { .. }) {
                                    *maximum_duplicate
                                } else {
                                    *maximum_value
                                }
                            }
                            Summary::Prefix(_, n) => *n,
                            _ => 0,
                        };
                        if maximum > root.value_limit {
                            return Err(Error::Corrupt("graph declared value profile exceeded"));
                        }
                    }
                    let keys = list.ok_or(Error::Corrupt("root absent from catalogue"))?;
                    if keys.len() != 1 {
                        return Err(Error::Corrupt("catalogue identity alias"));
                    }
                    let s = self
                        .c
                        .nodes
                        .get(&keys[0])
                        .ok_or(Error::Corrupt("descriptor missing"))?;
                    if s.incoming != 1
                        || !matches!(&s.info.summary,Summary::Descriptor(r)if r==root)
                    {
                        return Err(Error::Corrupt("catalogue/root delta mismatch"));
                    }
                }
            }
        }
        Ok(())
    }
    fn refresh_endpoint(&mut self, k: Key) -> Result<()> {
        self.work()?;
        let Some(mut s) = self.c.nodes.get(&k).cloned() else {
            return Ok(());
        };
        if !s.active {
            return Ok(());
        }
        for _ in 0..s.info.reads.len() + s.info.versions.len() {
            self.work()?;
        }
        let mut info = (*s.info).clone();
        for r in &mut info.reads {
            if r.length == 0 {
                let (_, v) = self.c.view.read_with_key(r.reference, 0)?;
                self.metrics.checked_reads += 1;
                if self.metrics.checked_reads > self.c.limits.max_reads {
                    return Err(Error::LimitExceeded);
                }
                r.version = v;
                self.metrics.endpoint_revalidations += 1;
            }
        }
        let versions = info
            .reads
            .iter()
            .map(|r| r.version)
            .chain([info.allocation])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        for old in &info.versions {
            if !versions.contains(old) {
                self.live_delta(*old, false)?;
            }
        }
        for new in &versions {
            if !info.versions.contains(new) {
                self.live_delta(*new, true)?;
            }
        }
        info.versions = versions;
        let old = info.cost;
        info.cost = info.accounted_cost();
        self.c.info_bytes = self
            .c
            .info_bytes
            .checked_sub(old)
            .and_then(|n| n.checked_add(info.cost))
            .ok_or(Error::LimitExceeded)?;
        s.info = Arc::new(info);
        self.c.nodes.insert(k, s);
        Ok(())
    }
}
