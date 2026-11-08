use super::*;

#[cfg(test)]
#[path = "scoped_position_tests.rs"]
mod scoped_tests;

/// Owned bytewise cursor position. It retains references and indexes, never bytes.
/// Every operation must resolve the same immutable tree in the supplied store.
/// Returned rows borrow only that operation's store.
pub struct CursorPosition {
    root: Reference,
    stack: Vec<(Reference, usize)>,
    scratch: Vec<(Reference, usize)>,
    leaf: Option<(Reference, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use ulmdb_storage::{Arena, SegmentTable};

    struct Counted {
        arena: Arena,
        root: Reference,
        root_reads: Cell<usize>,
    }
    impl ReadStore for Counted {
        fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
            if at == self.root {
                self.root_reads.set(self.root_reads.get() + 1);
            }
            self.arena.read(at, length)
        }
    }

    #[test]
    fn owned_position_continues_without_a_root_search_per_row() {
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let mut root = Reference::NULL;
        for key in 0..1200u64 {
            root = crate::put(&mut arena, root, &key.to_be_bytes(), &[7; 64]).unwrap();
        }
        let store = Counted {
            arena,
            root,
            root_reads: Cell::new(0),
        };
        let mut position = CursorPosition::new(root);
        assert_eq!(
            position
                .seek(&store, Bound::Unbounded, false)
                .unwrap()
                .unwrap()
                .0,
            &0u64.to_be_bytes()
        );
        store.root_reads.set(0);
        for key in 1..1200u64 {
            let pair = position.next(&store).unwrap().unwrap();
            assert_eq!(pair.0, &key.to_be_bytes());
            assert_eq!(pair.1, &[7; 64]);
        }
        assert!(position.next(&store).unwrap().is_none());
        assert!(
            store.root_reads.get() < 1200 / 8,
            "{} root reads",
            store.root_reads.get()
        );
        assert_eq!(
            position.current(&store).unwrap().unwrap().0,
            &1199u64.to_be_bytes()
        );
        for key in (0..1199u64).rev() {
            assert_eq!(
                position.previous(&store).unwrap().unwrap().0,
                &key.to_be_bytes()
            );
        }
        assert!(position.previous(&store).unwrap().is_none());
        assert_eq!(
            position.current(&store).unwrap().unwrap().0,
            &0u64.to_be_bytes()
        );
    }

    #[test]
    fn owned_position_crosses_empty_leaves_and_preserves_eof_and_failed_seek() {
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let mut root = Reference::NULL;
        for key in 0..600u64 {
            root = crate::put(&mut arena, root, &key.to_be_bytes(), &[9; 256]).unwrap();
        }
        for key in 1..599u64 {
            root = crate::delete(&mut arena, root, &key.to_be_bytes()).unwrap();
        }
        let mut position = CursorPosition::new(root);
        assert_eq!(
            position
                .seek(&arena, Bound::Included(&250u64.to_be_bytes()), false)
                .unwrap()
                .unwrap()
                .0,
            &599u64.to_be_bytes()
        );
        assert!(position.next(&arena).unwrap().is_none());
        assert_eq!(
            position.previous(&arena).unwrap().unwrap().0,
            &0u64.to_be_bytes()
        );
        assert!(position.previous(&arena).unwrap().is_none());
        assert!(
            position
                .seek(&arena, Bound::Excluded(&0u64.to_be_bytes()), true)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            position.current(&arena).unwrap().unwrap().0,
            &0u64.to_be_bytes()
        );
        assert_eq!(
            position.next(&arena).unwrap().unwrap().0,
            &599u64.to_be_bytes()
        );
    }
}
impl CursorPosition {
    pub fn new(root: Reference) -> Self {
        Self {
            root,
            stack: Vec::new(),
            scratch: Vec::new(),
            leaf: None,
        }
    }

    pub fn is_positioned(&self) -> bool {
        self.leaf.is_some()
    }

    /// Seek to the first row at the bound, or the last row for reverse traversal.
    /// An unsuccessful seek preserves the previous position.
    pub fn seek<'a, S: ReadStore>(
        &mut self,
        store: &'a S,
        bound: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Option<(&'a [u8], &'a [u8])>> {
        let (lower, upper) = if reverse {
            (Bound::Unbounded, bound)
        } else {
            (bound, Bound::Unbounded)
        };
        let mut cursor = Cursor::new(store, self.root, lower, upper, reverse)?;
        let row = cursor.next().transpose()?;
        if row.is_some() {
            self.leaf = Some((
                cursor.leaf,
                if reverse {
                    cursor.position
                } else {
                    cursor.position - 1
                },
            ));
            self.scratch = std::mem::replace(&mut self.stack, std::mem::take(&mut cursor.stack));
        }
        Ok(row)
    }

