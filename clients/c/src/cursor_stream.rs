//! Cursor query strategies. Byte ordering seeks native indexes directly;
//! alternate LMDB comparators scan native pairs while retaining one candidate.
use super::*;
use std::cmp::Ordering;

type BorrowedPair<'a> = (&'a [u8], &'a [u8]);
type Bound<'a> = Option<(&'a [u8], Option<&'a [u8]>)>;

#[derive(Clone, Copy)]
pub(super) struct Query<'a> {
    pub bound: Bound<'a>,
    pub inclusive: bool,
    pub reverse: bool,
    pub only_key: Option<&'a [u8]>,
    pub exact_value: Option<&'a [u8]>,
}

// Optional adapter implementation; the ordinary C ABI is unchanged.
#[cfg(feature = "sql-batch")]
#[path = "../../../storage-engines/postgresql/native/sql_batch.rs"]
pub(crate) mod sql_batch;
impl<'a> Query<'a> {
    pub(super) fn edge(reverse: bool) -> Self {
        Self {
            bound: None,
            inclusive: true,
            reverse,
            only_key: None,
            exact_value: None,
        }
    }
    pub(super) fn bounded(
        key: &'a [u8],
        value: Option<&'a [u8]>,
        inclusive: bool,
        reverse: bool,
    ) -> Self {
        Self {
            bound: Some((key, value)),
            inclusive,
            reverse,
            only_key: None,
            exact_value: None,
        }
    }
    pub(super) fn same_key(mut self, key: &'a [u8]) -> Self {
        self.only_key = Some(key);
        self
    }
    pub(super) fn exact_value(mut self, value: &'a [u8]) -> Self {
        self.exact_value = Some(value);
        self
    }
    fn accepts(self, txn: &MDB_txn, handle: &ResolvedAlias, pair: BorrowedPair<'_>) -> bool {
        if self
            .only_key
            .is_some_and(|key| !txn.key_order(handle, pair.0, key).is_eq())
        {
            return false;
        }
        if self
            .exact_value
            .is_some_and(|value| !txn.value_order(handle, pair.1, value).is_eq())
        {
            return false;
        }
        let Some((key, value)) = self.bound else {
            return true;
        };
        let order = txn.key_order(handle, pair.0, key).then_with(|| {
            if matches!(handle.kind(), DatabaseKind::Duplicates { .. }) {
                value.map_or(Ordering::Equal, |value| {
                    txn.value_order(handle, pair.1, value)
                })
            } else {
                Ordering::Equal
            }
        });
        (if self.reverse {
            order.is_lt()
        } else {
            order.is_gt()
        }) || (self.inclusive && order.is_eq())
    }
}

// The Strategy decision is made once per query, outside its native scan loop.
enum SeekStrategy {
    Bytes,
    Comparator,
}
impl SeekStrategy {
    fn choose(txn: &MDB_txn, handle: &ResolvedAlias) -> FfiResult<Self> {
        Ok(if txn.native_order_matches(handle)? {
            Self::Bytes
        } else {
            Self::Comparator
        })
    }
    fn select<'a>(
        &self,
        txn: &'a MDB_txn,
        handle: &ResolvedAlias,
        query: Query<'_>,
    ) -> FfiResult<Option<BorrowedPair<'a>>> {
        match self {
            Self::Bytes => {
                Ok(
                    native_seek(txn, handle, query.bound, query.inclusive, query.reverse)?
                        .filter(|pair| query.accepts(txn, handle, *pair)),
                )
            }
            Self::Comparator => {
                let mut selected = None;
                let mut pair = txn.physical_pair_after(handle, None).map_err(code)?;
                while let Some(candidate) = pair {
                    if query.accepts(txn, handle, candidate)
                        && selected.is_none_or(|previous| {
                            let order = pair_order(txn, handle, candidate, previous);
                            if query.reverse {
                                order.is_gt()
                            } else {
                                order.is_lt()
                            }
                        })
                    {
                        selected = Some(candidate);
                    }
                    pair = txn
                        .physical_pair_after(handle, Some(candidate))
                        .map_err(code)?;
                }
                Ok(selected)
            }
        }
    }
}
fn native_seek<'a>(
    txn: &'a MDB_txn,
    handle: &ResolvedAlias,
    bound: Bound<'_>,
    inclusive: bool,
    reverse: bool,
) -> FfiResult<Option<BorrowedPair<'a>>> {
    let result = txn.seek_database_pair(handle, bound, inclusive, reverse);
    match result {
        Err(Error::MissingDatabase) if handle.name() == MAIN => Ok(None),
        Err(Error::MissingDatabase) => Err(txn.missing_named_code(&handle.alias)?),
        result => result.map_err(code).map(|pair| {
            pair.map(|(key, value)| (key, txn.reserve_value(handle, key).unwrap_or(value)))
        }),
    }
}
fn pair_order(
    txn: &MDB_txn,
    handle: &ResolvedAlias,
    a: BorrowedPair<'_>,
    b: BorrowedPair<'_>,
) -> Ordering {
    txn.key_order(handle, a.0, b.0)
        .then_with(|| txn.value_order(handle, a.1, b.1))
}
fn mapped(value: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: value.len(),
        mv_data: value.as_ptr().cast_mut().cast(),
    }
}

