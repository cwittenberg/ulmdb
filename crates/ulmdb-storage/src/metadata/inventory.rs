//! Immutable canonical inventory with an explicitly invalidating Vec adapter.
use super::tree::{Kind, Node};
use crate::{Error, Result, SegmentEntry};
use std::{
    ops::{Deref, DerefMut},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use ulmdb_core::inventory_stamp::Stamp;

#[derive(Clone, Debug)]
pub struct Inventory {
    root: Option<Arc<Node>>,
    dense: OnceLock<Arc<Vec<SegmentEntry>>>,
    stamp: Stamp,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            root: None,
            dense: OnceLock::new(),
            stamp: Stamp::default(),
        }
    }
}
impl From<Vec<SegmentEntry>> for Inventory {
    fn from(entries: Vec<SegmentEntry>) -> Self {
        if entries.is_empty() {
            Self::default()
        } else {
            Self {
                root: None,
                dense: OnceLock::from(Arc::new(entries)),
                stamp: Stamp::default(),
            }
        }
    }
}
impl FromIterator<SegmentEntry> for Inventory {
    fn from_iter<T: IntoIterator<Item = SegmentEntry>>(entries: T) -> Self {
        Self::from(entries.into_iter().collect::<Vec<_>>())
    }
}
impl Inventory {
    pub(super) fn canonical(
        root: Option<Arc<Node>>,
        dense: Option<Arc<Vec<SegmentEntry>>>,
    ) -> Result<Self> {
        static IDS: AtomicU64 = AtomicU64::new(1);
        let id = IDS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::LimitExceeded)?;
        let cache = OnceLock::new();
        if let Some(dense) = dense {
            let _ = cache.set(dense);
        }
        Ok(Self {
            root,
            dense: cache,
            stamp: Stamp::new(id).ok_or(Error::LimitExceeded)?,
        })
    }
    pub(crate) fn range_from_entries(entries: Vec<SegmentEntry>) -> Result<Self> {
        super::validate_entries(&entries)?;
        Self::canonical(super::tree::build(&entries), None)
    }
    pub(crate) fn range_append(&self, entry: SegmentEntry) -> Result<Self> {
        if self.stamp.value() == 0 || self.last().is_some_and(|last| last.id >= entry.id) {
            return Err(Error::InvalidInput("uncertified range inventory append"));
        }
        Self::canonical(super::tree::insert(self.root.clone(), entry), None)
    }
    pub(crate) fn stamp(&self) -> u64 {
        self.stamp.value()
    }
    pub(crate) fn into_cache(self) -> Option<Arc<Vec<SegmentEntry>>> {
        self.dense.into_inner()
    }
    pub fn is_materialized(&self) -> bool {
        self.dense.get().is_some()
    }
    pub fn len(&self) -> usize {
        self.root.as_ref().map_or_else(
            || self.dense.get().map_or(0, |v| v.len()),
            |node| node.count,
        )
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, mut index: usize) -> Option<&SegmentEntry> {
        if let Some(dense) = self.dense.get() {
            return dense.get(index);
        }
        let Some(mut node) = self.root.as_deref() else {
            return self.dense.get()?.get(index);
        };
        loop {
            match &node.kind {
                Kind::Leaf(entry) => return (index == 0).then_some(entry),
                Kind::Branch(left, right) => {
                    node = if index < left.count {
                        left
                    } else {
                        index -= left.count;
                        right
                    };
                }
            }
        }
    }
    pub fn iter(&self) -> InventoryIter<'_> {
        if let Some(dense) = self.dense.get() {
            return InventoryIter {
                state: IterState::Dense(dense.iter()),
            };
        }
        InventoryIter {
            state: IterState::Tree {
                front: Path::new(self.root.as_deref(), false),
                back: Path::new(self.root.as_deref(), true),
                remaining: self.len(),
            },
        }
    }
    pub fn first(&self) -> Option<&SegmentEntry> {
        self.get(0)
    }
    pub fn last(&self) -> Option<&SegmentEntry> {
        self.len().checked_sub(1).and_then(|i| self.get(i))
    }
    fn materialized(&self) -> &Arc<Vec<SegmentEntry>> {
        self.dense.get_or_init(|| {
            fn visit(node: &Node, out: &mut Vec<SegmentEntry>) {
                match &node.kind {
                    Kind::Leaf(entry) => out.push(entry.clone()),
                    Kind::Branch(left, right) => {
                        visit(left, out);
                        visit(right, out);
                    }
                }
            }
            let mut entries = Vec::with_capacity(self.root.as_ref().map_or(0, |n| n.count));
            if let Some(root) = &self.root {
                visit(root, &mut entries);
            }
            Arc::new(entries)
        })
    }
}
impl Deref for Inventory {
    type Target = Vec<SegmentEntry>;
    fn deref(&self) -> &Self::Target {
        self.materialized()
    }
}
impl DerefMut for Inventory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Invalidation MUST precede every allocation, copy and mutable borrow.
        // Existing clones retain their independent stamp and immutable owners.
        self.stamp.invalidate();
        self.materialized();
        self.root = None;
        Arc::make_mut(self.dense.get_mut().expect("materialized inventory"))
    }
}
struct Path<'a> {
    stack: [Option<&'a Node>; 65],
    top: usize,
    reverse: bool,
}
impl<'a> Path<'a> {
    fn new(root: Option<&'a Node>, reverse: bool) -> Self {
        let mut stack = [None; 65];
        stack[0] = root;
        Self {
            stack,
            top: usize::from(root.is_some()),
            reverse,
        }
    }
    fn next_leaf(&mut self) -> Option<&'a SegmentEntry> {
        while self.top != 0 {
            self.top -= 1;
            let node = self.stack[self.top].take().expect("bounded inventory path");
            match &node.kind {
                Kind::Leaf(entry) => return Some(entry),
                Kind::Branch(left, right) => {
                    let (first, second) = if self.reverse {
                        (right, left)
                    } else {
                        (left, right)
                    };
                    self.stack[self.top] = Some(second);
                    self.stack[self.top + 1] = Some(first);
                    self.top += 2;
                }
            }
        }
        None
    }
}
// Two fixed64-bit radix paths avoid heap allocation and preserve Vec's
// double-ended iteration, including mixed next/next_back without duplication.
#[allow(clippy::large_enum_variant)]
enum IterState<'a> {
    Dense(std::slice::Iter<'a, SegmentEntry>),
    Tree {
        front: Path<'a>,
        back: Path<'a>,
        remaining: usize,
    },
}
pub struct InventoryIter<'a> {
    state: IterState<'a>,
}
impl<'a> Iterator for InventoryIter<'a> {
    type Item = &'a SegmentEntry;
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.state {
            IterState::Dense(iter) => iter.next(),
            IterState::Tree {
                front, remaining, ..
            } => {
                if *remaining == 0 {
                    return None;
                }
                *remaining -= 1;
                front.next_leaf()
            }
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = match &self.state {
            IterState::Dense(iter) => iter.len(),
            IterState::Tree { remaining, .. } => *remaining,
        };
        (remaining, Some(remaining))
    }
}
impl DoubleEndedIterator for InventoryIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.state {
            IterState::Dense(iter) => iter.next_back(),
            IterState::Tree {
                back, remaining, ..
            } => {
                if *remaining == 0 {
                    return None;
                }
                *remaining -= 1;
                back.next_leaf()
            }
        }
    }
}
impl ExactSizeIterator for InventoryIter<'_> {}
impl std::iter::FusedIterator for InventoryIter<'_> {}
impl<'a> IntoIterator for &'a Inventory {
    type Item = &'a SegmentEntry;
    type IntoIter = InventoryIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a> IntoIterator for &'a mut Inventory {
    type Item = &'a mut SegmentEntry;
    type IntoIter = std::slice::IterMut<'a, SegmentEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().iter_mut()
    }
}
impl IntoIterator for Inventory {
    type Item = SegmentEntry;
    type IntoIter = std::vec::IntoIter<SegmentEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.materialized();
        Arc::try_unwrap(self.dense.into_inner().unwrap())
            .unwrap_or_else(|v| (*v).clone())
            .into_iter()
    }
}
