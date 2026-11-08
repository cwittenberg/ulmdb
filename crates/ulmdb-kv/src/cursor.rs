use crate::{
    ordering::{Bytewise, Comparator, stored},
    page::Page,
};
use std::ops::Bound;
use ulmdb_core::storage::{Error, ReadStore, Reference, Result};

mod position;
pub use position::CursorPosition;

/// A bounded cursor pinned to one root and store. Frames retain the path rather
/// than following mutable sibling links. Construction allocates its small stack;
/// iteration borrows key/value bytes directly from the store. The decoded leaf
/// stays borrowed from that immutable store until the cursor crosses a page.
pub struct Cursor<'a, S: ReadStore, C: Comparator = Bytewise> {
    order: C,
    store: &'a S,
    stack: Vec<(Reference, usize)>,
    leaf: Reference,
    leaf_page: Option<Page<'a>>,
    position: usize,
    lower: Bound<Vec<u8>>,
    upper: Bound<Vec<u8>>,
    reverse: bool,
    done: bool,
}

fn own(bound: Bound<&[u8]>) -> Bound<Vec<u8>> {
    match bound {
        Bound::Included(v) => Bound::Included(v.to_vec()),
        Bound::Excluded(v) => Bound::Excluded(v.to_vec()),
        Bound::Unbounded => Bound::Unbounded,
    }
}

impl<'a, S: ReadStore> Cursor<'a, S> {
    pub fn new(
        store: &'a S,
        root: Reference,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Self> {
        Self::with_order(store, root, lower, upper, reverse, Bytewise)
    }
}

impl<'a, S: ReadStore, C: Comparator> Cursor<'a, S, C> {
    pub fn with_order(
        store: &'a S,
        root: Reference,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
        order: C,
    ) -> Result<Self> {
        order.check_ready()?;
        for bound in [&lower, &upper] {
            if let Bound::Included(key) | Bound::Excluded(key) = bound {
                order.validate(key)?;
            }
        }
        let mut cursor = Self {
            order,
            store,
            stack: Vec::new(),
            leaf: root,
            leaf_page: None,
            position: 0,
            lower: own(lower),
            upper: own(upper),
            reverse,
            done: root.is_null(),
        };
        if !root.is_null() {
            cursor.seek(root)?;
        }
        Ok(cursor)
    }

    fn seek(&mut self, mut at: Reference) -> Result<()> {
        let bound = if self.reverse {
            &self.upper
        } else {
            &self.lower
        };
        let mut expected = None;
        loop {
            let page = Page::load(self.store, at)?;
            if expected.is_some_and(|level| level != page.level) {
                return Err(Error::Corrupt("cursor tree level mismatch"));
            }
            if page.level == 0 {
                self.leaf = at;
                self.position = match bound {
                    Bound::Unbounded => {
                        if self.reverse {
                            page.count
                        } else {
                            0
                        }
                    }
                    Bound::Included(key) => {
                        page.search_with(self.store, key, self.reverse, &self.order)?
                    }
                    Bound::Excluded(key) => {
                        page.search_with(self.store, key, !self.reverse, &self.order)?
                    }
                };
                self.leaf_page = Some(page);
                return Ok(());
            }
            let child = match bound {
                Bound::Unbounded => {
                    if self.reverse {
                        page.count
                    } else {
                        0
                    }
                }
                Bound::Included(key) | Bound::Excluded(key) => {
                    page.search_with(self.store, key, true, &self.order)?
                }
            };
            self.stack.push((at, child));
            at = page.child(self.store, child)?;
            expected = Some(page.level - 1);
        }
    }

    fn descend_edge(&mut self, mut at: Reference, mut expected: u16) -> Result<()> {
        loop {
            let page = Page::load(self.store, at)?;
            if page.level != expected {
                return Err(Error::Corrupt("cursor tree level mismatch"));
            }
            if page.level == 0 {
                self.leaf = at;
                self.position = if self.reverse { page.count } else { 0 };
                self.leaf_page = Some(page);
                return Ok(());
            }
            let child = if self.reverse { page.count } else { 0 };
            self.stack.push((at, child));
            at = page.child(self.store, child)?;
            expected -= 1;
        }
    }

