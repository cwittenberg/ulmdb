use super::NodeRef;
use crate::SegmentEntry;
use std::sync::Arc;
#[derive(Clone, Debug)]
pub(super) struct Node {
    pub prefix: u64,
    pub depth: u8,
    pub count: usize,
    pub location: Option<NodeRef>,
    pub kind: Kind,
}
#[derive(Clone, Debug)]
pub(super) enum Kind {
    Leaf(SegmentEntry),
    Branch(Arc<Node>, Arc<Node>),
}
pub(super) fn equal(a: &SegmentEntry, b: &SegmentEntry) -> bool {
    a.id == b.id && a.length == b.length && a.hash == b.hash && a.cohort == b.cohort
}
pub(super) fn prefix(id: u64, depth: u8) -> u64 {
    if depth == 0 {
        0
    } else {
        id & (u64::MAX << (64 - depth))
    }
}
pub(super) fn bit(id: u64, depth: u8) -> bool {
    id & (1u64 << (63 - depth)) != 0
}
fn leaf(entry: SegmentEntry) -> Arc<Node> {
    Arc::new(Node {
        prefix: entry.id,
        depth: 64,
        count: 1,
        location: None,
        kind: Kind::Leaf(entry),
    })
}
fn branch(depth: u8, left: Arc<Node>, right: Arc<Node>) -> Arc<Node> {
    Arc::new(Node {
        prefix: prefix(left.prefix, depth),
        depth,
        count: left.count + right.count,
        location: None,
        kind: Kind::Branch(left, right),
    })
}
pub(super) fn build(entries: &[SegmentEntry]) -> Option<Arc<Node>> {
    if entries.is_empty() {
        return None;
    }
    if entries.len() == 1 {
        return Some(leaf(entries[0].clone()));
    }
    let depth = (entries[0].id ^ entries[entries.len() - 1].id).leading_zeros() as u8;
    let split = entries.partition_point(|e| !bit(e.id, depth));
    Some(branch(
        depth,
        build(&entries[..split]).unwrap(),
        build(&entries[split..]).unwrap(),
    ))
}
pub(super) fn insert(root: Option<Arc<Node>>, entry: SegmentEntry) -> Option<Arc<Node>> {
    let Some(node) = root else {
        return Some(leaf(entry));
    };
    let depth = ((node.prefix ^ entry.id).leading_zeros() as u8).min(node.depth);
    if depth < node.depth {
        let new = leaf(entry);
        return Some(if bit(new.prefix, depth) {
            branch(depth, node, new)
        } else {
            branch(depth, new, node)
        });
    }
    match &node.kind {
        Kind::Leaf(old) => Some(if equal(old, &entry) {
            Arc::clone(&node)
        } else {
            leaf(entry)
        }),
        Kind::Branch(left, right) => {
            let changed_right = bit(entry.id, node.depth);
            let old = if changed_right { right } else { left };
            let changed = insert(Some(Arc::clone(old)), entry).unwrap();
            if Arc::ptr_eq(old, &changed) {
                return Some(node);
            }
            Some(if changed_right {
                branch(node.depth, Arc::clone(left), changed)
            } else {
                branch(node.depth, changed, Arc::clone(right))
            })
        }
    }
}
pub(super) fn delete(root: Option<Arc<Node>>, id: u64) -> Option<Arc<Node>> {
    let node = root?;
    if prefix(id, node.depth) != node.prefix {
        return Some(node);
    }
    match &node.kind {
        Kind::Leaf(_) => None,
        Kind::Branch(left, right) => {
            let changed_right = bit(id, node.depth);
            let old = if changed_right { right } else { left };
            let Some(changed) = delete(Some(Arc::clone(old)), id) else {
                return Some(Arc::clone(if changed_right { left } else { right }));
            };
            if Arc::ptr_eq(old, &changed) {
                return Some(node);
            }
            Some(if changed_right {
                branch(node.depth, Arc::clone(left), changed)
            } else {
                branch(node.depth, changed, Arc::clone(right))
            })
        }
    }
}
pub(super) fn matches(root: &Option<Arc<Node>>, entries: &[SegmentEntry]) -> bool {
    fn check(node: &Node, entries: &[SegmentEntry], position: &mut usize) -> bool {
        match &node.kind {
            Kind::Leaf(entry) => {
                let okay = entries.get(*position).is_some_and(|e| equal(e, entry));
                *position += 1;
                okay
            }
            Kind::Branch(left, right) => {
                check(left, entries, position) && check(right, entries, position)
            }
        }
    }
    root.as_ref().map_or(entries.is_empty(), |node| {
        node.count == entries.len() && check(node, entries, &mut 0)
    })
}
