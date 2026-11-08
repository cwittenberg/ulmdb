use super::ChunkProvenance;
use crate::{Error, Result};
use std::sync::Arc;
// Fifteen branch nibbles plus the leaf nibble: at most SIXTEEN copied nodes.
#[derive(Clone)]
enum Node {
    Branch([Option<Arc<Node>>; 16]),
    Leaf([Option<Arc<ChunkProvenance>>; 16]),
}
#[derive(Clone, Default)]
pub struct ProvenanceDirectory {
    root: Option<Arc<Node>>,
    count: usize,
}
impl ProvenanceDirectory {
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn get(&self, id: u64) -> Option<&Arc<ChunkProvenance>> {
        let mut node = self.root.as_deref()?;
        for depth in 0..15 {
            let Node::Branch(children) = node else {
                return None;
            };
            node = children[((id >> (60 - depth * 4)) & 15) as usize].as_deref()?;
        }
        let Node::Leaf(values) = node else {
            return None;
        };
        values[(id & 15) as usize].as_ref()
    }
    /// Returns a new immutable root. Existing directories/cuts remain unchanged.
    pub fn insert(&self, value: Arc<ChunkProvenance>) -> Result<Self> {
        let id = value.identity().chunk;
        if self.count >= 1_048_575 {
            return Err(Error::LimitExceeded);
        }
        if self.get(id).is_some() {
            return Err(Error::InvalidInput("sealed chunk already registered"));
        }
        Ok(Self {
            root: Some(insert(self.root.as_deref(), 0, id, value)),
            count: self.count + 1,
        })
    }
    pub fn iter(&self) -> ProvenanceIter<'_> {
        let mut it = ProvenanceIter {
            stack: [None; 16],
            depth: 0,
        };
        if let Some(root) = self.root.as_deref() {
            it.stack[0] = Some((root, 0));
            it.depth = 1;
        }
        it
    }
}
fn insert(old: Option<&Node>, depth: usize, id: u64, value: Arc<ChunkProvenance>) -> Arc<Node> {
    if depth == 15 {
        let mut values = match old {
            Some(Node::Leaf(v)) => v.clone(),
            _ => std::array::from_fn(|_| None),
        };
        values[(id & 15) as usize] = Some(value);
        return Arc::new(Node::Leaf(values));
    }
    let mut children = match old {
        Some(Node::Branch(c)) => c.clone(),
        _ => std::array::from_fn(|_| None),
    };
    let at = ((id >> (60 - depth * 4)) & 15) as usize;
    children[at] = Some(insert(children[at].as_deref(), depth + 1, id, value));
    Arc::new(Node::Branch(children))
}
pub struct ProvenanceIter<'a> {
    stack: [Option<(&'a Node, usize)>; 16],
    depth: usize,
}
impl<'a> Iterator for ProvenanceIter<'a> {
    type Item = &'a Arc<ChunkProvenance>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.depth > 0 {
            let (node, next) = self.stack[self.depth - 1].as_mut()?;
            if *next == 16 {
                self.depth -= 1;
                continue;
            }
            let index = *next;
            *next += 1;
            match node {
                Node::Leaf(values) => {
                    if let Some(v) = &values[index] {
                        return Some(v);
                    }
                }
                Node::Branch(children) => {
                    if let Some(child) = children[index].as_deref() {
                        self.stack[self.depth] = Some((child, 0));
                        self.depth += 1;
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid::provenance::{ActiveProvenance, ChunkIdentity};
    fn chunk(id: u64) -> Arc<ChunkProvenance> {
        ActiveProvenance::new(ChunkIdentity {
            uuid: [1; 16],
            epoch: 1,
            chunk: id,
            logical_generation: 1,
            capacity: 65536,
        })
        .unwrap()
        .seal(4096, 0, 0)
        .unwrap()
    }
    fn changed(old: Option<&Arc<Node>>, new: &Arc<Node>) -> usize {
        if old.is_some_and(|o| Arc::ptr_eq(o, new)) {
            return 0;
        }
        1 + match new.as_ref() {
            Node::Leaf(_) => 0,
            Node::Branch(children) => children
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    n.as_ref().map(|n| {
                        let prior = match old.map(Arc::as_ref) {
                            Some(Node::Branch(c)) => c[i].as_ref(),
                            _ => None,
                        };
                        changed(prior, n)
                    })
                })
                .sum::<usize>(),
        }
    }
    #[test]
    fn each_insertion_copies_at_most_sixteen_nodes_and_capture_shares_root() {
        let mut d = ProvenanceDirectory::default();
        for id in [1, 2, 16, 256, 1 << 32, (1 << 60) + 1, u64::MAX - 1] {
            let next = d.insert(chunk(id)).unwrap();
            assert_eq!(changed(d.root.as_ref(), next.root.as_ref().unwrap()), 16);
            let captured = next.clone();
            assert!(Arc::ptr_eq(
                captured.root.as_ref().unwrap(),
                next.root.as_ref().unwrap()
            ));
            d = next;
        }
    }
}
