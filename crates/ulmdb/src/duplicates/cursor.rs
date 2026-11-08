use super::{
    MAX_VALUE,
    bucket::{Bucket, Inline},
    validate_stored,
};
use crate::ordering::BoundComparator;
use crate::{Error, Result};
use std::ops::Bound;
use ulmdb_core::storage::ReadStore;
use ulmdb_core::storage::Reference;
use ulmdb_kv::ordering::Comparator;
use ulmdb_storage::SegmentTable;

enum ValuesState<'a, S: ReadStore> {
    Empty,
    Inline {
        bucket: Inline<'a>,
        front: usize,
        back: usize,
        reverse: bool,
    },
    Tree(ulmdb_kv::Cursor<'a, S, BoundComparator>),
}
/// A borrowed within-key cursor using static inline/subtree dispatch.
pub struct DuplicateValues<'a, S: ReadStore = SegmentTable> {
    state: ValuesState<'a, S>,
    width: Option<u32>,
    failed: bool,
}
impl<'a, S: ReadStore> DuplicateValues<'a, S> {
    pub(super) fn new(
        store: &'a S,
        bucket: Option<Bucket<'a>>,
        width: Option<u32>,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
        order: BoundComparator,
    ) -> Result<Self> {
        check_bounds(
            &lower,
            &upper,
            MAX_VALUE.min(store.max_duplicate_value_size()),
        )?;
        for bound in [&lower, &upper] {
            if let Bound::Included(v) | Bound::Excluded(v) = bound {
                order.validate(v)?;
            }
        }
        let state = match bucket {
            None => ValuesState::Empty,
            Some(Bucket::Tree { root, .. }) => ValuesState::Tree(ulmdb_kv::Cursor::with_order(
                store,
                root,
                lower,
                upper,
                reverse,
                order.clone(),
            )?),
            Some(Bucket::Inline(bucket)) => {
                let front = match lower {
                    Bound::Unbounded => 0,
                    Bound::Included(v) => bucket.search_with(v, false, &order),
                    Bound::Excluded(v) => bucket.search_with(v, true, &order),
                };
                let back = match upper {
                    Bound::Unbounded => bucket.count,
                    Bound::Included(v) => bucket.search_with(v, true, &order),
                    Bound::Excluded(v) => bucket.search_with(v, false, &order),
                };
                ValuesState::Inline {
                    bucket,
                    front,
                    back,
                    reverse,
                }
            }
        };
        Ok(Self {
            state,
            width,
            failed: false,
        })
    }
}
impl<'a, S: ReadStore> Iterator for DuplicateValues<'a, S> {
    type Item = Result<&'a [u8]>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let result = match &mut self.state {
            ValuesState::Empty => return None,
            ValuesState::Inline {
                bucket,
                front,
                back,
                reverse,
            } => {
                if *front >= *back {
                    return None;
                }
                let index = if *reverse {
                    *back -= 1;
                    *back
                } else {
                    let index = *front;
                    *front += 1;
                    index
                };
                Ok(bucket.value(index))
            }
            ValuesState::Tree(cursor) => cursor.next()?.and_then(|(value, empty)| {
                if !empty.is_empty() {
                    return Err(Error::Corrupt("duplicate child value must be empty"));
                }
                validate_stored(value, self.width)?;
                Ok(value)
            }),
        };
        if result.is_err() {
            self.failed = true;
        }
        Some(result)
    }
}
impl<S: ReadStore> std::iter::FusedIterator for DuplicateValues<'_, S> {}

pub(super) fn check_bounds(
    lower: &Bound<&[u8]>,
    upper: &Bound<&[u8]>,
    maximum: usize,
) -> Result<()> {
    for bound in [lower, upper] {
        if let Bound::Included(value) | Bound::Excluded(value) = bound
            && value.len() > maximum
        {
            return Err(Error::InvalidInput(
                "duplicate cursor bound size limit exceeded",
            ));
        }
    }
    Ok(())
}

/// Lexicographic pair cursor. Only the current key's value cursor is retained;
/// next_key skips its remaining values without scanning or materializing them.
pub struct DuplicatePairs<'a, S: ReadStore = SegmentTable> {
    store: &'a S,
    keys: ulmdb_kv::Cursor<'a, S, BoundComparator>,
    current: Option<(&'a [u8], DuplicateValues<'a, S>)>,
    width: Option<u32>,
    reverse: bool,
    failed: bool,
    values: BoundComparator,
}
impl<'a, S: ReadStore> DuplicatePairs<'a, S> {
    pub(super) fn new(
        store: &'a S,
        root: Reference,
        width: Option<u32>,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Self> {
        Self::with_order(
            store,
            root,
            width,
            lower,
            upper,
            reverse,
            BoundComparator::Bytewise,
            BoundComparator::Bytewise,
        )
    }
    pub(super) fn with_order(
        store: &'a S,
        root: Reference,
        width: Option<u32>,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
        keys: BoundComparator,
        values: BoundComparator,
    ) -> Result<Self> {
        check_bounds(&lower, &upper, store.max_duplicate_key_size())?;
        Ok(Self {
            store,
            keys: ulmdb_kv::Cursor::with_order(store, root, lower, upper, reverse, keys)?,
            current: None,
            width,
            reverse,
            failed: false,
            values,
        })
    }
    /// Before iteration, returns the first pair. Afterwards, skips the current
    /// key and returns the first pair of the following key in cursor direction.
    pub fn next_key(&mut self) -> Option<Result<(&'a [u8], &'a [u8])>> {
        self.current = None;
        self.next()
    }
    fn advance(&mut self) -> Result<Option<(&'a [u8], &'a [u8])>> {
        loop {
            if let Some((key, values)) = &mut self.current
                && let Some(value) = values.next()
            {
                return Ok(Some((*key, value?)));
            }
            self.current = None;
            let Some(pair) = self.keys.next() else {
                return Ok(None);
            };
            let (key, bytes) = pair?;
            if key.len() > self.store.max_duplicate_key_size() {
                return Err(Error::Corrupt("stored duplicate key exceeds limit"));
            }
            let bucket = Bucket::decode_with(bytes, self.width, Some(&self.values))?;
            let values = DuplicateValues::new(
                self.store,
                Some(bucket),
                self.width,
                Bound::Unbounded,
                Bound::Unbounded,
                self.reverse,
                self.values.clone(),
            )?;
            self.current = Some((key, values));
        }
    }
}
impl<'a, S: ReadStore> Iterator for DuplicatePairs<'a, S> {
    type Item = Result<(&'a [u8], &'a [u8])>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.advance() {
            Ok(Some(pair)) => Some(Ok(pair)),
            Ok(None) => {
                self.failed = true;
                None
            }
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}
impl<S: ReadStore> std::iter::FusedIterator for DuplicatePairs<'_, S> {}
