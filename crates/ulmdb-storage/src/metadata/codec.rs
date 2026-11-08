use super::{Index, Kind, MAX_DESCRIPTOR_BYTES, Node, NodeRef, Resolver, tree};
use crate::{Error, Result, SegmentEntry, manifest::number};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
impl NodeRef {
    pub fn encode(self) -> [u8; 64] {
        let mut b = [0; 64];
        b[..16].copy_from_slice(&self.origin.to_le_bytes());
        b[16..24].copy_from_slice(&self.offset.to_le_bytes());
        b[24..28].copy_from_slice(&self.length.to_le_bytes());
        b[32..64].copy_from_slice(&self.hash);
        b
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let r = Self {
            origin: u128::from_le_bytes(number(b, 0)?),
            offset: u64::from_le_bytes(number(b, 16)?),
            length: u32::from_le_bytes(number(b, 24)?),
            hash: number(b, 32)?,
        };
        if b.len() != 64
            || r.origin == 0
            || r.offset < 256
            || !r.offset.is_multiple_of(32)
            || ![96, 160].contains(&r.length)
            || r.offset
                .checked_add(u64::from(r.length))
                .is_none_or(|end| end > MAX_DESCRIPTOR_BYTES)
            || b[28..32] != [0; 4]
        {
            return Err(Error::Corrupt("invalid metadata node reference"));
        }
        Ok(r)
    }
}
pub(super) fn serialize(
    node: Arc<Node>,
    generation: u128,
    force: bool,
    blob: &mut Vec<u8>,
    count: &mut u32,
) -> Result<Arc<Node>> {
    if node.location.is_some() && !force {
        return Ok(node);
    }
    let kind = match &node.kind {
        Kind::Leaf(e) => Kind::Leaf(e.clone()),
        Kind::Branch(left, right) => Kind::Branch(
            serialize(Arc::clone(left), generation, false, blob, count)?,
            serialize(Arc::clone(right), generation, false, blob, count)?,
        ),
    };
    let len = if matches!(kind, Kind::Leaf(_)) {
        96
    } else {
        160
    };
    let offset = 256u64 + blob.len() as u64;
    if offset + len as u64 > MAX_DESCRIPTOR_BYTES {
        return Err(Error::LimitExceeded);
    }
    let mut b = [0; 160];
    b[..2].copy_from_slice(&(if len == 96 { 1u16 } else { 2 }).to_le_bytes());
    b[2..4].copy_from_slice(&1u16.to_le_bytes());
    b[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    b[8..16].copy_from_slice(&node.prefix.to_le_bytes());
    b[16] = node.depth;
    match &kind {
        Kind::Leaf(e) => {
            b[32..40].copy_from_slice(&e.id.to_le_bytes());
            b[40..48].copy_from_slice(&e.length.to_le_bytes());
            b[48..80].copy_from_slice(&e.hash);
            b[80..96].copy_from_slice(&e.cohort.to_le_bytes());
        }
        Kind::Branch(left, right) => {
            b[32..96].copy_from_slice(&left.location.unwrap().encode());
            b[96..160].copy_from_slice(&right.location.unwrap().encode());
        }
    }
    let location = NodeRef {
        origin: generation,
        offset,
        length: len as u32,
        hash: Sha256::digest(&b[..len]).into(),
    };
    blob.extend_from_slice(&b[..len]);
    *count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
    Ok(Arc::new(Node {
        prefix: node.prefix,
        depth: node.depth,
        count: node.count,
        location: Some(location),
        kind,
    }))
}
struct Decode<'a, 'r> {
    bytes: &'a [u8],
    generation: u128,
    slots: BTreeMap<u64, u32>,
    seen: BTreeMap<(u128, u64), u32>,
    postorder: Vec<u64>,
    resolver: Option<&'r mut Resolver>,
    budget: usize,
    entries: Vec<SegmentEntry>,
}
impl Decode<'_, '_> {
    fn node(&mut self, r: NodeRef, parent_depth: Option<u8>) -> Result<Arc<Node>> {
        let key = (r.origin, r.offset);
        let end = r
            .offset
            .checked_add(u64::from(r.length))
            .filter(|end| *end <= MAX_DESCRIPTOR_BYTES)
            .ok_or(Error::Corrupt("metadata node extent overflow/bound"))?;
        let prior_overlap =
            self.seen
                .range(..=key)
                .next_back()
                .is_some_and(|((origin, offset), length)| {
                    *origin == r.origin
                        && offset
                            .checked_add(u64::from(*length))
                            .is_none_or(|end| end > r.offset)
                });
        let next_overlap = self
            .seen
            .range(key..)
            .next()
            .is_some_and(|((origin, offset), _)| *origin == r.origin && *offset < end);
        if self.budget == 0 || prior_overlap || next_overlap {
            return Err(Error::Corrupt("metadata alias/overlap/cycle/visit bound"));
        }
        self.seen.insert(key, r.length);
        self.budget -= 1;
        let mut storage = [0; 160];
        if r.origin == self.generation {
            if self.slots.get(&r.offset) != Some(&r.length) {
                return Err(Error::Corrupt("metadata local node extent"));
            }
            let offset =
                usize::try_from(r.offset).map_err(|_| Error::Corrupt("metadata offset"))?;
            storage[..r.length as usize]
                .copy_from_slice(&self.bytes[offset..offset + r.length as usize]);
        } else {
            self.resolver
                .as_mut()
                .ok_or(Error::Corrupt("metadata origin resolver required"))?
                .read(r, &mut storage)?;
        }
        let b = &storage[..r.length as usize];
        if <[u8; 32]>::from(Sha256::digest(b)) != r.hash
            || b[2..4] != 1u16.to_le_bytes()
            || b[4..8] != r.length.to_le_bytes()
            || b[17..32].iter().any(|v| *v != 0)
        {
            return Err(Error::Corrupt("metadata node hash/header"));
        }
        let prefix = u64::from_le_bytes(number(b, 8)?);
        let depth = b[16];
        if depth > 64 || parent_depth.is_some_and(|d| depth <= d) {
            return Err(Error::Corrupt("metadata depth"));
        }
        let (kind, count) = match u16::from_le_bytes(number(b, 0)?) {
            1 if r.length == 96 && depth == 64 => {
                let e = SegmentEntry {
                    id: u64::from_le_bytes(number(b, 32)?),
                    length: u64::from_le_bytes(number(b, 40)?),
                    hash: number(b, 48)?,
                    cohort: u128::from_le_bytes(number(b, 80)?),
                };
                if prefix != e.id {
                    return Err(Error::Corrupt("metadata leaf ID"));
                }
                self.entries.push(e.clone());
                (Kind::Leaf(e), 1)
            }
            2 if r.length == 160 && depth < 64 && tree::prefix(prefix, depth) == prefix => {
                let left_ref = NodeRef::decode(&b[32..96])?;
                let right_ref = NodeRef::decode(&b[96..160])?;
                for child in [left_ref, right_ref] {
                    if (child.origin == r.origin
                        && child
                            .offset
                            .checked_add(u64::from(child.length))
                            .is_none_or(|end| end > r.offset))
                        || (r.origin != self.generation && child.origin == self.generation)
                    {
                        return Err(Error::Corrupt(
                            "metadata immutable/backward child reference",
                        ));
                    }
                }
                let left = self.node(left_ref, Some(depth))?;
                let right = self.node(right_ref, Some(depth))?;
                if tree::prefix(left.prefix, depth) != prefix
                    || tree::prefix(right.prefix, depth) != prefix
                    || tree::bit(left.prefix, depth)
                    || !tree::bit(right.prefix, depth)
                    || (left.prefix ^ right.prefix).leading_zeros() != u32::from(depth)
                {
                    return Err(Error::Corrupt("metadata Patricia geometry"));
                }
                let count = left
                    .count
                    .checked_add(right.count)
                    .ok_or(Error::LimitExceeded)?;
                (Kind::Branch(left, right), count)
            }
            _ => return Err(Error::Corrupt("metadata node tag/length/depth")),
        };
        if r.origin == self.generation {
            self.postorder.push(r.offset);
        }
        Ok(Arc::new(Node {
            prefix,
            depth,
            count,
            location: Some(r),
            kind,
        }))
    }
}
pub(super) fn decode(
    bytes: &[u8],
    generation: u128,
    count: usize,
    local: u32,
    root: &[u8],
    resolver: Option<&mut Resolver>,
) -> Result<(Index, Vec<SegmentEntry>)> {
    if bytes.len() < 256 || bytes.len() as u64 > MAX_DESCRIPTOR_BYTES || count >= 1_048_576 {
        return Err(Error::Corrupt("indexed descriptor extent/count"));
    }
    let max = count.saturating_mul(2).saturating_sub(1);
    if local as usize > max {
        return Err(Error::Corrupt("metadata local count exceeds tree"));
    }
    let mut slots = BTreeMap::new();
    let mut offset = 256;
    while offset < bytes.len() {
        let len = u32::from_le_bytes(number(bytes, offset + 4)?);
        if ![96, 160].contains(&len) || offset + len as usize > bytes.len() {
            return Err(Error::Corrupt("metadata local blob extent"));
        }
        slots.insert(offset as u64, len);
        offset += len as usize;
        if slots.len() > local as usize {
            return Err(Error::Corrupt("metadata local blob count"));
        }
    }
    if slots.len() != local as usize {
        return Err(Error::Corrupt("metadata local blob count"));
    }
    let expected = slots.keys().copied().collect::<Vec<_>>();
    let mut decoder = Decode {
        bytes,
        generation,
        slots,
        seen: BTreeMap::new(),
        postorder: Vec::new(),
        resolver,
        budget: max,
        entries: Vec::with_capacity(count),
    };
    let root = if count == 0 {
        if root != [0; 64] || local != 0 {
            return Err(Error::Corrupt("nonempty metadata root for empty inventory"));
        }
        None
    } else {
        let r = NodeRef::decode(root)?;
        if r.origin != generation {
            return Err(Error::Corrupt("selected metadata root must be local"));
        }
        Some(decoder.node(r, None)?)
    };
    if decoder.entries.len() != count || decoder.postorder != expected {
        return Err(Error::Corrupt("metadata count or exact postorder mismatch"));
    }
    super::validate_entries(&decoder.entries)?;
    Ok((
        Index {
            generation,
            root,
            blob: bytes[256..].into(),
            local_nodes: local,
            origins: OnceLock::new(),
            inventory_stamp: 0,
            certified_table: None,
            own_layout: None,
        },
        decoder.entries,
    ))
}
