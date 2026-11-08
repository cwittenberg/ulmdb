use super::*;
use crate::cursor_stream::{self as stream, Query};

fn refresh(
    cursor: &mut MDB_cursor,
) -> FfiResult<Option<(ResolvedAlias, compat_features::Settings)>> {
    let txn = unsafe { get_mut(cursor.txn)? };
    txn.flush_reserves()?;
    let handle = match txn.operation_alias_after_live(cursor.dbi)? {
        Some(view_alias::OperationAlias::Cached(handle, settings)) => {
            if !matches!(
                handle.alias.kind(),
                DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
            ) {
                return Err(INCOMPATIBLE);
            }
            cursor.revision = txn.revision;
            return Ok(Some((handle, settings)));
        }
        Some(view_alias::OperationAlias::Uncached(handle, _, _)) => handle,
        None => txn.peek_handle(cursor.dbi)?,
    };
    if !matches!(
        handle.kind(),
        DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
    ) {
        return Err(INCOMPATIBLE);
    }
    txn.settings(&handle)?;
    cursor.revision = txn.revision;
    Ok(None)
}
fn retain_multiple(cursor: &mut MDB_cursor, txn: &mut MDB_txn) {
    if !cursor.multiple.is_empty() {
        txn.retired_rows
            .push(vec![(Vec::new(), std::mem::take(&mut cursor.multiple))]);
    }
}
fn current_query(position: &Pair, duplicates: bool) -> Query<'_> {
    let query = Query::bounded(
        &position.0,
        duplicates.then_some(position.1.as_slice()),
        true,
        false,
    )
    .same_key(&position.0);
    if duplicates {
        query.exact_value(&position.1)
    } else {
        query
    }
}

