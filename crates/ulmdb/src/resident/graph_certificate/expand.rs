use super::*;
use std::cell::{Cell, RefCell};
struct Trace<'a> {
    view: &'a View,
    budget: u64,
    reads: Cell<u64>,
    bindings: RefCell<Vec<object_graph::ReadBinding>>,
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
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        let n = self.reads.get();
        if n == self.budget {
            return Err(Error::LimitExceeded);
        }
        let (b, v, sources) =
            self.view
                .read_with_sources_limited(at, len, (self.budget - n - 1) as usize)?;
        self.reads.set(n + 1 + sources.len() as u64);
        for source in sources {
            self.bindings.borrow_mut().push(object_graph::ReadBinding {
                reference: source.reference,
                length: source.length,
                version: source.version,
            });
        }
        self.bindings.borrow_mut().push(object_graph::ReadBinding {
            reference: at,
            length: len,
            version: v,
        });
        Ok(b)
    }
}
impl Engine {
    pub(super) fn ensure(&mut self, k: Key, depth: u16) -> Result<()> {
        self.work()?;
        if depth > 132 {
            return Err(Error::Corrupt("incremental graph depth/cycle"));
        }
        if self.c.nodes.get(&k).is_some() {
            return Ok(());
        }
        let v = version(k.at)?;
        if !self.cold && !self.new.contains(&v) {
            return Err(Error::Corrupt("uncertified committed object source"));
        }
        if !self.c.view.contains_version(v) {
            return Err(Error::Corrupt("graph allocation absent"));
        }
        let remaining = self
            .c
            .limits
            .max_reads
            .checked_sub(self.metrics.checked_reads)
            .ok_or(Error::LimitExceeded)?;
        match k.role {
            Role::Catalog | Role::Kv(_) | Role::Duplicates(..) | Role::DuplicateValues(..) => {
                let tag = match k.role {
                    Role::Kv(tag) | Role::DuplicateValues(_, tag) | Role::Duplicates(_, tag, _) => {
                        tag
                    }
                    _ => 0,
                };
                let order = if tag == 6 {
                    None
                } else {
                    Some(crate::ordering::BoundComparator::bind(
                        crate::ordering::OrderingSpec::decode(tag)?,
                        None,
                    )?)
                };
                let (local, reads, count) = {
                    let t = Trace {
                        view: &self.c.view,
                        budget: remaining,
                        reads: Cell::new(0),
                        bindings: RefCell::new(Vec::new()),
                    };
                    let p = ulmdb_kv::object_graph::inspect_page_with_order(
                        &t,
                        k.at,
                        !matches!(k.role, Role::Kv(_)),
                        if matches!(k.role, Role::DuplicateValues(..)) {
                            65536
                        } else {
                            4080
                        },
                        if matches!(k.role, Role::Kv(_)) {
                            catalog_codec::OVERFLOW_VALUE_BYTES
                        } else {
                            4080
                        },
                        order.as_ref(),
                    )?;
                    (p, t.bindings.into_inner(), t.reads.get())
                };
                self.metrics.checked_reads += count;
                let ObjectKind::KvPage { level } = local.object.kind else {
                    unreachable!()
                };
                let mut maximum_value = local.maximum_value;
                let mut maximum_duplicate = 0;
                let mut entries = if level == 0 {
                    local.rows.len() as u64
                } else {
                    0
                };
                let mut minimum = local.minimum;
                let mut maximum = local.maximum;
                let mut edges = Vec::new();
                for child in local.children {
                    let target = Key {
                        at: child.reference,
                        role: k.role,
                    };
                    self.ensure(target, depth + 1)?;
                    let s = self.c.nodes.get(&target).unwrap();
                    let Summary::Kv {
                        maximum_value: child_maximum,
                        maximum_duplicate: child_duplicate,
                        entries: child_entries,
                        level: child_level,
                        minimum: lo,
                        maximum: hi,
                    } = &s.info.summary
                    else {
                        return Err(Error::Corrupt("graph KV child kind"));
                    };
                    maximum_value = maximum_value.max(*child_maximum);
                    maximum_duplicate = maximum_duplicate.max(*child_duplicate);
                    entries = entries
                        .checked_add(*child_entries)
                        .ok_or(Error::LimitExceeded)?;
                    use ulmdb_kv::ordering::Comparator;
                    if *child_level + 1 != level
                        || order.as_ref().is_some_and(|o| {
                            lo.as_ref().is_some_and(|a| {
                                child
                                    .lower
                                    .as_ref()
                                    .is_some_and(|b| o.compare(a, b).is_lt())
                            })
                        })
                        || order.as_ref().is_some_and(|o| {
                            hi.as_ref().is_some_and(|a| {
                                child
                                    .upper
                                    .as_ref()
                                    .is_some_and(|b| !o.compare(a, b).is_lt())
                            })
                        })
                    {
                        return Err(Error::Corrupt("audited child level/ancestor bounds"));
                    }
                    if let Some(lo) = lo
                        && minimum.as_ref().is_none_or(|x| {
                            order.as_ref().is_some_and(|o| o.compare(lo, x).is_lt())
                        })
                    {
                        minimum = Some(lo.clone());
                    }
                    if let Some(hi) = hi
                        && maximum.as_ref().is_none_or(|x| {
                            order.as_ref().is_some_and(|o| o.compare(hi, x).is_gt())
                        })
                    {
                        maximum = Some(hi.clone());
                    }
                    edges.push(target);
                }
                for payload in local.payloads {
                    let target = Key {
                        at: payload.object.reference,
                        role: Role::Payload,
                    };
                    self.ensure(target, depth + 1)?;
                    let s = self.c.nodes.get(&target).unwrap();
                    if s.info.object != payload.object {
                        return Err(Error::Corrupt("graph payload length changed"));
                    }
                    edges.push(target);
                }
                for (name, value, at, container) in local.rows {
                    match k.role {
                        Role::Duplicates(width, _, values) => {
                            let values_order = if values == 6 {
                                None
                            } else {
                                Some(crate::ordering::BoundComparator::bind(
                                    crate::ordering::OrderingSpec::decode(values)?,
                                    None,
                                )?)
                            };
                            let bucket = crate::duplicates::bucket::Bucket::decode_with(
                                &value,
                                width,
                                values_order
                                    .as_ref()
                                    .map(|o| o as &dyn ulmdb_kv::ordering::Comparator),
                            )?;
                            if let crate::duplicates::bucket::Bucket::Inline(inline) = bucket {
                                for i in 0..inline.count {
                                    maximum_duplicate =
                                        maximum_duplicate.max(inline.value(i).len());
                                }
                            }
                            if let crate::duplicates::bucket::Bucket::Tree { root, count } = bucket
                            {
                                let target = Key {
                                    at: root,
                                    role: Role::DuplicateValues(width, values),
                                };
                                self.ensure(target, depth + 1)?;
                                if !matches!(&self.c.nodes.get(&target).unwrap().info.summary, Summary::Kv { entries, .. } if *entries == count)
                                {
                                    return Err(Error::Corrupt(
                                        "incremental duplicate count mismatch",
                                    ));
                                }
                                if let Summary::Kv {
                                    maximum_duplicate: n,
                                    ..
                                } = &self.c.nodes.get(&target).unwrap().info.summary
                                {
                                    maximum_duplicate = maximum_duplicate.max(*n);
                                }
                                edges.push(target);
                            }
                            continue;
                        }
                        Role::DuplicateValues(width, _) => {
                            maximum_duplicate = maximum_duplicate.max(name.len());
                            if !value.is_empty() || width.is_some_and(|w| name.len() != w as usize)
                            {
                                return Err(Error::Corrupt(
                                    "incremental duplicate child width/value",
                                ));
                            }
                            continue;
                        }
                        _ => (),
                    }
                    if name.is_empty() {
                        let capacity = crate::CatalogCapacity::decode_marker(&value)?;
                        let object = ObjectId {
                            reference: at,
                            length: value.len() as u64,
                            kind: ObjectKind::CatalogProfile,
                        };
                        let end = at
                            .offset
                            .checked_add(object.length)
                            .ok_or(Error::LimitExceeded)?;
                        let parent_end = container
                            .reference
                            .offset
                            .checked_add(container.length)
                            .ok_or(Error::LimitExceeded)?;
                        if at.segment != container.reference.segment
                            || at.offset < container.reference.offset
                            || end > parent_end
                        {
                            return Err(Error::Corrupt("profile extent outside allocation"));
                        }
                        let allocation = self
                            .c
                            .view
                            .allocation_key(container.reference, container.length as usize)?;
                        let (_, binding) = self.read(at, value.len())?;
                        let target = Key {
                            at,
                            role: Role::Profile,
                        };
                        self.install(
                            target,
                            object,
                            allocation,
                            Vec::new(),
                            vec![binding],
                            Summary::Profile(capacity),
                        )?;
                        edges.push(target);
                        continue;
                    }
                    if name.len() > 255 {
                        return Err(Error::Corrupt("resident catalog name/count limit"));
                    }
                    let descriptor_bytes = value.len();
                    let db = catalog_codec::decode(&value)?;
                    let spec = RootSpec {
                        value_limit: catalog_codec::value_limit(&value)?,
                        name,
                        id: db.id,
                        kind: db.kind,
                        ordering: db.ordering,
                        root: db.root,
                    };
                    let object = ObjectId {
                        reference: at,
                        length: descriptor_bytes as u64,
                        kind: ObjectKind::CatalogDescriptor { database: db.id },
                    };
                    let target = Key {
                        at,
                        role: Role::Descriptor,
                    };
                    // A descriptor is an embedded logical object, never an invented allocation.
                    let end = at
                        .offset
                        .checked_add(descriptor_bytes as u64)
                        .ok_or(Error::LimitExceeded)?;
                    let ce = container
                        .reference
                        .offset
                        .checked_add(container.length)
                        .ok_or(Error::LimitExceeded)?;
                    if at.segment != container.reference.segment
                        || at.offset < container.reference.offset
                        || end > ce
                    {
                        return Err(Error::Corrupt("descriptor extent outside allocation"));
                    }
                    let allocation = self
                        .c
                        .view
                        .allocation_key(container.reference, container.length as usize)?;
                    let (_, binding) = self.read(at, descriptor_bytes)?;
                    self.install(
                        target,
                        object,
                        allocation,
                        Vec::new(),
                        vec![binding],
                        Summary::Descriptor(spec),
                    )?;
                    edges.push(target);
                }
                let allocation = self.c.view.allocation_key(local.object.reference, 4096)?;
                self.install(
                    k,
                    local.object,
                    allocation,
                    edges,
                    reads,
                    Summary::Kv {
                        maximum_value,
                        maximum_duplicate,
                        entries,
                        level,
                        minimum,
                        maximum,
                    },
                )?;
            }
            Role::V4 | Role::V6 => {
                let (local, reads, count) = {
                    let t = Trace {
                        view: &self.c.view,
                        budget: remaining,
                        reads: Cell::new(0),
                        bindings: RefCell::new(Vec::new()),
                    };
                    let p = ulmdb_prefix::object_graph::inspect_node(&t, k.at)?;
                    (p, t.bindings.into_inner(), t.reads.get())
                };
                self.metrics.checked_reads += count;
                let expected = if k.role == Role::V4 { 4 } else { 6 };
                if local.object.kind != (ObjectKind::RadixNode { family: expected }) {
                    return Err(Error::Corrupt("resident prefix descriptor family mismatch"));
                }
                let mut edges = Vec::new();
                let mut maximum_value = 0;
                for (at, prefix) in local.children {
                    let target = Key { at, role: k.role };
                    self.ensure(target, depth + 1)?;
                    if !matches!(&self.c.nodes.get(&target).unwrap().info.summary,Summary::Prefix(p,_)if *p==prefix)
                    {
                        return Err(Error::Corrupt("graph radix summary mismatch"));
                    }
                    if let Summary::Prefix(_, n) = self.c.nodes.get(&target).unwrap().info.summary {
                        maximum_value = maximum_value.max(n);
                    }
                    edges.push(target);
                }
                if let Some(p) = local.payload {
                    maximum_value = maximum_value.max((p.object.length - 16) as usize);
                    let target = Key {
                        at: p.object.reference,
                        role: Role::Payload,
                    };
                    self.ensure(target, depth + 1)?;
                    if self.c.nodes.get(&target).unwrap().info.object != p.object {
                        return Err(Error::Corrupt("graph radix payload mismatch"));
                    }
                    edges.push(target);
                }
                let allocation = self.c.view.allocation_key(local.object.reference, 96)?;
                self.install(
                    k,
                    local.object,
                    allocation,
                    edges,
                    reads,
                    Summary::Prefix(local.prefix, maximum_value),
                )?;
            }
            Role::Payload => {
                let (object, reads, count) = {
                    let t = Trace {
                        view: &self.c.view,
                        budget: remaining,
                        reads: Cell::new(0),
                        bindings: RefCell::new(Vec::new()),
                    };
                    let b = t.payload(k.at)?;
                    if b.len() > catalog_codec::OVERFLOW_VALUE_BYTES {
                        return Err(Error::Corrupt("resident prefix family/value limit"));
                    }
                    let p = ulmdb_core::object_graph::PayloadExtent::new(k.at, b.len())?;
                    (p.object, t.bindings.into_inner(), t.reads.get())
                };
                self.metrics.checked_reads += count;
                let allocation = self
                    .c
                    .view
                    .allocation_key(object.reference, object.length as usize)?;
                self.install(k, object, allocation, Vec::new(), reads, Summary::Payload)?;
            }
            Role::Descriptor | Role::Profile => {
                return Err(Error::Corrupt("descriptor requires checked page issuer"));
            }
        }
        Ok(())
    }
    fn install(
        &mut self,
        k: Key,
        object: ObjectId,
        allocation: VersionKey,
        edges: Vec<Key>,
        mut reads: Vec<object_graph::ReadBinding>,
        summary: Summary,
    ) -> Result<()> {
        if let Some(old) = self.c.nodes.get(&k) {
            if old.info.object != object
                || old.info.summary != summary
                || old.info.allocation != allocation
            {
                return Err(Error::Corrupt("graph identity collision"));
            }
            return Ok(());
        }
        reads.sort();
        reads.dedup();
        let versions = reads
            .iter()
            .map(|r| r.version)
            .chain([allocation])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let extra = match &summary {
            Summary::Kv {
                minimum, maximum, ..
            } => {
                minimum.as_ref().map_or(0, Vec::capacity)
                    + maximum.as_ref().map_or(0, Vec::capacity)
            }
            Summary::Descriptor(r) => r.name.capacity(),
            _ => 0,
        };
        let cost = std::mem::size_of::<Info>()
            + edges.capacity() * std::mem::size_of::<Key>()
            + reads.capacity() * (std::mem::size_of::<object_graph::ReadBinding>() + 64)
            + versions.capacity() * std::mem::size_of::<VersionKey>()
            + extra;
        self.c.info_bytes = self
            .c
            .info_bytes
            .checked_add(cost)
            .ok_or(Error::LimitExceeded)?;
        self.c.edge_count = self
            .c
            .edge_count
            .checked_add(edges.len())
            .ok_or(Error::LimitExceeded)?;
        self.c.nodes.insert(
            k,
            State {
                info: Arc::new(Info {
                    object,
                    allocation,
                    edges,
                    reads,
                    versions,
                    summary,
                    cost,
                }),
                incoming: 0,
                active: false,
            },
        );
        self.metrics.expanded_objects += 1;
        self.check_budget()
    }
}