fn bookmark(handle: &ResolvedAlias, pair: BorrowedPair<'_>) -> Pair {
    // KV seeks and deletion/replacement notifications identify a position by
    // key. The returned value already borrows pinned transaction storage.
    // Duplicate traversal additionally needs its (bounded) ordered value.
    (
        pair.0.to_vec(),
        if matches!(handle.kind(), DatabaseKind::Duplicates { .. }) {
            pair.1.to_vec()
        } else {
            Vec::new()
        },
    )
}

pub(super) enum Selection {
    // Raw output views refer to the transaction's pinned storage. Callers must
    // emit them before any engine mutation, exactly as with ordinary mdb_get.
    Mapped {
        key: MDB_val,
        value: MDB_val,
        position: Pair,
    },
    Catalog(Pair),
    CachedCatalog {
        index: Arc<catalog::CatalogIndex>,
        at: usize,
    },
}
impl Selection {
    pub(super) fn position(&self) -> &Pair {
        match self {
            Self::Mapped { position, .. } | Self::Catalog(position) => position,
            Self::CachedCatalog { index, at } => &index.rows[*at].pair,
        }
    }
    pub(super) fn output(
        self,
        txn: &mut MDB_txn,
        key: *mut MDB_val,
        value: *mut MDB_val,
    ) -> FfiResult<Pair> {
        match self {
            Self::Mapped {
                key: stored_key,
                value: stored_value,
                position,
            } => {
                *unsafe { get_mut(key)? } = stored_key;
                *unsafe { get_mut(value)? } = stored_value;
                Ok(position)
            }
            Self::Catalog(position) => {
                let retained = position.clone();
                txn.retired_rows.push(vec![retained]);
                let row = &txn.retired_rows.last().unwrap()[0];
                set_value(key, &row.0)?;
                set_value(value, &row.1)?;
                Ok(position)
            }
            Self::CachedCatalog { index, at } => {
                let row = &index.rows[at].pair;
                set_value(key, &row.0)?;
                set_value(value, &row.1)?;
                // The transaction retains the stored cache winner through
                // reset/end, so subsequent cursor calls need no row copies.
                Ok(row.clone())
            }
        }
    }
}

/// Traversal state owns no borrowed pages and no snapshot/physical-pin owner.
pub(super) struct ReadContinuation {
    alias: AliasId,
    actual: DatabaseHandle,
    revision: u64,
    native: ulmdb::KvPosition,
}

pub(super) enum ReadSelection {
    Mapped {
        key: MDB_val,
        value: MDB_val,
    },
    Catalog {
        index: Arc<catalog::CatalogIndex>,
        at: usize,
    },
}
impl ReadSelection {
    pub(super) fn output(
        self,
        cursor: &mut MDB_cursor,
        key: *mut MDB_val,
        value: *mut MDB_val,
    ) -> FfiResult<()> {
        let (stored_key, stored_value, catalog) = match &self {
            Self::Mapped { key, value } => (*key, *value, false),
            Self::Catalog { index, at } => {
                let row = &index.rows[*at].pair;
                (mapped(&row.0), mapped(&row.1), true)
            }
        };
        let position = cursor
            .position
            .get_or_insert_with(|| (Vec::new(), Vec::new()));
        position.0.clear();
        position.0.extend_from_slice(unsafe { bytes(&stored_key)? });
        position.1.clear();
        if catalog {
            position
                .1
                .extend_from_slice(unsafe { bytes(&stored_value)? });
        }
        *unsafe { get_mut(key)? } = stored_key;
        *unsafe { get_mut(value)? } = stored_value;
        Ok(())
    }
}

