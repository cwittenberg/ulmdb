//! Sorted duplicates use inline buckets or per-key trees. No tuple decoding or
//! second stored copy of a duplicate value is needed to return borrowed bytes.
use crate::ordering::{BoundComparator, DatabaseComparators};
use crate::{DatabaseKind, Error, ReadTxn, Result};
pub(crate) mod bucket;
mod client_cursor;
pub(crate) use client_cursor::{seek_pair, seek_pair_with};
mod copy;
mod cursor;
pub(crate) mod structural;
pub(crate) use copy::{
    equivalent as equivalent_buckets, equivalent_scoped as equivalent_buckets_scoped,
    rebuild as copy_buckets, rebuild_scoped as copy_buckets_scoped,
};
pub use structural::StructuralPairs;
pub(crate) mod ordered_write;
mod write;
use bucket::Bucket;
pub use cursor::{DuplicatePairs, DuplicateValues};
use std::ops::Bound;
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_storage::SegmentTable;
pub(crate) use write::{Mutation, mutate_root};

const MAX_VALUE: usize = 65_536;

fn width(kind: DatabaseKind) -> Result<Option<u32>> {
    if let DatabaseKind::Duplicates { width } = kind {
        Ok(width)
    } else {
        Err(Error::WrongKind)
    }
}
fn check_key<S: ReadStore>(store: &S, key: &[u8]) -> Result<()> {
    if key.len() > store.max_duplicate_key_size() {
        Err(Error::InvalidInput("duplicate key size limit exceeded"))
    } else {
        Ok(())
    }
}
fn check_value(value: &[u8], width: Option<u32>) -> Result<()> {
    if value.len() > MAX_VALUE {
        return Err(Error::InvalidInput("duplicate value size limit exceeded"));
    }
    if width.is_some_and(|width| value.len() != width as usize) {
        return Err(Error::InvalidInput("fixed duplicate width mismatch"));
    }
    Ok(())
}
fn validate_stored(value: &[u8], width: Option<u32>) -> Result<()> {
    if value.len() > MAX_VALUE || width.is_some_and(|width| value.len() != width as usize) {
        Err(Error::Corrupt("stored duplicate width/size mismatch"))
    } else {
        Ok(())
    }
}

/// Prepared read view: bind the database once, then query its raw keys/values.
/// The originating transaction pins every returned slice and iterator.
pub struct DuplicatesRead<'a, S: ReadStore = SegmentTable> {
    store: &'a S,
    root: Reference,
    width: Option<u32>,
    keys: BoundComparator,
    values: BoundComparator,
}
impl<'a, S: ReadStore> DuplicatesRead<'a, S> {
    pub(crate) fn chunked_order(
        store: &'a S,
        root: Reference,
        kind: DatabaseKind,
        keys: BoundComparator,
        values: BoundComparator,
    ) -> Result<Self> {
        Ok(Self {
            store,
            root,
            width: width(kind)?,
            keys,
            values,
        })
    }

    pub(crate) fn bytewise(store: &'a S, root: Reference, kind: DatabaseKind) -> Result<Self> {
        Ok(Self {
            store,
            root,
            width: width(kind)?,
            keys: BoundComparator::Bytewise,
            values: BoundComparator::Bytewise,
        })
    }
    pub fn fixed_width(&self) -> Option<u32> {
        self.width
    }
    pub fn count(&self, key: &[u8]) -> Result<u64> {
        check_key(self.store, key)?;
        Ok(Bucket::get_with(
            self.store,
            self.root,
            key,
            self.width,
            &self.keys,
            &self.values,
        )?
        .map_or(0, |bucket| bucket.count()))
    }
    pub fn contains(&self, key: &[u8], value: &[u8]) -> Result<bool> {
        check_key(self.store, key)?;
        check_value(value, self.width)?;
        if value.len() > self.store.max_duplicate_value_size() {
            return Err(Error::InvalidInput("duplicate value size limit exceeded"));
        }
        let Some(bucket) = Bucket::get_with(
            self.store,
            self.root,
            key,
            self.width,
            &self.keys,
            &self.values,
        )?
        else {
            return Ok(false);
        };
        Ok(bucket.seek_with(self.store, value, &self.values)?.is_some())
    }
    /// Return the first value in unsigned byte order.
    pub fn first(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        check_key(self.store, key)?;
        match Bucket::get_with(
            self.store,
            self.root,
            key,
            self.width,
            &self.keys,
            &self.values,
        )? {
            None => Ok(None),
            Some(Bucket::Inline(inline)) => Ok(Some(inline.value(0))),
            bucket => DuplicateValues::new(
                self.store,
                bucket,
                self.width,
                Bound::Unbounded,
                Bound::Unbounded,
                false,
                self.values.clone(),
            )?
            .next()
            .transpose(),
        }
    }
    /// Return the mapped raw value for an exact pair, if present.
    pub fn seek_pair(&self, key: &[u8], value: &[u8]) -> Result<Option<&'a [u8]>> {
        check_key(self.store, key)?;
        check_value(value, self.width)?;
        if value.len() > self.store.max_duplicate_value_size() {
            return Err(Error::InvalidInput("duplicate value size limit exceeded"));
        }
        let Some(bucket) = Bucket::get_with(
            self.store,
            self.root,
            key,
            self.width,
            &self.keys,
            &self.values,
        )?
        else {
            return Ok(None);
        };
        let result = bucket.seek_with(self.store, value, &self.values)?;
        if let Some(stored) = result {
            validate_stored(stored, self.width)?;
        }
        Ok(result)
    }
    /// Bounds apply to duplicate values within one key. Empty/missing keys give
    /// an empty iterator; reverse changes direction without changing bounds.
    pub fn values(
        &self,
        key: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<DuplicateValues<'a, S>> {
        check_key(self.store, key)?;
        DuplicateValues::new(
            self.store,
            Bucket::get_with(
                self.store,
                self.root,
                key,
                self.width,
                &self.keys,
                &self.values,
            )?,
            self.width,
            lower,
            upper,
            reverse,
            self.values.clone(),
        )
    }
    pub fn pairs(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<DuplicatePairs<'a, S>> {
        DuplicatePairs::with_order(
            self.store,
            self.root,
            self.width,
            lower,
            upper,
            reverse,
            self.keys.clone(),
            self.values.clone(),
        )
    }
}

