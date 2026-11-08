use crate::node::{Node, branch};
use ulmdb_core::{
    Prefix,
    storage::{Error, ReadStore, Reference, Result},
};

#[derive(Clone, Copy)]
enum Mode {
    Ancestors,
    Subnets,
    Overlaps,
}

/// Streaming enumeration with bounded inline storage, independent of result size.
/// Ancestors are longest-first. Other traversals use network, then prefix order.
pub struct Cursor<'a, S: ReadStore + ?Sized> {
    store: &'a S,
    query: Prefix,
    mode: Mode,
    pending: [Option<Node>; 129],
    count: usize,
    failed: bool,
}
impl<'a, S: ReadStore + ?Sized> Cursor<'a, S> {
    fn new(store: &'a S, root: Reference, query: Prefix, mode: Mode) -> Result<Self> {
        let mut cursor = Self {
            store,
            query,
            mode,
            pending: [None; 129],
            count: 0,
            failed: false,
        };
        if root.is_null() {
            return Ok(cursor);
        }
        let mut node = Node::read(store, root)?;
        if matches!(mode, Mode::Ancestors) {
            loop {
                if !node.prefix.contains_prefix(query) {
                    break;
                }
                if !node.value.is_null() {
                    cursor.push(node)?;
                }
                if node.prefix.length() == query.length() {
                    break;
                }
                let Some(child) =
                    node.child(store, branch(query.network(), node.prefix.length()))?
                else {
                    break;
                };
                node = child;
            }
        } else if node.prefix.overlaps(query) {
            cursor.push(node)?;
        }
        Ok(cursor)
    }
    fn push(&mut self, node: Node) -> Result<()> {
        let slot = self
            .pending
            .get_mut(self.count)
            .ok_or(Error::Corrupt("radix traversal depth exceeded"))?;
        *slot = Some(node);
        self.count += 1;
        Ok(())
    }
    fn advance(&mut self) -> Result<Option<(Prefix, &'a [u8])>> {
        while self.count != 0 {
            self.count -= 1;
            let node = self.pending[self.count]
                .take()
                .ok_or(Error::Corrupt("invalid radix traversal stack"))?;
            if !matches!(self.mode, Mode::Ancestors) {
                // Push one then zero so pop order is lexicographic network order.
                for side in [1, 0] {
                    if let Some(child) = node.child(self.store, side)?
                        && child.prefix.overlaps(self.query)
                    {
                        self.push(child)?;
                    }
                }
            }
            let selected = match self.mode {
                Mode::Ancestors => true,
                Mode::Subnets => self.query.contains_prefix(node.prefix),
                Mode::Overlaps => true,
            };
            if selected && !node.value.is_null() {
                return Ok(Some((node.prefix, self.store.payload(node.value)?)));
            }
        }
        Ok(None)
    }
}
impl<'a, S: ReadStore + ?Sized> Iterator for Cursor<'a, S> {
    type Item = Result<(Prefix, &'a [u8])>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.advance() {
            Ok(Some(item)) => Some(Ok(item)),
            Ok(None) => None,
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}
impl<S: ReadStore + ?Sized> std::iter::FusedIterator for Cursor<'_, S> {}

pub fn ancestors<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Cursor<'_, S>> {
    Cursor::new(store, root, prefix, Mode::Ancestors)
}
pub fn subnets<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Cursor<'_, S>> {
    Cursor::new(store, root, prefix, Mode::Subnets)
}
pub fn overlaps<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Cursor<'_, S>> {
    Cursor::new(store, root, prefix, Mode::Overlaps)
}

/// Bounded subnet continuation over one immutable radix view. No borrowed
/// payload/window is retained between calls; each result borrows its own store.
pub struct SubnetPosition {
    root: Reference,
    query: Prefix,
    pending: Option<([Option<Node>; 129], usize, bool)>,
}
impl SubnetPosition {
    pub fn new(root: Reference, query: Prefix) -> Self {
        Self {
            root,
            query,
            pending: None,
        }
    }
    pub fn next<'a, S: ReadStore + ?Sized>(
        &mut self,
        store: &'a S,
    ) -> Result<Option<(Prefix, &'a [u8])>> {
        let mut cursor = if let Some((pending, count, failed)) = self.pending.take() {
            Cursor {
                store,
                query: self.query,
                mode: Mode::Subnets,
                pending,
                count,
                failed,
            }
        } else {
            Cursor::new(store, self.root, self.query, Mode::Subnets)?
        };
        let result = cursor.next().transpose();
        self.pending = Some((cursor.pending, cursor.count, cursor.failed));
        result
    }
}
