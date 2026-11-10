//! Private optional SQL scan ABI. The actual transaction owns every pin and
//! value; the existing cursor owns only its native position/bookmark.
use super::mapped;
use crate::*;
use std::ops::Bound;

#[path = "sql_write.rs"]
pub(crate) mod sql_write;

#[cfg(feature = "sql-hash")]
#[path = "sql_hash.rs"]
pub(crate) mod sql_hash;

const MAX_ROWS: usize = 1024;
const MAX_BYTES: usize = 64 << 20;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SqlRow {
    key: MDB_val,
    value: MDB_val,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SqlHit {
    id: i64,
    value: MDB_val,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_sql_get_batch(
    txn: *mut MDB_txn,
    dbi: u32,
    ids: *const i64,
    candidates: usize,
    hits: *mut SqlHit,
    capacity: usize,
    consumed: *mut usize,
    found: *mut usize,
) -> c_int {
    unsafe {
        get_batch(
            txn, dbi, ids, candidates, hits, capacity, consumed, found, MAX_BYTES,
        )
    }
}

unsafe fn get_batch(
    txn: *mut MDB_txn,
    dbi: u32,
    ids: *const i64,
    candidates: usize,
    hits: *mut SqlHit,
    capacity: usize,
    consumed: *mut usize,
    found: *mut usize,
    byte_limit: usize,
) -> c_int {
    boundary(|| {
        let consumed = unsafe { get_mut(consumed)? };
        let found = unsafe { get_mut(found)? };
        *consumed = 0;
        *found = 0;
        if candidates > MAX_ROWS
            || capacity > MAX_ROWS
            || (candidates != 0 && (ids.is_null() || hits.is_null() || capacity == 0))
        {
            return Err(libc::EINVAL);
        }
        let txn = unsafe { get_mut(txn)? };
        let (handle, settings) = admit(txn, dbi)?;
        // Every SQL key has exactly eight bytes; preserve C-specific limits too.
        txn.check_key_settings(&[0; 8], settings)?;
        let Some(State::Read(read)) = txn.state.as_ref() else {
            return Err(INCOMPATIBLE);
        };
        let view = read.kv_handle(handle.actual()).map_err(code)?;
        let mut visited = 0;
        let mut written = 0;
        let mut bytes_used = 0usize;
        while visited < candidates && written < capacity {
            let id = unsafe { ids.add(visited).read() };
            let key = ((id as u64) ^ 0x8000_0000_0000_0000).to_be_bytes();
            #[cfg(feature = "sql-hash")]
            let value = match read.get_after_admission(handle.actual(), &key)? {
                Some(value) => value,
                None => view.get(&key).map_err(code)?.map(mapped),
            };
            #[cfg(not(feature = "sql-hash"))]
            let value = view.get(&key).map_err(code)?.map(mapped);
            if let Some(value) = value {
                let bytes = value.mv_size.checked_add(8).ok_or(BAD_VALSIZE)?;
                let total = bytes_used.checked_add(bytes).ok_or(BAD_VALSIZE)?;
                if total > byte_limit {
                    if written == 0 {
                        return Err(BAD_VALSIZE);
                    }
                    break;
                }
                unsafe {
                    hits.add(written).write(SqlHit { id, value });
                }
                bytes_used = total;
                written += 1;
            }
            visited += 1;
        }
        *consumed = visited;
        *found = written;
        Ok(())
    })
}

fn admit(txn: &mut MDB_txn, dbi: u32) -> FfiResult<(ResolvedAlias, compat_features::Settings)> {
    // This must precede every lock, alias lookup and continuation mutation.
    txn.live()?;
    let Some(State::Read(read)) = txn.state.as_ref() else {
        return Err(INCOMPATIBLE);
    };
    if !txn.pending_reserves.is_empty()
        || read.external_synchronization().is_some()
        || read.transform_context().is_some()
        || read.uses_chunk_cache()
    {
        return Err(INCOMPATIBLE);
    }
    bytewise_format_after_live(txn, dbi)
}

/// Format admission also supports a top-level writer before its first mutation.
/// It changes neither the native ordering nor any stored records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_sql_bytewise_format(txn: *mut MDB_txn, dbi: u32) -> c_int {
    boundary(|| {
        let txn = unsafe { get_mut(txn)? };
        // Process/thread/active checks precede every options or DBI lock.
        txn.live()?;
        if !matches!(txn.state.as_ref(), Some(State::Read(_) | State::Write(_)))
            || !txn.pending_reserves.is_empty()
        {
            return Err(INCOMPATIBLE);
        }
        bytewise_format_after_live(txn, dbi).map(|_| ())
    })
}

fn bytewise_format_after_live(
    txn: &mut MDB_txn,
    dbi: u32,
) -> FfiResult<(ResolvedAlias, compat_features::Settings)> {
    let (handle, settings) = match txn.operation_alias_after_live(dbi)? {
        Some(view_alias::OperationAlias::Cached(handle, settings)) => (handle, settings),
        _ => {
            let handle = txn.handle(dbi)?;
            let settings = txn.settings(&handle)?;
            (handle, settings)
        }
    };
    let ordinary_view = matches!(handle.binding.view, DataView::Direct)
        || (matches!(txn.state.as_ref(), Some(State::Write(_)))
            && matches!(handle.binding.view, DataView::Writable(_)));
    if handle.name() == MAIN
        || handle.kind() != DatabaseKind::Kv
        || !ordinary_view
        || settings.flags != 0
    {
        return Err(INCOMPATIBLE);
    }
    {
        let options = txn.shared.options.lock().map_err(|_| BAD_TXN)?;
        if options
            .get(&compat_features::callback_key(&handle))
            .is_some_and(|options| {
                options.key_cmp.is_some()
                    || options.dup_cmp.is_some()
                    || options.native_key.is_some()
                    || options.native_dup.is_some()
                    || options.relfunc != 0
            })
        {
            return Err(INCOMPATIBLE);
        }
    }
    let ordering = txn.actual_native_order(&handle)?;
    if ordering.keys != ulmdb::ordering::OrderingSpec::Bytewise
        || ordering.duplicates != ulmdb::ordering::OrderingSpec::Bytewise
    {
        return Err(INCOMPATIBLE);
    }
    Ok((handle, settings))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ulmdb_sql_cursor_batch(
    cursor: *mut MDB_cursor,
    rows: *mut SqlRow,
    capacity: usize,
    count: *mut usize,
    operation: c_uint,
) -> c_int {
    unsafe { batch(cursor, rows, capacity, count, operation, MAX_BYTES) }
}

unsafe fn batch(
    cursor: *mut MDB_cursor,
    rows: *mut SqlRow,
    capacity: usize,
    count: *mut usize,
    operation: c_uint,
    byte_limit: usize,
) -> c_int {
    boundary(|| {
        let count = unsafe { get_mut(count)? };
        *count = 0;
        if rows.is_null() || capacity == 0 || capacity > MAX_ROWS || !matches!(operation, 0 | 8) {
            return Err(libc::EINVAL);
        }
        let cursor = unsafe { get_mut(cursor)? };
        let txn = unsafe { get_mut(cursor.txn)? };
        let (handle, _) = admit(txn, cursor.dbi)?;
        let Some(State::Read(read)) = txn.state.as_ref() else {
            return Err(INCOMPATIBLE);
        };
        // Bind the genuine snapshot/store once for this call. The native cursor
        // retains its decoded leaf locally; no borrowed cursor escapes the ABI.
        // Preserve both existing bookmarks on unsupported/zero-output failures.
        let bound = if operation == 0 {
            Bound::Unbounded
        } else {
            cursor
                .position
                .as_ref()
                .map_or(Bound::Unbounded, |position| {
                    Bound::Excluded(position.0.as_slice())
                })
        };
        let view = read.kv_handle(handle.actual()).map_err(code)?;
        let mut native = view.cursor(bound, Bound::Unbounded, false).map_err(code)?;
        let mut written = 0;
        let mut bytes_used = 0usize;
        while let Some(pair) = native.next() {
            let (key, value) = pair.map_err(code)?;
            let bytes = key.len().checked_add(value.len()).ok_or(BAD_VALSIZE)?;
            let total = bytes_used.checked_add(bytes).ok_or(BAD_VALSIZE)?;
            if total > byte_limit {
                if written == 0 {
                    return Err(BAD_VALSIZE);
                }
                // The next call re-seeks after the last emitted generic key.
                break;
            }
            unsafe {
                rows.add(written).write(SqlRow {
                    key: mapped(key),
                    value: mapped(value),
                });
            }
            written += 1;
            bytes_used = total;
            if written == capacity {
                break;
            }
        }
        if written == 0 {
            return Err(NOTFOUND);
        }
        let last = unsafe { rows.add(written - 1).read() };
        let bookmark = cursor
            .position
            .get_or_insert_with(|| (Vec::new(), Vec::new()));
        bookmark.0.clear();
        bookmark.0.extend_from_slice(unsafe { bytes(&last.key)? });
        bookmark.1.clear();
        cursor.continuation = None;
        cursor.revision = txn.revision;
        cursor.deleted = false;
        cursor.deleted_from = None;
        *count = written;
        Ok(())
    })
}

#[cfg(test)]
#[path = "sql_batch_tests.rs"]
mod tests;
