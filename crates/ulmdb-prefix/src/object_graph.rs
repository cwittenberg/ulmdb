//! Additional cold typed traversal using the unchanged generic checked decoder.
//! Events are observations, not source, allocation or retirement certificates.
use crate::node::{Node, SIZE};
use ulmdb_core::{
    Address,
    object_graph::{Edge, EdgeKind, ObjectId, ObjectKind, PayloadExtent},
    storage::{Error, ReadStore, Reference, Result},
};
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Object(ObjectId),
    Edge(Edge),
    Payload(PayloadExtent),
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    pub nodes: u64,
    pub values: u64,
}
fn id(at: Reference, node: Node) -> ObjectId {
    ObjectId {
        reference: at,
        length: SIZE as u64,
        kind: ObjectKind::RadixNode {
            family: match node.prefix.network() {
                Address::V4(_) => 4,
                Address::V6(_) => 6,
            },
        },
    }
}
/// Includes valueless branch nodes and both children. The immutable ReadStore
/// contract must hold throughout. A callback may observe a partial stream on
/// failure; no checked graph may be published until the whole call succeeds.
pub fn visit_tree<S: ReadStore, V: FnMut(Event) -> Result<()>>(
    store: &S,
    root: Reference,
    max_nodes: u64,
    mut visitor: V,
) -> Result<Counts> {
    if root.is_null() {
        return Ok(Counts::default());
    }
    let mut stack = Vec::new();
    stack
        .try_reserve_exact(129)
        .map_err(|_| Error::LimitExceeded)?;
    stack.push((root, Node::read(store, root)?));
    let mut counts = Counts::default();
    while let Some((at, node)) = stack.pop() {
        if counts.nodes == max_nodes {
            return Err(Error::LimitExceeded);
        }
        counts.nodes += 1;
        let from = id(at, node);
        visitor(Event::Object(from))?;
        // Match whole-family Cursor's eager child validation before payload.
        for side in [1, 0] {
            if let Some(child) = node.child(store, side)? {
                let reference = node.children[side];
                visitor(Event::Edge(Edge {
                    from,
                    to: id(reference, child),
                    kind: EdgeKind::RadixChild { side: side as u8 },
                }))?;
                if stack.len() == 129 {
                    return Err(Error::Corrupt("typed radix path bound"));
                }
                stack.push((reference, child));
            }
        }
        if !node.value.is_null() {
            let bytes = store.payload(node.value)?;
            let payload = PayloadExtent::new(node.value, bytes.len())?;
            visitor(Event::Payload(payload))?;
            visitor(Event::Edge(Edge {
                from,
                to: payload.object,
                kind: EdgeKind::RadixValue,
            }))?;
            counts.values = counts.values.checked_add(1).ok_or(Error::LimitExceeded)?;
        }
    }
    Ok(counts)
}

#[derive(Clone, Debug)]
pub struct Local {
    pub object: ObjectId,
    pub prefix: ulmdb_core::Prefix,
    pub children: Vec<(Reference, ulmdb_core::Prefix)>,
    pub payload: Option<PayloadExtent>,
}
/// One node plus the eager checked child headers, never a descendant traversal.
pub fn inspect_node<S: ReadStore>(store: &S, at: Reference) -> Result<Local> {
    let node = Node::read(store, at)?;
    let mut children = Vec::new();
    for side in [1, 0] {
        if let Some(child) = node.child(store, side)? {
            children.push((node.children[side], child.prefix));
        }
    }
    let payload = if node.value.is_null() {
        None
    } else {
        Some(PayloadExtent::new(
            node.value,
            store.payload_length(node.value)?,
        )?)
    };
    Ok(Local {
        object: id(at, node),
        prefix: node.prefix,
        children,
        payload,
    })
}