/// Update active write cursors immediately after a record deletion, before a
/// following insertion can change which record occupies the vacated position.
pub(super) fn refresh_deleted_positions(txn: &mut MDB_txn, dbi: u32) -> FfiResult<()> {
    let handle = txn.handle(dbi)?;
    let duplicates = matches!(handle.kind(), DatabaseKind::Duplicates { .. });
    for &pointer in &txn.cursors {
        let cursor = unsafe { get_mut(pointer)? };
        if txn.peek_handle(cursor.dbi)?.id() != handle.id() {
            continue;
        }
        let Some(old) = cursor.position.as_ref() else {
            continue;
        };
        if stream::select(txn, &handle, current_query(old, duplicates))?.is_some() {
            continue;
        }
        let same_key = duplicates
            && stream::select(
                txn,
                &handle,
                Query::bounded(&old.0, None, true, false).same_key(&old.0),
            )?
            .is_some();
        let mut query =
            Query::bounded(&old.0, duplicates.then_some(old.1.as_slice()), false, false);
        if same_key {
            query = query.same_key(&old.0);
        }
        let next = stream::select(txn, &handle, query)?;
        let previous = old.clone();
        if let Some(next) = next {
            cursor.position = Some(next.position().clone());
        } else if stream::select(txn, &handle, Query::edge(false))?.is_none() {
            cursor.position = None;
        }
        cursor.deleted = true;
        cursor.deleted_from = Some(previous);
    }
    Ok(())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_open(
    txn: *mut MDB_txn,
    dbi: u32,
    out: *mut *mut MDB_cursor,
) -> c_int {
    boundary(|| {
        let out = unsafe { get_mut(out)? };
        *out = ptr::null_mut();
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        let metadata = t.peek_handle(dbi)?;
        match t.handle(dbi) {
            Ok(_) | Err(NOTFOUND | BAD_DBI | INCOMPATIBLE) => {}
            Err(error) => return Err(error),
        }
        if !matches!(
            metadata.kind(),
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
        ) {
            return Err(INCOMPATIBLE);
        }
        t.settings(&metadata)?;
        let cursor = Box::into_raw(Box::new(MDB_cursor {
            txn,
            dbi,
            shared: t.shared.clone(),
            revision: t.revision,
            position: None,
            deleted: false,
            deleted_from: None,
            multiple: Vec::new(),
            continuation: None,
        }));
        t.cursors.push(cursor);
        *out = cursor;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_close(cursor: *mut MDB_cursor) {
    if cursor.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let borrowed = unsafe { &*cursor };
        if let Some(txn) = unsafe { borrowed.txn.as_ref() }
            && !txn.correct_thread()
        {
            return;
        }
        let mut cursor = unsafe { Box::from_raw(cursor) };
        if let Some(txn) = unsafe { cursor.txn.as_mut() } {
            retain_multiple(&mut cursor, txn);
            txn.cursors
                .retain(|pointer| *pointer != (&*cursor as *const MDB_cursor).cast_mut());
        }
    }));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_renew(txn: *mut MDB_txn, cursor: *mut MDB_cursor) -> c_int {
    boundary(|| {
        let c = unsafe { get_mut(cursor)? };
        let t = unsafe { get_mut(txn)? };
        t.flush_reserves()?;
        t.live()?;
        if !t.readonly || !Arc::ptr_eq(&c.shared, &t.shared) {
            return Err(libc::EINVAL);
        }
        let handle = t.handle(c.dbi)?;
        t.settings(&handle)?;
        if c.txn == txn {
            retain_multiple(c, t);
            t.cursors.retain(|pointer| *pointer != cursor);
        } else if let Some(old) = unsafe { c.txn.as_mut() } {
            if !old.correct_thread() {
                return Err(BAD_TXN);
            }
            retain_multiple(c, old);
            old.cursors.retain(|pointer| *pointer != cursor);
        }
        c.txn = txn;
        c.multiple.clear();
        c.revision = t.revision;
        c.position = None;
        c.continuation = None;
        c.deleted = false;
        c.deleted_from = None;
        t.cursors.push(cursor);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_txn(cursor: *mut MDB_cursor) -> *mut MDB_txn {
    unsafe { cursor.as_ref() }.map_or(ptr::null_mut(), |c| c.txn)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_dbi(cursor: *mut MDB_cursor) -> u32 {
    unsafe { cursor.as_ref() }.map_or(0, |c| c.dbi)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_get(
    cursor: *mut MDB_cursor,
    key: *mut MDB_val,
    value: *mut MDB_val,
    op: c_uint,
) -> c_int {
    boundary(|| {
        let c = unsafe { get_mut(cursor)? };
        let admitted = refresh(c)?;
        let t = unsafe { get_mut(c.txn)? };
        let (h, checked_settings) = match admitted {
            Some((handle, settings)) => (handle, Some(settings)),
            None => (t.handle(c.dbi)?, None),
        };
        let duplicates = matches!(h.kind(), DatabaseKind::Duplicates { .. });
        if matches!(op, 1 | 2 | 3 | 7) && !duplicates {
            return Err(INCOMPATIBLE);
        }
        unsafe { get_mut(key)? };
        unsafe { get_mut(value)? };
        if matches!(op, 5 | 10 | 18) {
            let result = stream::multiple(c, t, &h, key, value, op);
            if result.is_ok() {
                c.deleted = false;
                c.deleted_from = None;
            }
            return result;
        }
        let inputkey = if matches!(op, 2 | 3 | 15 | 16 | 17) {
            let k = unsafe { bytes(key)? }.to_vec();
            match checked_settings {
                Some(settings) => t.check_key_settings(&k, settings)?,
                None => t.check_sizes(&h, &k, None, false)?,
            }
            k
        } else {
            Vec::new()
        };
        let inputvalue = if matches!(op, 2 | 3) {
            unsafe { bytes(value)? }.to_vec()
        } else {
            Vec::new()
        };
        let position = c.position.as_ref();
        let deleted_current = if c.deleted {
            match position {
                Some(position) => {
                    stream::select(t, &h, current_query(position, duplicates))?.is_some()
                }
                None => false,
            }
        } else {
            false
        };
        let deleted_key_changed = c
            .deleted_from
            .as_ref()
            .zip(position)
            .is_some_and(|(old, current)| !t.key_order(&h, &old.0, &current.0).is_eq());
        let query = match op {
            0 => Query::edge(false),
            6 => Query::edge(true),
            1 | 7 => {
                let p = position.ok_or(libc::EINVAL)?;
                Query::bounded(&p.0, None, true, op == 7).same_key(&p.0)
            }
            2 | 3 => {
                let q =
                    Query::bounded(&inputkey, Some(&inputvalue), true, false).same_key(&inputkey);
                if op == 2 {
                    q.exact_value(&inputvalue)
                } else {
                    q
                }
            }
            4 => {
                let p = position.ok_or(libc::EINVAL)?;
                current_query(p, duplicates)
            }
            8 | 12 => match position {
                None => Query::edge(op == 12),
                Some(p) if op == 8 && deleted_current => current_query(p, duplicates),
                Some(p) => {
                    Query::bounded(&p.0, duplicates.then_some(p.1.as_slice()), false, op == 12)
                }
            },
            9 | 13 => {
                if !duplicates {
                    return Err(NOTFOUND);
                }
                let p = position.ok_or(NOTFOUND)?;
                if op == 9 && c.deleted && deleted_key_changed {
                    return Err(NOTFOUND);
                }
                if op == 9 && deleted_current {
                    current_query(p, true)
                } else {
                    Query::bounded(&p.0, Some(&p.1), false, op == 13).same_key(&p.0)
                }
            }
            11 | 14 => match position {
                None => Query::edge(op == 14),
                Some(p) if op == 11 && deleted_current && deleted_key_changed => {
                    current_query(p, duplicates)
                }
                Some(p) => Query::bounded(&p.0, None, false, op == 14),
            },
            15 | 16 => Query::bounded(&inputkey, None, true, false).same_key(&inputkey),
            17 => Query::bounded(&inputkey, None, true, false),
            _ => return Err(libc::EINVAL),
        };
        let original_key = if op == 15 {
            Some(MDB_val {
                mv_size: unsafe { (*key).mv_size },
                mv_data: unsafe { (*key).mv_data },
            })
        } else {
            None
        };
        if let Some(candidate) = stream::continued(
            &mut c.continuation,
            t,
            &h,
            query,
            matches!(op, 8 | 11 | 12 | 14),
            checked_settings,
        )? {
            candidate.ok_or(NOTFOUND)?.output(c, key, value)?;
            if let Some(original_key) = original_key {
                unsafe { *key = original_key };
            }
            c.deleted = false;
            c.deleted_from = None;
            return Ok(());
        }
        let candidate = stream::select(t, &h, query)?.ok_or(NOTFOUND)?;
        // Compatibility callbacks retain their original observation point.
        let original_key = if op == 15 {
            Some(MDB_val {
                mv_size: unsafe { (*key).mv_size },
                mv_data: unsafe { (*key).mv_data },
            })
        } else {
            None
        };
        let position = candidate.output(t, key, value)?;
        if let Some(original_key) = original_key {
            // MDB_SET preserves the caller's key view; MDB_SET_KEY returns it.
            unsafe { *key = original_key };
        }
        c.position = Some(position);
        c.deleted = false;
        c.deleted_from = None;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_put(
    cursor: *mut MDB_cursor,
    key: *mut MDB_val,
    value: *mut MDB_val,
    flags: u32,
) -> c_int {
    boundary(|| {
        let c = unsafe { get_mut(cursor)? };
        refresh(c)?;
        if flags & !(0x10 | 0x20 | 0x40 | 0x10000 | 0x20000 | 0x40000 | 0x80000) != 0
            || flags & (0x40 | 0x80000) == (0x40 | 0x80000)
        {
            return Err(libc::EINVAL);
        }
        let t = unsafe { get_mut(c.txn)? };
        if t.readonly {
            return Err(t.shared.readonly_error());
        }
        t.check_file_value_size(unsafe { get_ref(value)? }.mv_size)?;
        let k = unsafe { bytes(key)? }.to_vec();
        let replaced = (flags & 0x40 != 0).then(|| c.position.clone()).flatten();
        let v = if flags & 0x10000 != 0 {
            Vec::new()
        } else {
            unsafe { bytes(value)? }.to_vec()
        };
        if flags & 0x40 != 0 {
            let p = c.position.as_ref().ok_or(libc::EINVAL)?;
            let t = unsafe { get_mut(c.txn)? };
            t.finish_reserves()?;
            let h = t.handle(c.dbi)?;
            if !t.key_order(&h, &p.0, &k).is_eq() {
                return Err(libc::EINVAL);
            }
            if matches!(h.kind(), DatabaseKind::Duplicates { .. }) {
                if flags & 0x10000 != 0 {
                    return Err(INCOMPATIBLE);
                }
                t.check_sizes(&h, &k, Some(&v), true)?;
                stream::select(
                    t,
                    &h,
                    Query::bounded(&p.0, Some(&p.1), true, false)
                        .same_key(&p.0)
                        .exact_value(&p.1),
                )?
                .ok_or(NOTFOUND)?;
                let previous = stream::select(
                    t,
                    &h,
                    Query::bounded(&p.0, Some(&p.1), false, true).same_key(&p.0),
                )?;
                let next = stream::select(
                    t,
                    &h,
                    Query::bounded(&p.0, Some(&p.1), false, false).same_key(&p.0),
                )?;
                if previous
                    .as_ref()
                    .is_some_and(|row| !t.value_order(&h, &row.position().1, &v).is_lt())
                    || next
                        .as_ref()
                        .is_some_and(|row| !t.value_order(&h, &row.position().1, &v).is_gt())
                {
                    return Err(libc::EINVAL);
                }
                // Main-catalog clearing can retain this cursor's read view
                // after removing the native database. Check before deleting.
                t.delete_alias_duplicate(&h, &p.0, &p.1)?;
                t.mark_database_dirty(&h)?;
                t.quota_used = None;
                t.changed();
            }
        }
        let result = if flags & 0x40 != 0 {
            let mut copied_key = MDB_val {
                mv_size: k.len(),
                mv_data: k.as_ptr().cast_mut().cast(),
            };
            let mut copied_value = MDB_val {
                mv_size: v.len(),
                mv_data: v.as_ptr().cast_mut().cast(),
            };
            unsafe {
                operations::put_value(
                    c.txn,
                    c.dbi,
                    &mut copied_key,
                    if flags & 0x10000 != 0 {
                        value
                    } else {
                        &mut copied_value
                    },
                    flags & (0x20 | 0x10000),
                )
            }
        } else {
            unsafe { operations::put_value(c.txn, c.dbi, key, value, flags) }
        };
        if result == 0 || result == KEYEXIST {
            let t = unsafe { get_mut(c.txn)? };
            let h = t.handle(c.dbi)?;
            let duplicates = matches!(h.kind(), DatabaseKind::Duplicates { .. });
            let attempted = if duplicates && flags & 0x80000 != 0 {
                unsafe { bytes(value)? }.to_vec()
            } else {
                v
            };
            let query = if result == KEYEXIST && flags & 0x20000 != 0 {
                Query::edge(true)
            } else if result == KEYEXIST && flags & 0x10 != 0 {
                Query::bounded(&k, None, true, false).same_key(&k)
            } else if result == KEYEXIST && flags & 0x40000 != 0 {
                Query::bounded(&k, None, true, true).same_key(&k)
            } else if duplicates {
                Query::bounded(&k, Some(&attempted), true, false)
                    .same_key(&k)
                    .exact_value(&attempted)
            } else {
                Query::bounded(&k, None, true, false).same_key(&k)
            };
            if let Some(position) = stream::select(t, &h, query)? {
                let position = position.position().clone();
                if let Some(old) = replaced.as_ref().filter(|_| duplicates) {
                    for &pointer in &t.cursors {
                        if pointer == cursor {
                            continue;
                        }
                        let other = unsafe { get_mut(pointer)? };
                        if other.dbi == c.dbi
                            && other.position.as_ref().is_some_and(|other| {
                                t.key_order(&h, &other.0, &old.0).is_eq()
                                    && t.value_order(&h, &other.1, &old.1).is_eq()
                            })
                        {
                            other.position = Some(position.clone());
                            other.deleted = false;
                            other.deleted_from = None;
                        }
                    }
                }
                c.position = Some(position);
                c.deleted = false;
                c.deleted_from = None;
            }
        }
        if result != 0 {
            return Err(result);
        }
        c.deleted = false;
        c.deleted_from = None;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_del(cursor: *mut MDB_cursor, flags: u32) -> c_int {
    boundary(|| {
        if flags & !0x20 != 0 {
            return Err(libc::EINVAL);
        }
        let (txn, dbi, k, v) = {
            let c = unsafe { get_mut(cursor)? };
            refresh(c)?;
            let (k, v) = c.position.as_ref().ok_or(libc::EINVAL)?;
            (c.txn, c.dbi, k.clone(), v.clone())
        };
        let mut key = MDB_val {
            mv_size: k.len(),
            mv_data: k.as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: v.len(),
            mv_data: v.as_ptr().cast_mut().cast(),
        };
        let result = unsafe {
            operations::mdb_del(
                txn,
                dbi,
                &mut key,
                if flags & 0x20 != 0 {
                    ptr::null_mut()
                } else {
                    &mut value
                },
            )
        };
        if result != 0 {
            return Err(result);
        }
        // mdb_del's notification can mutably access every registered cursor;
        // no reference to this cursor is retained across that call.
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_cursor_count(cursor: *mut MDB_cursor, out: *mut usize) -> c_int {
    boundary(|| {
        let c = unsafe { get_mut(cursor)? };
        refresh(c)?;
        let t = unsafe { get_mut(c.txn)? };
        let h = t.handle(c.dbi)?;
        if !matches!(h.kind(), DatabaseKind::Duplicates { .. }) {
            return Err(INCOMPATIBLE);
        }
        let p = c.position.as_ref().ok_or(libc::EINVAL)?;
        *unsafe { get_mut(out)? } = stream::duplicate_count(t, &h, &p.0)?;
        Ok(())
    })
}

#[cfg(test)]
#[path = "cursor_read_tests.rs"]
mod read_tests;
