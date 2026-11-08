pub use crate::contract::*;
use bbtree::unsync::BBTreeMap;
use std::ops::Bound;

pub struct Blessed<'a> {
    pub map: BBTreeMap<Vec<u8>, usize>,
    pub records: &'a [Record<'a>],
}
impl Lookup for Blessed<'_> {
    fn name(&self) -> &'static str {
        "bbtree_0_2_0_unsync"
    }
    fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.map.get(key).map(|&id| self.records[id].1)
    }
    fn rows<'a>(&'a self, key: &[u8], limit: usize) -> Vec<Record<'a>> {
        self.map
            .range::<[u8], _>((Bound::Included(key), Bound::Unbounded))
            .take(limit)
            .map(|(_, &id)| self.records[id])
            .collect()
    }
    fn scan_digest(&self, key: &[u8], limit: usize) -> u64 {
        self.map
            .range::<[u8], _>((Bound::Included(key), Bound::Unbounded))
            .take(limit)
            .map(|(_, &id)| {
                let (k, v) = self.records[id];
                row_digest(k, v)
            })
            .fold(0u64, u64::wrapping_add)
    }
}
pub fn node_bytes(map: &BBTreeMap<Vec<u8>, usize>) -> usize {
    let Some(root) = map.inspect().root() else {
        return 0;
    };
    let mut pending = vec![root];
    let mut bytes = 0;
    while let Some(node) = pending.pop() {
        bytes += node.page_map().size();
        for i in 0..=node.len() {
            if let Some(child) = node.child(i) {
                pending.push(child);
            }
        }
    }
    bytes
}
