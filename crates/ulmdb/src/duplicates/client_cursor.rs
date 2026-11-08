//! Bounded pair seeks for foreign clients without retaining iterator state.
//! Each seek retains only index paths and returns one borrowed or owned pair.
use super::{MAX_VALUE, bucket::Bucket, validate_stored};
use crate::{DatabaseKind, Error, ReadTxn, Result, WriteTxn};
use std::ops::Bound;
use ulmdb_core::storage::{ReadStore, Reference};

type BorrowedPair<'a> = (&'a [u8], &'a [u8]);
type PairBound<'a> = Option<(&'a [u8], Option<&'a [u8]>)>;

fn directional_bounds(
    key: Option<&[u8]>,
    inclusive: bool,
    reverse: bool,
) -> (Bound<&[u8]>, Bound<&[u8]>) {
    let bound = match key {
        None => Bound::Unbounded,
        Some(key) if inclusive => Bound::Included(key),
        Some(key) => Bound::Excluded(key),
    };
    if reverse {
        (Bound::Unbounded, bound)
    } else {
        (bound, Bound::Unbounded)
    }
}

fn bucket_edge<'a, S: ReadStore, V: ulmdb_kv::ordering::Comparator + Clone>(
    store: &'a S,
    bucket: Bucket<'a>,
    width: Option<u32>,
    value: Option<&[u8]>,
    inclusive: bool,
    reverse: bool,
    values: &V,
) -> Result<Option<&'a [u8]>> {
    match bucket {
        Bucket::Inline(inline) => {
            let index = match value {
                None if reverse => inline.count.checked_sub(1),
                None => Some(0),
                Some(value) if reverse => {
                    inline.search_with(value, inclusive, values).checked_sub(1)
                }
                Some(value) => Some(inline.search_with(value, !inclusive, values)),
            };
            Ok(index
                .filter(|&index| index < inline.count)
                .map(|index| inline.value(index)))
        }
        Bucket::Tree { root, .. } => {
            let (lower, upper) = directional_bounds(value, inclusive, reverse);
            let found =
                ulmdb_kv::Cursor::with_order(store, root, lower, upper, reverse, values.clone())?
                    .next()
                    .transpose()?;
            match found {
                None => Ok(None),
                Some((value, empty)) => {
                    if !empty.is_empty() {
                        return Err(Error::Corrupt("duplicate child value must be empty"));
                    }
                    validate_stored(value, width)?;
                    Ok(Some(value))
                }
            }
        }
    }
}

pub(crate) fn seek_pair<'a, S: ReadStore>(
    store: &'a S,
    root: Reference,
    kind: DatabaseKind,
    bound: PairBound<'_>,
    inclusive: bool,
    reverse: bool,
) -> Result<Option<BorrowedPair<'a>>> {
    seek_pair_with(
        store,
        root,
        kind,
        bound,
        inclusive,
        reverse,
        &ulmdb_kv::ordering::Bytewise,
        &ulmdb_kv::ordering::Bytewise,
    )
}
pub(crate) fn seek_pair_with<
    'a,
    S: ReadStore,
    K: ulmdb_kv::ordering::Comparator + Clone,
    V: ulmdb_kv::ordering::Comparator + Clone,
