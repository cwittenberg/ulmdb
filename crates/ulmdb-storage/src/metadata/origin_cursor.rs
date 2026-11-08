//! Owned, bounded metadata-node traversal. Information only: neither a graph
//! certificate nor permission to delete a cohort follows from this cursor.
use super::{Kind, Node};
use crate::{Error, Manifest, Result};
use std::{mem::ManuallyDrop, sync::Arc};

pub struct MetadataOriginCursor {
    creator: u32,
    pending: ManuallyDrop<Vec<Arc<Node>>>,
}
impl Drop for MetadataOriginCursor {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            let pending = std::mem::replace(&mut self.pending, ManuallyDrop::new(Vec::new()));
            drop(ManuallyDrop::into_inner(pending));
        }
    }
}
impl MetadataOriginCursor {
    pub(crate) fn new(manifest: &Manifest) -> Result<Self> {
        let mut pending = Vec::new();
        if manifest.format_minor == 4 {
            let index = manifest
                .index
                .as_ref()
                .ok_or(Error::Corrupt("missing prepared metadata index"))?;
            if index.generation != manifest.generation {
                return Err(Error::Corrupt("metadata cursor generation"));
            }
            pending
                .try_reserve_exact(65)
                .map_err(|_| Error::LimitExceeded)?;
            if let Some(root) = &index.root {
                pending.push(root.clone());
            }
        }
        Ok(Self {
            creator: std::process::id(),
            pending: ManuallyDrop::new(pending),
        })
    }
    /// Visit exactly one immutable Patricia node (depth <=64). Repeated origins
    /// are intentional; callers charge each node and deduplicate their marks.
    pub fn next_origin(&mut self) -> Result<Option<u128>> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited metadata cursor"));
        }
        let Some(node) = self.pending.pop() else {
            return Ok(None);
        };
        let origin = node
            .location
            .ok_or(Error::Corrupt("unprepared metadata cursor node"))?
            .origin;
        if let Kind::Branch(left, right) = &node.kind {
            if self.pending.len() + 2 > 65 {
                return Err(Error::Corrupt("metadata cursor depth bound"));
            }
            self.pending.push(right.clone());
            self.pending.push(left.clone());
        }
        Ok(Some(origin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_resumes_one_node_without_materializing_origin_set() {
        let mut m = Manifest::empty().unwrap();
        m.format_minor = 4;
        m.segments = (1..=100)
            .map(|id| crate::SegmentEntry {
                id,
                length: 4096,
                hash: [7; 32],
                cohort: 0,
            })
            .collect();
        m.next_segment = 101;
        m.prepare_indexed(None).unwrap();
        assert!(m.index.as_ref().unwrap().origins.get().is_none());
        let mut c = m.metadata_origin_cursor().unwrap();
        let mut count = 0;
        while let Some(origin) = c.next_origin().unwrap() {
            assert_eq!(origin, m.generation);
            count += 1;
        }
        assert_eq!(count, 199);
        assert!(m.index.as_ref().unwrap().origins.get().is_none());
        assert!(c.next_origin().unwrap().is_none());
        let mut bad = m.clone_metadata();
        bad.generation += 1;
        assert!(bad.metadata_origin_cursor().is_err());
    }
}
