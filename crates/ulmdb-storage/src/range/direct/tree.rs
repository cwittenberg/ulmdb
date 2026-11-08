//! Persistent ID Patricia paths. Untouched Arc nodes retain exact physical refs.
use super::reference::PhysicalRef;
use crate::{Error, Result, SegmentEntry};
use sha2::{Digest, Sha256};
use std::sync::Arc;
pub(super) const DOMAIN: &[u8] = b"ULMDB/range-locator/node/v6\0";
#[derive(Clone, Debug)]
pub(super) struct Node {
    pub prefix: u64,
    pub depth: u8,
    pub count: usize,
    pub location: Option<PhysicalRef>,
    pub kind: Kind,
}
#[derive(Clone, Debug)]
pub(super) enum Kind {
    Leaf {
        entry: SegmentEntry,
        data: PhysicalRef,
    },
    Branch(Arc<Node>, Arc<Node>),
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
fn leaf(entry: SegmentEntry, data: PhysicalRef) -> Arc<Node> {
    Arc::new(Node {
        prefix: entry.id,
        depth: 64,
        count: 1,
        location: None,
        kind: Kind::Leaf { entry, data },
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
pub(super) fn insert(
    root: Option<Arc<Node>>,
    entry: SegmentEntry,
    data: PhysicalRef,
) -> Result<Arc<Node>> {
    let Some(node) = root else {
        return Ok(leaf(entry, data));
    };
    let depth = ((node.prefix ^ entry.id).leading_zeros() as u8).min(node.depth);
    if depth < node.depth {
        let new = leaf(entry, data);
        return Ok(if bit(new.prefix, depth) {
            branch(depth, node, new)
        } else {
            branch(depth, new, node)
        });
    }
    match &node.kind {
        Kind::Leaf { .. } => Err(Error::Corrupt("range segment ID rebinding")),
        Kind::Branch(left, right) => Ok(if bit(entry.id, node.depth) {
            branch(
                node.depth,
                Arc::clone(left),
                insert(Some(Arc::clone(right)), entry, data)?,
            )
        } else {
            branch(
                node.depth,
                insert(Some(Arc::clone(left)), entry, data)?,
                Arc::clone(right),
            )
        }),
    }
}
pub(super) fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(bytes);
    h.finalize().into()
}
#[derive(Default)]
pub(super) struct Encoded {
    pub bytes: Vec<u8>,
    pub written: u64,
    pub visited: u64,
}
pub(super) fn serialize(
    node: Arc<Node>,
    file: [u8; 16],
    base: u64,
    out: &mut Encoded,
) -> Result<Arc<Node>> {
    out.visited = out.visited.checked_add(1).ok_or(Error::LimitExceeded)?;
    if node.location.is_some() {
        return Ok(node);
    }
    let kind = match &node.kind {
        Kind::Leaf { entry, data } => Kind::Leaf {
            entry: entry.clone(),
            data: *data,
        },
        Kind::Branch(l, r) => Kind::Branch(
            serialize(Arc::clone(l), file, base, out)?,
            serialize(Arc::clone(r), file, base, out)?,
        ),
    };
    let length = if matches!(kind, Kind::Leaf { .. }) {
        96
    } else {
        160
    };
    let mut b = [0; 160];
    b[..8].copy_from_slice(b"ULMDBL06");
    b[8] = if length == 96 { 0 } else { 1 };
    b[9] = node.depth;
    b[16..24].copy_from_slice(&node.prefix.to_le_bytes());
    b[24..32].copy_from_slice(&(node.count as u64).to_le_bytes());
    match &kind {
        Kind::Leaf { data, .. } => b[32..96].copy_from_slice(&data.encode()),
        Kind::Branch(l, r) => {
            b[32..96].copy_from_slice(&l.location.unwrap().encode());
            b[96..160].copy_from_slice(&r.location.unwrap().encode());
        }
    }
    let location = PhysicalRef {
        file,
        offset: base
            .checked_add(out.bytes.len() as u64)
            .ok_or(Error::LimitExceeded)?,
        length: length as u64,
        hash: hash(&b[..length]),
    };
    out.bytes.extend_from_slice(&b[..length]);
    out.written += 1;
    Ok(Arc::new(Node {
        prefix: node.prefix,
        depth: node.depth,
        count: node.count,
        location: Some(location),
        kind,
    }))
}