    fn next_leaf(&mut self) -> Result<bool> {
        while let Some((at, child)) = self.stack.pop() {
            let page = Page::load(self.store, at)?;
            let next = if self.reverse {
                child.checked_sub(1)
            } else if child < page.count {
                Some(child + 1)
            } else {
                None
            };
            if let Some(next) = next {
                self.stack.push((at, next));
                self.descend_edge(page.child(self.store, next)?, page.level - 1)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn advance(&mut self) -> Result<Option<(&'a [u8], &'a [u8])>> {
        loop {
            let page = match self.leaf_page {
                Some(page) => page,
                None => {
                    // Reference-only continuations acquire bytes from their
                    // current scope; no borrowed page crosses a scope boundary.
                    let page = Page::load(self.store, self.leaf)?;
                    if page.level != 0 {
                        return Err(Error::Corrupt("cursor expected leaf"));
                    }
                    self.leaf_page = Some(page);
                    page
                }
            };
            let index = if self.reverse {
                self.position.checked_sub(1)
            } else if self.position < page.count {
                Some(self.position)
            } else {
                None
            };
            if let Some(index) = index {
                self.position = if self.reverse { index } else { index + 1 };
                let cell = page.cell(self.store, index)?;
                stored(&self.order, cell.key)?;
                let below = match &self.lower {
                    Bound::Included(v) => self.order.compare(cell.key, v).is_lt(),
                    Bound::Excluded(v) => self.order.compare(cell.key, v).is_le(),
                    Bound::Unbounded => false,
                };
                let above = match &self.upper {
                    Bound::Included(v) => self.order.compare(cell.key, v).is_gt(),
                    Bound::Excluded(v) => self.order.compare(cell.key, v).is_ge(),
                    Bound::Unbounded => false,
                };
                if below || above {
                    return Ok(None);
                }
                return Ok(Some((cell.key, cell.value)));
            }
            if !self.next_leaf()? {
                return Ok(None);
            }
        }
    }
}

impl<'a, S: ReadStore, C: Comparator> Iterator for Cursor<'a, S, C> {
    type Item = Result<(&'a [u8], &'a [u8])>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.advance() {
            Ok(Some(pair)) => Some(Ok(pair)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}

impl<S: ReadStore, C: Comparator> std::iter::FusedIterator for Cursor<'_, S, C> {}

/// An in-order continuation containing only checked references and cursor indexes.
/// Each step can borrow a fresh bounded scope over the SAME immutable tree.
/// Returned bytes borrow the supplied store; this position owns no data pins.
pub struct InOrderPosition {
    root: Reference,
    state: Option<(Vec<(Reference, usize)>, Reference, usize, bool)>,
}
impl InOrderPosition {
    pub fn new(root: Reference) -> Self {
        Self { root, state: None }
    }
    pub fn next<'a, S: ReadStore>(&mut self, store: &'a S) -> Result<Option<(&'a [u8], &'a [u8])>> {
        let mut cursor = if let Some((stack, leaf, position, done)) = self.state.take() {
            Cursor {
                order: Bytewise,
                store,
                stack,
                leaf,
                leaf_page: None,
                position,
                lower: Bound::Unbounded,
                upper: Bound::Unbounded,
                reverse: false,
                done,
            }
        } else {
            Cursor::new(store, self.root, Bound::Unbounded, Bound::Unbounded, false)?
        };
        let result = cursor.next().transpose();
        self.state = Some((
            std::mem::take(&mut cursor.stack),
            cursor.leaf,
            cursor.position,
            cursor.done,
        ));
        result
    }
}

#[cfg(test)]
#[path = "cursor/tests.rs"]
mod tests;
