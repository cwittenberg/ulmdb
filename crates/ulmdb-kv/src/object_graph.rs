//! Additional cold typed walk. The existing full audit runs first, unchanged.
use crate::page::{MAX_DEPTH, Page};
use ulmdb_core::{
    object_graph::{Edge, EdgeKind, ObjectId, ObjectKind, PayloadExtent},
    storage::{Error, ReadStore, Reference, Result},
};

/// Physical reference fields of one locally checked page. This is a relocation
/// description, never a graph, ordering or reclamation certificate.
#[derive(Clone, Debug)]
pub struct RelocationPage {
    pub links: Vec<RelocationLink>,
    /// Inline leaf values, requested only for an engine's typed bucket codec.
    pub inline_values: Vec<(usize, usize)>,
}
#[derive(Clone, Copy, Debug)]
pub struct RelocationLink {
    pub offset: usize,
    pub reference: Reference,
    pub kind: RelocationKind,
}
#[derive(Clone, Copy, Debug)]
pub enum RelocationKind { Child, Key, Value }

/// Inspect one page without copying or borrowing ordinary value bodies. All
/// offsets refer to that exact encoded page, including compact/V3 geometry.
/// No application comparator is called and no ordering is invented.
pub fn inspect_relocation_page<S: ReadStore>(
    store: &S, at: Reference, inline_values: bool,
) -> Result<RelocationPage> {
    crate::page::validate_page_metadata::<S, crate::ordering::Bytewise>(store, at, None)?;
    let page = Page::load(store, at)?;
    let mut result = RelocationPage { links: Vec::new(), inline_values: Vec::new() };
    if page.level != 0 {
        for index in 0..=page.count {
            result.links.push(RelocationLink {
                offset: page.child_offset(index)?, reference: page.child(store, index)?,
                kind: RelocationKind::Child,
            });
        }
    }
    for index in 0..page.count {
        let (cell, value_length) = page.cell_metadata(store, index, false)?;
        let (flags, header, key_length) = page.encoding(&cell)?;
        let key_bytes = if flags & 1 != 0 {
            let offset = cell.start + header;
            result.links.push(RelocationLink {
                offset, reference: Reference::decode(&page.bytes[offset..offset+16])?,
                kind: RelocationKind::Key,
            });
            16
        } else { key_length };
        if page.level == 0 {
            let offset = cell.start + header + key_bytes;
            if flags & 2 != 0 {
                result.links.push(RelocationLink {
                    offset, reference: Reference::decode(&page.bytes[offset..offset+16])?,
                    kind: RelocationKind::Value,
                });
            } else if inline_values {
                result.inline_values.push((offset, value_length));
            }
        }
    }
    Ok(result)
}
#[derive(Clone, Copy, Debug)]
pub enum Event<'a> {
    Object(ObjectId),
    Edge(Edge),
    Payload(PayloadExtent),
    Leaf {
        page: ObjectId,
        index: u16,
        value_reference: Reference,
        value_container: ObjectId,
        key: &'a [u8],
        value: &'a [u8],
    },
}
fn page_id(size: usize, at: Reference, level: u16) -> ObjectId {
    ObjectId {
        reference: at,
        length: size as u64,
        kind: ObjectKind::KvPage { level },
    }
}
/// Every page/edge, including first children, overflow separators and values.
/// Visitors can see a partial event stream on failure; only complete success is
/// a checked description. No graph/certificate authority follows from events.
/// This prototype pays a full existing audit PLUS a second checked decoder walk.
pub fn visit_tree<S: ReadStore, V: FnMut(Event<'_>) -> Result<()>>(
    store: &S,
    root: Reference,
    max_pages: u64,
    visitor: V,
) -> Result<crate::TreeAudit> {
    let audit = crate::audit_tree(store, root, max_pages)?;
    emit_tree(store, root, max_pages, visitor, audit)
}
/// The same checked typed walk with structural safety only. It does not invoke
/// application comparisons or certify an unavailable custom ordering.
pub fn visit_tree_structure<S: ReadStore, V: FnMut(Event<'_>) -> Result<()>>(
    store: &S,
    root: Reference,
    max_pages: u64,
    visitor: V,
) -> Result<crate::TreeAudit> {
    let audit = crate::audit_tree_structure(store, root, max_pages)?;
    emit_tree(store, root, max_pages, visitor, audit)
}
/// Typed walk under a supplied ordering. Callers can use persisted builtin
/// descriptions without invoking any process-local application comparator.
pub fn visit_tree_with<
    S: ReadStore,
    V: FnMut(Event<'_>) -> Result<()>,
    C: crate::ordering::Comparator + ?Sized,
>(
    store: &S,
    root: Reference,
    max_pages: u64,
    order: &C,
    visitor: V,
) -> Result<crate::TreeAudit> {
    let audit = crate::audit_tree_with(store, root, max_pages, order)?;
    emit_tree(store, root, max_pages, visitor, audit)
}
fn emit_tree<S: ReadStore, V: FnMut(Event<'_>) -> Result<()>>(
    store: &S,
    root: Reference,
    max_pages: u64,
    mut visitor: V,
    audit: crate::TreeAudit,
) -> Result<crate::TreeAudit> {
    if root.is_null() {
        return Ok(audit);
    }
    let mut stack: Vec<(Reference, Page<'_>, usize)> = Vec::new();
    stack
        .try_reserve_exact(MAX_DEPTH as usize)
        .map_err(|_| Error::LimitExceeded)?;
    let mut next = Some(root);
    let mut visits = 0u64;
    loop {
        if let Some(at) = next.take() {
            if visits == max_pages {
                return Err(Error::LimitExceeded);
            }
            visits += 1;
            let page = Page::load(store, at)?;
            let id = page_id(store.page_size(), at, page.level);
            visitor(Event::Object(id))?;
            for index in 0..page.count {
                let cell = page.cell(store, index)?;
                let (flags, header, inline_key_bytes) = page.encoding(&cell)?;
                let key_bytes = if flags & 1 != 0 {
                    let payload = PayloadExtent::new(
                        Reference::decode(&cell.raw[header..header + 16])?,
                        cell.key.len(),
                    )?;
                    visitor(Event::Payload(payload))?;
                    visitor(Event::Edge(Edge {
                        from: id,
                        to: payload.object,
                        kind: EdgeKind::OverflowKey { cell: index as u16 },
                    }))?;
                    16
                } else {
                    inline_key_bytes
                };
                if page.level == 0 && flags & 2 != 0 {
                    let start = header + key_bytes;
                    let payload = PayloadExtent::new(
                        Reference::decode(&cell.raw[start..start + 16])?,
                        cell.value.len(),
                    )?;
                    visitor(Event::Payload(payload))?;
                    visitor(Event::Edge(Edge {
                        from: id,
                        to: payload.object,
                        kind: EdgeKind::OverflowValue { cell: index as u16 },
                    }))?;
                }
                if page.level == 0 {
                    let (value_reference, value_container) = if flags & 2 != 0 {
                        let start = header + key_bytes;
                        let payload = PayloadExtent::new(
                            Reference::decode(&cell.raw[start..start + 16])?,
                            cell.value.len(),
                        )?;
                        (payload.data, payload.object)
                    } else {
                        let offset = at
                            .offset
                            .checked_add((cell.start + header + key_bytes) as u64)
                            .ok_or(Error::Corrupt("typed inline value offset"))?;
                        (
                            Reference {
                                segment: at.segment,
                                offset,
                            },
                            id,
                        )
                    };
                    visitor(Event::Leaf {
                        page: id,
                        index: index as u16,
                        value_reference,
                        value_container,
                        key: cell.key,
                        value: cell.value,
                    })?;
                }
            }
            if page.level != 0 {
                if stack.len() == MAX_DEPTH as usize {
                    return Err(Error::Corrupt("typed graph path bound"));
                }
                stack.push((at, page, 0));
            }
        }
        let Some((at, page, index)) = stack.last_mut() else {
            break;
        };
        if *index > page.count {
            stack.pop();
            continue;
        }
        let child = page.child(store, *index)?;
        let from = page_id(store.page_size(), *at, page.level);
        let to = page_id(store.page_size(), child, page.level - 1);
        visitor(Event::Edge(Edge {
            from,
            to,
            kind: EdgeKind::KvChild {
                ordinal: *index as u16,
            },
        }))?;
        *index += 1;
        next = Some(child);
    }
    Ok(audit)
}

/// One checked page only. No descendant walk; callers must validate child summaries.
#[derive(Clone, Debug)]
pub struct Child {
    pub reference: Reference,
    pub lower: Option<Vec<u8>>,
    pub upper: Option<Vec<u8>>,
}
#[derive(Clone, Debug)]
pub struct Local {
    pub maximum_value: usize,
    pub minimum_key_bytes: usize,
    pub maximum_key_bytes: usize,
    pub records: usize,
    pub object: ObjectId,
    pub children: Vec<Child>,
    pub payloads: Vec<PayloadExtent>,
    pub rows: Vec<(Vec<u8>, Vec<u8>, Reference, ObjectId)>,
    pub minimum: Option<Vec<u8>>,
    pub maximum: Option<Vec<u8>>,
}
pub fn inspect_page<S: ReadStore>(
    store: &S,
    at: Reference,
    catalog: bool,
    max_field: usize,
) -> Result<Local> {
    inspect_page_with_limits(store, at, catalog, max_field, max_field)
}
/// One checked page with separately declared key/value limits.
pub fn inspect_page_with_limits<S: ReadStore>(
    store: &S,
    at: Reference,
    catalog: bool,
    max_key: usize,
    max_value: usize,
) -> Result<Local> {
    inspect_page_with_order::<S, crate::ordering::Bytewise>(
        store,
        at,
        catalog,
        max_key,
        max_value,
        Some(&crate::ordering::Bytewise),
    )
}
/// Bounded one-node inspection. None deliberately checks structure without
/// invoking or inventing an unavailable process-local comparator.
pub fn inspect_page_with_order<S: ReadStore, C: crate::ordering::Comparator + ?Sized>(
    store: &S,
    at: Reference,
    catalog: bool,
    max_key: usize,
    max_value: usize,
    order: Option<&C>,
) -> Result<Local> {
    inspect_page_bounded(store, at, catalog, max_key, max_value, order, usize::MAX)
}
/// Same local decoder with a conservative pre-allocation bound for owned
/// summaries. Borrowed page/payload bytes remain charged by the caller's store.
#[allow(clippy::too_many_arguments)]
pub fn inspect_page_bounded<S: ReadStore, C: crate::ordering::Comparator + ?Sized>(
    store: &S,
    at: Reference,
    catalog: bool,
    max_key: usize,
    max_value: usize,
    order: Option<&C>,
    metadata_bytes: usize,
) -> Result<Local> {
    if at.segment == 0 || !at.offset.is_multiple_of(store.page_size() as u64) {
        return Err(Error::Corrupt("invalid audited page reference"));
    }
    crate::page::validate_page_metadata(store, at, order)?;
    let page = Page::load(store, at)?;
    let object = page_id(store.page_size(), at, page.level);
    // Count all copied keys, path bounds, row bytes and vector bookkeeping
    // before allocating summaries. Validation itself borrows only store pins.
    if metadata_bytes != usize::MAX {
        let mut metadata = 512usize;
        for index in 0..page.count {
            let (cell, value_length) = page.cell_metadata(store, index, catalog)?;
            let keys = cell.key.len().checked_mul(4).ok_or(Error::LimitExceeded)?;
            let value = if catalog && page.level == 0 {
                value_length
            } else {
                0
            };
            metadata = metadata
                .checked_add(keys)
                .and_then(|n| n.checked_add(value))
                .and_then(|n| n.checked_add(512))
                .ok_or(Error::LimitExceeded)?;
            if metadata > metadata_bytes {
                return Err(Error::LimitExceeded);
            }
        }
        if metadata > metadata_bytes {
            return Err(Error::LimitExceeded);
        }
    }
    let mut keys = Vec::with_capacity(if page.level == 0 { 0 } else { page.count });
    let mut maximum_value = 0;
    let mut minimum_key_bytes = usize::MAX;
    let mut maximum_key_bytes = 0;
    let mut leaf_minimum = None;
    let mut leaf_maximum = None;
    let mut payloads = Vec::new();
    let mut rows = Vec::new();
    for index in 0..page.count {
        let (cell, value_length) = page.cell_metadata(store, index, catalog)?;
        minimum_key_bytes = minimum_key_bytes.min(cell.key.len());
        maximum_key_bytes = maximum_key_bytes.max(cell.key.len());
        if page.level == 0 {
            maximum_value = maximum_value.max(value_length);
        }
        if cell.key.len() > max_key || value_length > max_value {
            return Err(Error::Corrupt(
                "resident indexed object exceeds declared limit",
            ));
        }
        let (flags, header, inline_key_bytes) = page.encoding(&cell)?;
        let key_bytes = if flags & 1 != 0 {
            let p = PayloadExtent::new(
                Reference::decode(&cell.raw[header..header + 16])?,
                cell.key.len(),
            )?;
            payloads.push(p);
            16
        } else {
            inline_key_bytes
        };
        let mut value_container = object;
        let mut value_reference = Reference {
            segment: at.segment,
            offset: at
                .offset
                .checked_add((cell.start + header + key_bytes) as u64)
                .ok_or(Error::Corrupt("typed inline value offset"))?,
        };
        if page.level == 0 && flags & 2 != 0 {
            let start = header + key_bytes;
            let p = PayloadExtent::new(
                Reference::decode(&cell.raw[start..start + 16])?,
                value_length,
            )?;
            value_container = p.object;
            value_reference = p.data;
            payloads.push(p);
        }
        if catalog && page.level == 0 {
            rows.push((
                cell.key.to_vec(),
                cell.value.to_vec(),
                value_reference,
                value_container,
            ));
        }
        if page.level == 0 {
            if index == 0 {
                leaf_minimum = Some(cell.key.to_vec());
            }
            if index + 1 == page.count {
                leaf_maximum = Some(cell.key.to_vec());
            }
        } else {
            keys.push(cell.key.to_vec());
        }
    }
    let mut children = Vec::new();
    if page.level != 0 {
        for i in 0..=page.count {
            children.push(Child {
                reference: page.child(store, i)?,
                lower: if i == 0 {
                    None
                } else {
                    Some(keys[i - 1].clone())
                },
                upper: keys.get(i).cloned(),
            });
        }
    }
    Ok(Local {
        maximum_value,
        minimum_key_bytes,
        maximum_key_bytes,
        records: if page.level == 0 { page.count } else { 0 },
        object,
        children,
        payloads,
        rows,
        minimum: if page.level == 0 {
            leaf_minimum
        } else {
            keys.first().cloned()
        },
        maximum: if page.level == 0 {
            leaf_maximum
        } else {
            keys.last().cloned()
        },
    })
}

mod logical;
pub use logical::{LogicalPage, LogicalRow, SmallValue, inspect_logical_page};