    pub fn current<'a, S: ReadStore>(&self, store: &'a S) -> Result<Option<(&'a [u8], &'a [u8])>> {
        let Some((at, index)) = self.leaf else {
            return Ok(None);
        };
        let page = Page::load(store, at)?;
        if page.level != 0 {
            return Err(Error::Corrupt("position expected leaf"));
        }
        let cell = page.cell(store, index)?;
        Ok(Some((cell.key, cell.value)))
    }

    /// Reuse the current row if it satisfies this bytewise bound, otherwise
    /// attempt exactly one adjacent step. Only an unpositioned cursor seeks.
    /// The attempted row is filtered by the bound; an out-of-bound neighbor
    /// can advance this owned position while returning None, as current+step
    /// followed by caller-side filtering does. EOF/error preserve position.
    /// Page bytes are borrowed only from this operation's immutable store.
    pub fn current_or_step<'a, S: ReadStore>(
        &mut self,
        store: &'a S,
        bound: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Option<(&'a [u8], &'a [u8])>> {
        let Some((at, index)) = self.leaf else {
            return self.seek(store, bound, reverse);
        };
        let page = Page::load(store, at)?;
        if page.level != 0 {
            return Err(Error::Corrupt("position expected leaf"));
        }
        // Keep current()'s complete cell/value validation before considering
        // the adjacent row. A rejected bound must not skip current corruption.
        let cell = page.cell(store, index)?;
        let accepts = |key: &[u8]| match bound {
            Bound::Unbounded => true,
            Bound::Included(bound) => if reverse { key <= bound } else { key >= bound },
            Bound::Excluded(bound) => if reverse { key < bound } else { key > bound },
        };
        if accepts(cell.key) {
            return Ok(Some((cell.key, cell.value)));
        }
        Ok(self
            .step_loaded(store, at, index, page, reverse)?
            .filter(|pair| accepts(pair.0)))
    }

    pub fn next<'a, S: ReadStore>(&mut self, store: &'a S) -> Result<Option<(&'a [u8], &'a [u8])>> {
        self.step(store, false)
    }

    pub fn previous<'a, S: ReadStore>(
        &mut self,
        store: &'a S,
    ) -> Result<Option<(&'a [u8], &'a [u8])>> {
        self.step(store, true)
    }

    fn step<'a, S: ReadStore>(
        &mut self,
        store: &'a S,
        reverse: bool,
    ) -> Result<Option<(&'a [u8], &'a [u8])>> {
        let Some((at, index)) = self.leaf else {
            return self.seek(store, Bound::Unbounded, reverse);
        };
        let page = Page::load(store, at)?;
        self.step_loaded(store, at, index, page, reverse)
    }

    // A decoded page can be reused only within the fresh operation that loaded
    // it. The owned continuation still stores no borrowed bytes or owners.
    fn step_loaded<'a, S: ReadStore>(
        &mut self,
        store: &'a S,
        at: Reference,
        index: usize,
        page: Page<'a>,
        reverse: bool,
    ) -> Result<Option<(&'a [u8], &'a [u8])>> {
        if page.level != 0 || index >= page.count {
            return Err(Error::Corrupt("invalid cursor position"));
        }
        let next = if reverse {
            index.checked_sub(1)
        } else {
            (index + 1 < page.count).then_some(index + 1)
        };
        if let Some(next) = next {
            let cell = page.cell(store, next)?;
            self.leaf = Some((at, next));
            return Ok(Some((cell.key, cell.value)));
        }
        // Preserve the current row on EOF and on a fallible page crossing.
        // Scratch capacity is reused; only page crossings copy the short path.
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.stack);
        let mut cursor = Cursor {
            order: Bytewise,
            store,
            stack: std::mem::take(&mut self.scratch),
            leaf: at,
            leaf_page: Some(page),
            position: index,
            lower: Bound::Unbounded,
            upper: Bound::Unbounded,
            reverse,
            done: false,
        };
        let row = (|| {
            if cursor.next_leaf()? {
                cursor.advance()
            } else {
                Ok(None)
            }
        })();
        if matches!(&row, Ok(Some(_))) {
            self.leaf = Some((
                cursor.leaf,
                if reverse {
                    cursor.position
                } else {
                    cursor.position - 1
                },
            ));
            std::mem::swap(&mut self.stack, &mut cursor.stack);
        }
        self.scratch = cursor.stack;
        row
    }
}