fn accepts_bytes(query: Query<'_>, pair: BorrowedPair<'_>) -> bool {
    if query.only_key.is_some_and(|key| key != pair.0)
        || query.exact_value.is_some_and(|value| value != pair.1)
    {
        return false;
    }
    let Some((key, _)) = query.bound else {
        return true;
    };
    let order = pair.0.cmp(key);
    (if query.reverse {
        order.is_lt()
    } else {
        order.is_gt()
    }) || query.inclusive && order.is_eq()
}

/// Outer None keeps the established compatibility path. Inner None is an
/// admitted lookup that found no row. Admission is refreshed for every call.
pub(super) fn continued(
    continuation_state: &mut Option<ReadContinuation>,
    txn: &MDB_txn,
    handle: &ResolvedAlias,
    query: Query<'_>,
    sequential: bool,
    checked_settings: Option<compat_features::Settings>,
) -> FfiResult<Option<Option<ReadSelection>>> {
    let previous = continuation_state.take();
    let Some(State::Read(read)) = txn.state.as_ref() else {
        return Ok(None);
    };
    if handle.kind() != DatabaseKind::Kv
        || !matches!(handle.binding.view, DataView::Direct)
        || read.external_synchronization().is_some()
        || read.transform_context().is_some()
        || read.uses_chunk_cache()
        || !txn.pending_reserves.is_empty()
        || match checked_settings {
            Some(settings) => settings.flags,
            None => txn.settings(handle)?.flags,
        } != 0
    {
        return Ok(None);
    }
    {
        let options = txn.shared.options.lock().map_err(|_| BAD_TXN)?;
        if options
            .get(&compat_features::callback_key(handle))
            .is_some_and(|o| o.key_cmp.is_some())
        {
            return Ok(None);
        }
    }
    let mut continuation = match previous {
        Some(previous)
            if previous.alias == handle.id()
                && previous.actual == *handle.actual()
                && previous.revision == txn.revision
                && previous.native.matches_snapshot(read) =>
        {
            previous
        }
        _ => {
            if !read
                .database_ordering(handle.actual().name())
                .is_ok_and(|order| order.keys == ulmdb::ordering::OrderingSpec::Bytewise)
            {
                return Ok(None);
            }
            ReadContinuation {
                alias: handle.id(),
                actual: handle.actual().clone(),
                revision: txn.revision,
                native: read.kv_position(handle.actual()).map_err(code)?,
            }
        }
    };
    let bound = match query.bound {
        None => std::ops::Bound::Unbounded,
        Some((key, _)) if query.inclusive => std::ops::Bound::Included(key),
        Some((key, _)) => std::ops::Bound::Excluded(key),
    };
    let native = if sequential && query.only_key.is_none() && query.exact_value.is_none() {
        // One fresh native borrow and leaf decode for this operation. Retain
        // an ahead-of-bookmark native candidate when MAIN catalog rows win.
        continuation
            .native
            .current_or_step(read, bound, query.reverse)
            .map_err(code)?
    } else if sequential {
        // Auxiliary filters retain their established rejection/one-step path.
        match continuation.native.current(read).map_err(code)? {
            Some(pair) if accepts_bytes(query, pair) => Some(pair),
            Some(_) => {
                if query.reverse {
                    continuation.native.previous(read).map_err(code)?
                } else {
                    continuation.native.next(read).map_err(code)?
                }
            }
            None => continuation
                .native
                .seek(read, bound, query.reverse)
                .map_err(code)?,
        }
    } else {
        continuation
            .native
            .seek(read, bound, query.reverse)
            .map_err(code)?
    }
    .filter(|pair| accepts_bytes(query, *pair));
    let catalog = if handle.name() == MAIN {
        let index = txn.catalog_index()?;
        let at = match (query.bound, query.reverse) {
            (None, false) => Some(0),
            (None, true) => index.rows.len().checked_sub(1),
            (Some((key, _)), false) => Some(index.rows.partition_point(|row| {
                row.pair.0.as_slice() < key || (!query.inclusive && row.pair.0.as_slice() == key)
            })),
            (Some((key, _)), true) => index
                .rows
                .partition_point(|row| {
                    row.pair.0.as_slice() < key || (query.inclusive && row.pair.0.as_slice() == key)
                })
                .checked_sub(1),
        };
        at.filter(|at| {
            index
                .rows
                .get(*at)
                .is_some_and(|row| accepts_bytes(query, (&row.pair.0, &row.pair.1)))
        })
        .map(|at| (index, at))
    } else {
        None
    };
    let selected = match (native, catalog) {
        (Some(pair), Some((index, at))) => {
            let order = index.rows[at].pair.0.as_slice().cmp(pair.0);
            if if query.reverse {
                order.is_gt()
            } else {
                order.is_lt()
            } {
                Some(ReadSelection::Catalog { index, at })
            } else {
                Some(ReadSelection::Mapped {
                    key: mapped(pair.0),
                    value: mapped(pair.1),
                })
            }
        }
        (Some(pair), None) => Some(ReadSelection::Mapped {
            key: mapped(pair.0),
            value: mapped(pair.1),
        }),
        (None, Some((index, at))) => Some(ReadSelection::Catalog { index, at }),
        (None, None) => None,
    };
    if selected.is_some() {
        *continuation_state = Some(continuation);
    }
    Ok(Some(selected))
}
pub(super) fn select(
    txn: &MDB_txn,
    handle: &ResolvedAlias,
    query: Query<'_>,
) -> FfiResult<Option<Selection>> {
    // Refresh the named descriptor once per query, outside comparator scans.
    txn.refresh_view_alias(handle)?;
    let native = SeekStrategy::choose(txn, handle)?.select(txn, handle, query)?;
    let mut catalog = None;
    if handle.name() == MAIN
        && txn.readonly
        && !txn.custom_key(handle)
        && txn.db_flags(handle)? & (DUPSORT | 2 | 8) == 0
    {
        let index = txn.catalog_index()?;
        let rows = &index.rows;
        let at = match (query.bound, query.reverse) {
            (None, false) => Some(0),
            (None, true) => rows.len().checked_sub(1),
            (Some((key, _)), false) => Some(rows.partition_point(|row| {
                row.pair.0.as_slice() < key || (!query.inclusive && row.pair.0.as_slice() == key)
            })),
            (Some((key, _)), true) => rows
                .partition_point(|row| {
                    row.pair.0.as_slice() < key || (query.inclusive && row.pair.0.as_slice() == key)
                })
                .checked_sub(1),
        };
        if let Some(at) = at.filter(|at| {
            rows.get(*at)
                .is_some_and(|row| query.accepts(txn, handle, (&row.pair.0, &row.pair.1)))
        }) {
            catalog = Some(Selection::CachedCatalog { index, at });
        }
    } else if handle.name() == MAIN {
        let mut selected = None;
        for pair in txn.catalog_pairs()? {
            if query.accepts(txn, handle, (&pair.0, &pair.1))
                && selected.as_ref().is_none_or(|previous: &Pair| {
                    let order =
                        pair_order(txn, handle, (&pair.0, &pair.1), (&previous.0, &previous.1));
                    if query.reverse {
                        order.is_gt()
                    } else {
                        order.is_lt()
                    }
                })
            {
                selected = Some(pair);
            }
        }
        catalog = selected.map(Selection::Catalog);
    }
    match (native, catalog) {
        (Some(native), Some(catalog)) => {
            let position = catalog.position();
            let order = pair_order(txn, handle, (&position.0, &position.1), native);
            if if query.reverse {
                order.is_gt()
            } else {
                order.is_lt()
            } {
                return Ok(Some(catalog));
            }
            Ok(Some(Selection::Mapped {
                key: mapped(native.0),
                value: mapped(native.1),
                position: bookmark(handle, native),
            }))
        }
        (Some(native), None) => Ok(Some(Selection::Mapped {
            key: mapped(native.0),
            value: mapped(native.1),
            position: bookmark(handle, native),
        })),
        (None, Some(catalog)) => Ok(Some(catalog)),
        (None, None) => Ok(None),
    }
}