impl ReadTxn {
    pub fn duplicates(&self, name: &[u8]) -> Result<DuplicatesRead<'_>> {
        self.duplicates_with_ordering(name, &Default::default())
    }
    pub fn duplicates_with_ordering(
        &self,
        name: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<DuplicatesRead<'_>> {
        let db = self.database(name)?;
        Ok(DuplicatesRead {
            store: &self.snapshot.segments,
            root: db.root,
            width: width(db.kind)?,
            keys: BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?,
            values: BoundComparator::bind(db.ordering.duplicates, callbacks.duplicates.as_ref())?,
        })
    }
    pub fn duplicate_count(&self, name: &[u8], key: &[u8]) -> Result<u64> {
        self.duplicates(name)?.count(key)
    }
    pub fn duplicate_first(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.duplicates(name)?.first(key)
    }
    pub fn duplicate_seek_pair(
        &self,
        name: &[u8],
        key: &[u8],
        value: &[u8],
    ) -> Result<Option<&[u8]>> {
        self.duplicates(name)?.seek_pair(key, value)
    }
    pub fn duplicate_values(
        &self,
        name: &[u8],
        key: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<DuplicateValues<'_>> {
        self.duplicates(name)?.values(key, lower, upper, reverse)
    }
}

impl ReadTxn {
    pub fn duplicate_pairs(
        &self,
        name: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<DuplicatePairs<'_>> {
        self.duplicates(name)?.pairs(lower, upper, reverse)
    }
}

/// Shared snapshot/export audit, accepting legacy subtree and inline buckets.
pub(crate) fn audit_buckets<S: ReadStore>(
    store: &S,
    root: Reference,
    width: Option<u32>,
    page_budget: u64,
) -> Result<u64> {
    let mut max_tree_pages = ulmdb_kv::audit_tree(store, root, page_budget)?.pages;
    let mut previous = None;
    for pair in ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, bytes) = pair?;
        if key.len() > store.max_duplicate_key_size()
            || previous.is_some_and(|prior: &[u8]| prior >= key)
            || ulmdb_kv::get(store, root, key)? != Some(bytes)
        {
            return Err(Error::Corrupt(
                "invalid duplicate main ordering/size/routing",
            ));
        }
        previous = Some(key);
        if let Bucket::Tree {
            root,
            count: expected,
        } = Bucket::decode(bytes, width)?
        {
            max_tree_pages =
                max_tree_pages.max(ulmdb_kv::audit_tree(store, root, page_budget)?.pages);
            let mut count = 0u64;
            let mut previous = None;
            for pair in
                ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)?
            {
                let (value, empty) = pair?;
                validate_stored(value, width)?;
                if !empty.is_empty()
                    || previous.is_some_and(|prior: &[u8]| prior >= value)
                    || ulmdb_kv::get(store, root, value)? != Some(empty)
                {
                    return Err(Error::Corrupt(
                        "invalid duplicate child ordering/value/routing",
                    ));
                }
                previous = Some(value);
                count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
            }
            if count != expected {
                return Err(Error::Corrupt("duplicate count mismatch"));
            }
        }
    }
    Ok(max_tree_pages)
}

pub(crate) fn count_at<S: ReadStore>(
    store: &S,
    root: Reference,
    kind: DatabaseKind,
    key: &[u8],
) -> Result<u64> {
    check_key(store, key)?;
    Ok(Bucket::get(store, root, key, width(kind)?)?.map_or(0, Bucket::count))
}