>(
    store: &'a S,
    root: Reference,
    kind: DatabaseKind,
    bound: PairBound<'_>,
    inclusive: bool,
    reverse: bool,
    keys_order: &K,
    values_order: &V,
) -> Result<Option<BorrowedPair<'a>>> {
    if !matches!(kind, DatabaseKind::Kv | DatabaseKind::Duplicates { .. }) {
        return Err(Error::WrongKind);
    }
    if bound.is_some_and(|(key, _)| key.len() > store.max_key_size()) {
        return Err(Error::InvalidInput("client cursor key bound exceeds limit"));
    }
    let key = bound.map(|(key, _)| key);
    let duplicate_value = if matches!(kind, DatabaseKind::Duplicates { .. }) {
        bound.and_then(|(_, value)| value)
    } else {
        None
    };
    if duplicate_value.is_some_and(|value| value.len() > MAX_VALUE) {
        return Err(Error::InvalidInput(
            "client cursor value bound exceeds limit",
        ));
    }
    // A pair bound may still select a later/earlier value of the same key.
    // A key-only exclusive bound skips that key's entire duplicate bucket.
    let key_inclusive = inclusive || duplicate_value.is_some();
    let (lower, upper) = directional_bounds(key, key_inclusive, reverse);
    let mut keys =
        ulmdb_kv::Cursor::with_order(store, root, lower, upper, reverse, keys_order.clone())?;
    if kind == DatabaseKind::Kv {
        return keys.next().transpose();
    }
    let DatabaseKind::Duplicates { width } = kind else {
        return Err(Error::WrongKind);
    };
    for pair in keys {
        let (stored_key, bytes) = pair?;
        if stored_key.len() > store.max_key_size() {
            return Err(Error::Corrupt("stored duplicate key exceeds limit"));
        }
        let bucket = Bucket::decode_with(bytes, width, Some(values_order))?;
        let value_bound = if key.is_some_and(|key| keys_order.compare(key, stored_key).is_eq()) {
            duplicate_value
        } else {
            None
        };
        if let Some(value) = bucket_edge(
            store,
            bucket,
            width,
            value_bound,
            inclusive,
            reverse,
            values_order,
        )? {
            return Ok(Some((stored_key, value)));
        }
    }
    Ok(None)
}

impl ReadTxn {
    /// Seek one KV or duplicate pair without materializing the database.
    ///
    /// `None` selects the first pair, or the last when `reverse` is true. A
    /// key-only bound includes that key's directional duplicate edge when
    /// inclusive, and otherwise skips the key. A key/value bound compares the
    /// full duplicate pair; ordinary KV databases ignore the value component.
    /// Returned bytes are owned independently of the transaction.
    pub fn client_seek_pair(
        &self,
        name: &[u8],
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        self.client_seek_pair_borrowed(name, bound, inclusive, reverse)
            .map(|pair| pair.map(|(key, value)| (key.to_vec(), value.to_vec())))
    }

    /// Borrow one selected key/value pair directly from the pinned snapshot.
    /// Seek semantics match [`Self::client_seek_pair`]. Dropping the temporary
    /// index cursors does not invalidate these mapped bytes; the transaction
    /// pins their storage across later seeks and other committed writers.
    pub fn client_seek_pair_borrowed(
        &self,
        name: &[u8],
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let database = self.database(name)?;
        seek_pair_with(
            &self.snapshot.segments,
            database.root,
            database.kind,
            bound,
            inclusive,
            reverse,
            &crate::ordering::BoundComparator::bind(database.ordering.keys, None)?,
            &crate::ordering::BoundComparator::bind(database.ordering.duplicates, None)?,
        )
    }
}

impl WriteTxn<'_> {
    /// Seek one pair in the current private writer state. Semantics match
    /// [`ReadTxn::client_seek_pair`]; subsequent mutations cannot alias the copy.
    pub fn client_seek_pair(
        &self,
        name: &[u8],
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        self.client_seek_pair_borrowed(name, bound, inclusive, reverse)
            .map(|pair| pair.map(|(key, value)| (key.to_vec(), value.to_vec())))
    }

    /// Borrow one selected key/value pair from the current writer's store.
    /// Seek semantics match [`ReadTxn::client_seek_pair`]. Subsequent seeks may
    /// share these bytes; a later writer mutation requires ending their borrow.
    pub fn client_seek_pair_borrowed(
        &self,
        name: &[u8],
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> Result<Option<(&[u8], &[u8])>> {
        self.live()?;
        let database = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        seek_pair_with(
            &self.arena,
            database.root,
            database.kind,
            bound,
            inclusive,
            reverse,
            &crate::ordering::BoundComparator::bind(database.ordering.keys, None)?,
            &crate::ordering::BoundComparator::bind(database.ordering.duplicates, None)?,
        )
    }
}