pub(super) fn duplicate_count(
    txn: &MDB_txn,
    handle: &ResolvedAlias,
    key: &[u8],
) -> FfiResult<usize> {
    if txn.detached_database(handle.id())?.is_some() {
        let mut count = 0usize;
        let mut previous = None;
        loop {
            let bound = previous
                .map(|value| (key, Some(value)))
                .or(Some((key, None)));
            let Some((stored, value)) = txn
                .seek_database_pair(handle, bound, previous.is_none(), false)
                .map_err(code)?
            else {
                break;
            };
            if stored != key {
                break;
            }
            count = count.checked_add(1).ok_or(-30792)?;
            previous = Some(value);
        }
        return Ok(count);
    }
    let count = txn.count_alias_duplicates(handle, key)?;
    usize::try_from(count).map_err(|_| BAD_VALSIZE)
}

pub(super) fn multiple(
    cursor: &mut MDB_cursor,
    txn: &mut MDB_txn,
    handle: &ResolvedAlias,
    key: *mut MDB_val,
    value: *mut MDB_val,
    op: u32,
) -> FfiResult<()> {
    if txn.db_flags(handle)? & 0x10 == 0 {
        return Err(INCOMPATIBLE);
    }
    let current = match cursor.position.as_ref() {
        Some(position) => position.clone(),
        None if op == 10 => select(txn, handle, Query::edge(false))?
            .ok_or(NOTFOUND)?
            .position()
            .clone(),
        None if op == 18 => select(txn, handle, Query::edge(true))?
            .ok_or(NOTFOUND)?
            .position()
            .clone(),
        None => return Err(libc::EINVAL),
    };
    let width = current.1.len();
    if width == 0 {
        return Err(BAD_VALSIZE);
    }
    let capacity = (txn.shared.page_size.get() - 16) / width;
    if capacity == 0 {
        return Err(BAD_VALSIZE);
    }
    let edge = Query::bounded(&current.0, None, true, false).same_key(&current.0);
    let mut candidate = select(txn, handle, edge)?;
    let mut index = 0usize;
    let mut current_index = None;
    while let Some(row) = candidate {
        if txn
            .value_order(handle, &row.position().1, &current.1)
            .is_eq()
        {
            current_index = Some(index);
            break;
        }
        let position = row.position();
        candidate = select(
            txn,
            handle,
            Query::bounded(&position.0, Some(&position.1), false, false).same_key(&current.0),
        )?;
        index += 1;
    }
    let current_index = current_index.ok_or(NOTFOUND)?;
    let block = match op {
        5 => current_index / capacity,
        10 if cursor.position.is_none() => 0,
        10 => (current_index + 1) / capacity,
        18 => (current_index / capacity).checked_sub(1).ok_or(NOTFOUND)?,
        _ => return Err(libc::EINVAL),
    };
    if op == 10
        && cursor.position.is_some()
        && current_index + 1 >= duplicate_count(txn, handle, &current.0)?
    {
        return Err(NOTFOUND);
    }
    let start = block * capacity;
    let mut next = select(txn, handle, edge)?;
    let mut packed = Vec::with_capacity(capacity * width);
    let mut last = None;
    let mut stored_key = None;
    let mut seen = 0usize;
    while let Some(row) = next {
        let position = row.position();
        if seen >= start {
            if position.1.len() != width {
                return Err(BAD_VALSIZE);
            }
            if stored_key.is_none() {
                stored_key = Some(match &row {
                    Selection::Mapped { key, .. } => *key,
                    Selection::Catalog(_) | Selection::CachedCatalog { .. } => {
                        return Err(INCOMPATIBLE);
                    }
                });
            }
            packed.extend_from_slice(&position.1);
            last = Some(position.clone());
            if packed.len() / width == capacity {
                break;
            }
        }
        next = select(
            txn,
            handle,
            Query::bounded(&position.0, Some(&position.1), false, false).same_key(&current.0),
        )?;
        seen += 1;
    }
    let last = last.ok_or(NOTFOUND)?;
    if !cursor.multiple.is_empty() {
        txn.retired_rows
            .push(vec![(Vec::new(), std::mem::take(&mut cursor.multiple))]);
    }
    cursor.multiple = packed;
    cursor.position = Some(last);
    *unsafe { get_mut(key)? } = stored_key.ok_or(NOTFOUND)?;
    set_value(value, &cursor.multiple)
}
