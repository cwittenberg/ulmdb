//! Explicit optional C lookup strategy; one real read owns all borrowed indexes.
use crate::*;
use std::{cell::RefCell, collections::BTreeMap, ops::Deref};
use ulmdb::lookup::{HashLimits, Options, PreparedLookup};

const MAX_DATABASES: usize = 256;
const MAX_NAME_BYTES: usize = 256 * 255;
const MAX_ENTRIES: usize = 65_536;
const MAX_INDEX_BYTES: usize = 64 << 20;
const MAX_VALUE_BYTES: usize = 16 << 20;
const MAX_TOTAL_VALUE_BYTES: usize = 64 << 20;

struct ReadOwner {
    read: ReadTxn,
    names: Vec<Box<[u8]>>,
}

enum Entry<'a> {
    Prepared {
        handle: DatabaseHandle,
        lookup: PreparedLookup<'a>,
    },
    Refused,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct SqlHashStats {
    pub prepared_databases: usize,
    pub refused_databases: usize,
    pub preparations: usize,
    pub entries: usize,
    pub index_bytes: usize,
    pub total_value_bytes: usize,
}

struct Cache<'a> {
    databases: BTreeMap<usize, Entry<'a>>,
    stats: SqlHashStats,
    limits: HashLimits,
}

struct Dependent<'a>(RefCell<Cache<'a>>);

self_cell::self_cell!(
    struct ReadCell {
        owner: ReadOwner,
        #[not_covariant]
        dependent: Dependent,
    }
);

pub(crate) struct SqlReadTxn {
    cell: ReadCell,
}

impl SqlReadTxn {
    pub(crate) fn try_new(read: ReadTxn) -> FfiResult<Self> {
        // Bound the catalog allocation before cloning any names. This is done
        // before begin/renew publishes the C transaction, never by moving an
        // already-live read out from under previously returned value pointers.
        let names = if read.external_synchronization().is_none()
            && read.transform_context().is_none()
            && !read.uses_chunk_cache()
            && read.stats().databases <= MAX_DATABASES
        {
            let names = read.database_names().map_err(code)?;
            if names.len() > MAX_DATABASES
                || names.iter().any(|name| name.is_empty() || name.len() > 255)
                || names.iter().map(Vec::len).sum::<usize>() > MAX_NAME_BYTES
            {
                return Err(code(Error::Corrupt("lookup name inventory mismatch")));
            }
            names.into_iter().map(Vec::into_boxed_slice).collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            cell: ReadCell::new(ReadOwner { read, names }, |_| {
                Dependent(RefCell::new(Cache {
                    databases: BTreeMap::new(),
                    stats: SqlHashStats::default(),
                    limits: HashLimits {
                        max_entries: MAX_ENTRIES,
                        max_index_bytes: MAX_INDEX_BYTES,
                        max_value_bytes: MAX_VALUE_BYTES,
                        max_total_value_bytes: MAX_TOTAL_VALUE_BYTES,
                    },
                }))
            }),
        })
    }
}

impl Deref for SqlReadTxn {
    type Target = ReadTxn;
    fn deref(&self) -> &Self::Target {
        &self.cell.borrow_owner().read
    }
}

impl SqlReadTxn {
    /// Called only after this C operation's live, alias, source, callbacks,
    /// native-order and key-limit admission. None means use the native path;
    /// Some(None) is an indexed miss. Only raw C value views leave the closure.
    pub(crate) fn get_after_admission(
        &self,
        handle: &DatabaseHandle,
        key: &[u8],
    ) -> FfiResult<Option<Option<MDB_val>>> {
        self.cell.with_dependent(|owner, dependent| {
            let Ok(index) = owner
                .names
                .binary_search_by(|name0| name0.as_ref().cmp(handle.name()))
            else {
                return Ok(None);
            };
            let mut cache = dependent.0.try_borrow_mut().map_err(|_| BAD_TXN)?;
            if !cache.databases.contains_key(&index) {
                // Validate owner/ID/kind before name-based preparation. Later
                // hits compare the full handle with this admitted identity.
                owner.read.kv_handle(handle).map_err(code)?;
                cache.stats.preparations += 1;
                let limits = HashLimits {
                    max_entries: cache.limits.max_entries.saturating_sub(cache.stats.entries),
                    max_index_bytes: cache
                        .limits
                        .max_index_bytes
                        .saturating_sub(cache.stats.index_bytes),
                    max_value_bytes: cache.limits.max_value_bytes,
                    max_total_value_bytes: cache
                        .limits
                        .max_total_value_bytes
                        .saturating_sub(cache.stats.total_value_bytes),
                };
                match owner
                    .read
                    .prepare_lookup(&owner.names[index], Options::Hash(limits))
                {
                    Ok(lookup) => {
                        let stats = lookup.stats();
                        cache.stats.prepared_databases += 1;
                        cache.stats.entries += stats.entries;
                        cache.stats.index_bytes += stats.index_bytes;
                        cache.stats.total_value_bytes += stats.total_value_bytes;
                        cache.databases.insert(
                            index,
                            Entry::Prepared {
                                handle: handle.clone(),
                                lookup,
                            },
                        );
                    }
                    Err(
                        Error::LimitExceeded
                        | Error::InvalidInput("hash lookup requires bytewise ordering")
                        | Error::InvalidInput(
                            "hash lookup requires immutable untransformed managed storage",
                        ),
                    ) => {
                        cache.stats.refused_databases += 1;
                        cache.databases.insert(index, Entry::Refused);
                    }
                    Err(error) => return Err(code(error)),
                }
            }
            match cache.databases.get(&index).unwrap() {
                Entry::Refused => Ok(None),
                Entry::Prepared {
                    handle: admitted,
                    lookup,
                } => {
                    if admitted != handle {
                        return Err(code(Error::StaleHandle));
                    }
                    Ok(Some(
                        lookup
                            .get(key)
                            .map_err(code)?
                            .map(crate::cursor_stream::mapped),
                    ))
                }
            }
        })
    }

    fn hash_stats(&self) -> FfiResult<SqlHashStats> {
        self.cell
            .with_dependent(|_, dependent| Ok(dependent.0.try_borrow().map_err(|_| BAD_TXN)?.stats))
    }
}

/// The mdb_get caller has already checked live/alias/key limits and reserves.
/// No callback occurs between those checks and this independent cache admission.
pub(crate) fn ordinary_get(
    txn: &MDB_txn,
    handle: &ResolvedAlias,
    key: &[u8],
    settings: compat_features::Settings,
) -> FfiResult<Option<Option<MDB_val>>> {
    if handle.is_main()
        || handle.kind() != DatabaseKind::Kv
        || settings.flags != 0
        || !matches!(handle.binding.view, DataView::Direct)
        || !txn.pending_reserves.is_empty()
    {
        return Ok(None);
    }
    let Some(State::Read(read)) = txn.state.as_ref() else {
        return Ok(None);
    };
    if read.external_synchronization().is_some()
        || read.transform_context().is_some()
        || read.uses_chunk_cache()
    {
        return Ok(None);
    }
    let Ok(ordering) = txn.actual_native_order(handle) else {
        return Ok(None);
    };
    if ordering.keys != ulmdb::ordering::OrderingSpec::Bytewise
        || ordering.duplicates != ulmdb::ordering::OrderingSpec::Bytewise
    {
        return Ok(None);
    }
    {
        let options = txn.shared.options.lock().map_err(|_| BAD_TXN)?;
        if options
            .get(&compat_features::callback_key(handle))
            .is_some_and(|options| {
                options.key_cmp.is_some()
                    || options.dup_cmp.is_some()
                    || options.native_key.is_some()
                    || options.native_dup.is_some()
                    || options.relfunc != 0
            })
        {
            return Ok(None);
        }
    }
    read.get_after_admission(handle.actual(), key)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_sql_hash_stats(txn: *mut MDB_txn, out: *mut SqlHashStats) -> c_int {
    boundary(|| {
        let txn = unsafe { get_mut(txn)? };
        txn.live()?;
        let out = unsafe { get_mut(out)? };
        let Some(State::Read(read)) = txn.state.as_ref() else {
            return Err(INCOMPATIBLE);
        };
        *out = read.hash_stats()?;
        Ok(())
    })
}

#[cfg(test)]
#[path = "sql_hash_tests.rs"]
mod tests;
