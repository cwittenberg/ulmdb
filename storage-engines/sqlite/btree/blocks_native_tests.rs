use super::*;
use std::collections::BTreeMap;

fn payload(id: i64, size: usize) -> Vec<u8> {
    let seed = encoded(id);
    (0..size)
        .map(|at| seed[at & 7].wrapping_add((at % 251) as u8))
        .collect()
}

fn model() -> BTreeMap<i64, Vec<u8>> {
    let mut rows = BTreeMap::from([(1, b"old".to_vec())]);
    let sizes = [0, 3, 64, 1024, 8192, 32_768, 65_569];
    for n in -70_i64..=70 {
        let id = n * 17;
        rows.insert(id, payload(id, sizes[(n + 70) as usize % sizes.len()]));
    }
    rows.insert(i64::MIN, payload(i64::MIN, 64));
    rows.insert(i64::MAX, payload(i64::MAX, 2048));
    rows
}

fn digest(rows: &[(i64, Vec<u8>)]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for (id, value) in rows {
        for byte in encoded(*id)
            .into_iter()
            .chain((value.len() as u64).to_be_bytes())
            .chain(value.iter().copied())
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    hash
}

unsafe fn check_rows(txn: *mut Txn, rows: &BTreeMap<i64, Vec<u8>>) {
    let mut cursor = ptr::null_mut();
    assert_eq!(unsafe { ubt_cursor_open(txn, 3, &mut cursor) }, 0);
    let mut actual = Vec::new();
    let mut next = unsafe { cursor_row(cursor, 0, None) };
    while let Ok(row) = next {
        assert_eq!(unsafe { cursor_row(cursor, 4, None) }, Ok(row.clone()));
        actual.push(row);
        next = unsafe { cursor_row(cursor, 8, None) };
    }
    assert_eq!(next, Err(NOTFOUND));
    let expected: Vec<_> = rows
        .iter()
        .map(|(id, value)| (*id, value.clone()))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(digest(&actual), digest(&expected));
    if let Some(last) = expected.last() {
        assert_eq!(unsafe { cursor_row(cursor, 4, None) }, Ok(last.clone()));
    }
    let mut reverse = Vec::new();
    let mut next = unsafe { cursor_row(cursor, 6, None) };
    while let Ok(row) = next {
        reverse.push(row);
        next = unsafe { cursor_row(cursor, 12, None) };
    }
    assert_eq!(next, Err(NOTFOUND));
    reverse.reverse();
    assert_eq!(reverse, expected);
    unsafe { ubt_cursor_close(cursor) };
}

#[test]
fn blocks_native_commit_rejects_foreign_reservation_owner_before_publication() {
    let f = Fixture::new();
    let foreign = Fixture::new();
    unsafe {
        f.initialize();
        foreign.initialize();
        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(put(w, 3, 1, b"must not publish"), 0);
        let root = match &(*w).state {
            State::Write(root) => root.clone(),
            _ => unreachable!(),
        };
        let origin = (*f.env).owner.as_ref().unwrap().clone();
        let other = (*foreign.env).owner.as_ref().unwrap().clone();
        let origin_head = origin.engine.begin_read().sequence();
        let other_head = other.engine.begin_read().sequence();
        let saved = std::mem::replace(&mut root.borrow_mut()._reservation.0, other.clone());
        assert!(Arc::ptr_eq(&saved, &origin));
        let result = root.borrow().commit();
        // Restore the genuine reservation before any assertion can unwind.
        let displaced = std::mem::replace(&mut root.borrow_mut()._reservation.0, saved);
        assert!(Arc::ptr_eq(&displaced, &other));
        assert_eq!(result, Err(BAD_TXN));
        assert_eq!(origin.engine.begin_read().sequence(), origin_head);
        assert_eq!(other.engine.begin_read().sequence(), other_head);
        drop(root);
        ubt_txn_abort(w);
        let r = f.begin(ptr::null_mut(), true);
        assert_eq!(get(r, 1), Ok(b"old".to_vec()));
        ubt_txn_abort(r);
        let next = f.begin(ptr::null_mut(), false);
        ubt_txn_abort(next);
    }
}

#[test]
fn blocks_native_commit_rejects_a_genuinely_advanced_base_head() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(put(w, 3, 1, b"stale SQL delta"), 0);
        let root = match &(*w).state {
            State::Write(root) => root.clone(),
            _ => unreachable!(),
        };
        let owner = (*f.env).owner.as_ref().unwrap().clone();
        let base_head = root.borrow().base.read.sequence();
        // This real native publication deliberately bypasses only the private
        // SQL reservation, while retaining the same actual owner and format.
        let mut native_write = owner.engine.begin_write().unwrap();
        // Same-value-only puts are a native no-op. A real A->B->A transaction
        // publishes a new head while keeping the visible format unchanged.
        native_write
            .put(DATA, &format_key(), b"uncommitted test format transition")
            .unwrap();
        native_write.put(DATA, &format_key(), FORMAT).unwrap();
        native_write.commit().unwrap();
        let published = owner.engine.begin_read().sequence();
        assert!(published > base_head);
        assert_eq!(root.borrow().commit(), Err(BAD_TXN));
        assert_eq!(owner.engine.begin_read().sequence(), published);
        drop(root);
        ubt_txn_abort(w);
        let r = f.begin(ptr::null_mut(), true);
        assert_eq!(get(r, 1), Ok(b"old".to_vec()));
        ubt_txn_abort(r);
        let next = f.begin(ptr::null_mut(), false);
        assert_eq!(put(next, 3, 1, b"fresh SQL base"), 0);
        assert_eq!(ubt_txn_commit(next), 0);
    }
}

#[test]
fn blocks_native_sparse_groups_keep_pinned_views_signed_ranges_and_full_hashes() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let original = model();
        let w = f.begin(ptr::null_mut(), false);
        for (id, value) in &original {
            assert_eq!(put(w, 3, *id, value), 0);
        }
        assert_eq!(ubt_txn_commit(w), 0);
        let old = f.begin(ptr::null_mut(), true);
        check_rows(old, &original);
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(old, 3, &mut cursor), 0);
        let mut key = val(&[]);
        let mut value = val(&[]);
        assert_eq!(ubt_cursor_get(cursor, &mut key, &mut value, 0), 0);
        let mut retained_key = Val {
            size: key.size,
            data: key.data,
        };
        let retained_value = Val {
            size: value.size,
            data: value.data,
        };
        assert_eq!(cursor_row(cursor, 8, None).unwrap().0, -1190);
        ubt_cursor_close(cursor);

        let mut updated = original.clone();
        let w = f.begin(ptr::null_mut(), false);
        let mut deleting = ptr::null_mut();
        assert_eq!(ubt_cursor_open(w, 3, &mut deleting), 0);
        for n in (-70_i64..=70).step_by(9) {
            let id = n * 17;
            assert_eq!(
                cursor_row(deleting, 15, Some(id)),
                Ok((id, original[&id].clone()))
            );
            assert_eq!(ubt_cursor_del(deleting, 0), 0);
            updated.remove(&id);
        }
        ubt_cursor_close(deleting);
        for (id, size) in [
            (i64::MIN, 65_570),
            (-1189, 1),
            (-17, 32_769),
            (3, 64),
            (1191, 8193),
            (i64::MAX - 1, 3),
            (i64::MAX, 0),
        ] {
            let value = payload(id, size);
            assert_eq!(put(w, 3, id, &value), 0);
            updated.insert(id, value);
        }
        check_rows(w, &updated);
        assert_eq!(ubt_txn_commit(w), 0);

        // Returned group-directory key/value slices stay in the old genuine
        // epoch across cursor destruction and a different committed packing.
        assert_eq!(bytes(&retained_key).unwrap(), encoded(i64::MIN));
        assert_eq!(bytes(&retained_value).unwrap(), original[&i64::MIN]);
        assert_eq!(ubt_get(old, 3, &mut retained_key, &mut value), 0);
        assert_eq!(bytes(&value).unwrap(), original[&i64::MIN]);
        check_rows(old, &original);
        ubt_txn_abort(old);

        let r = f.begin(ptr::null_mut(), true);
        check_rows(r, &updated);
        for (id, value) in &updated {
            assert_eq!(get(r, *id), Ok(value.clone()));
        }
        let mut a = ptr::null_mut();
        let mut b = ptr::null_mut();
        assert_eq!(ubt_cursor_open(r, 3, &mut a), 0);
        assert_eq!(ubt_cursor_open(r, 3, &mut b), 0);
        let first = updated.first_key_value().unwrap();
        let last = updated.last_key_value().unwrap();
        assert_eq!(cursor_row(a, 0, None), Ok((*first.0, first.1.clone())));
        assert_eq!(cursor_row(b, 6, None), Ok((*last.0, last.1.clone())));
        for target in [
            i64::MIN,
            i64::MIN + 1,
            -2000,
            -1190,
            -1189,
            -18,
            -17,
            0,
            1,
            2,
            3,
            1190,
            1191,
            2000,
            i64::MAX - 1,
            i64::MAX,
        ] {
            let expected = updated
                .range(target..)
                .next()
                .map(|(id, v)| (*id, v.clone()))
                .ok_or(NOTFOUND);
            assert_eq!(cursor_row(a, 17, Some(target)), expected);
            let exact = updated
                .get(&target)
                .map(|v| (target, v.clone()))
                .ok_or(NOTFOUND);
            assert_eq!(cursor_row(a, 15, Some(target)), exact);
        }
        assert_eq!(cursor_row(b, 4, None), Ok((*last.0, last.1.clone())));
        ubt_cursor_close(a);
        ubt_cursor_close(b);
        ubt_txn_abort(r);
    }
}

#[test]
fn blocks_native_drop_recreate_and_child_undo_keep_logical_catalog_and_groups_atomic() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let original = model();
        let w = f.begin(ptr::null_mut(), false);
        for (id, value) in &original {
            assert_eq!(put(w, 3, *id, value), 0);
        }
        assert_eq!(ubt_txn_commit(w), 0);
        let w = f.begin(ptr::null_mut(), false);
        let child = f.begin(w, false);
        assert_eq!(ubt_drop(child, 3, 1), 0);
        assert_eq!(get(child, 1), Err(BAD_DBI));
        let mut dbi = 0;
        assert_eq!(
            ubt_dbi_open(child, c"t00000003".as_ptr(), CREATE, &mut dbi),
            0
        );
        assert_eq!(dbi, 3);
        assert_eq!(put(child, 3, 77, b"discarded recreation"), 0);
        ubt_txn_abort(child);
        check_rows(w, &original);
        let child = f.begin(w, false);
        assert_eq!(ubt_drop(child, 3, 0), 0);
        assert_eq!(put(child, 3, 99, b"released but not committed"), 0);
        assert_eq!(ubt_txn_commit(child), 0);
        check_rows(
            w,
            &BTreeMap::from([(99, b"released but not committed".to_vec())]),
        );
        ubt_txn_abort(w);
        let r = f.begin(ptr::null_mut(), true);
        check_rows(r, &original);
        ubt_txn_abort(r);

        let w = f.begin(ptr::null_mut(), false);
        assert_eq!(ubt_drop(w, 3, 1), 0);
        assert_eq!(ubt_dbi_open(w, c"t00000003".as_ptr(), CREATE, &mut dbi), 0);
        assert_eq!(put(w, 3, -9, b"committed recreation"), 0);
        assert_eq!(ubt_txn_commit(w), 0);
        let r = f.begin(ptr::null_mut(), true);
        check_rows(r, &BTreeMap::from([(-9, b"committed recreation".to_vec())]));
        assert_eq!(get(r, 1), Err(NOTFOUND));
        ubt_txn_abort(r);
    }
}

#[test]
fn blocks_native_real_reader_and_writer_base_audit_each_dbi_only_once() {
    let f = Fixture::new();
    let audits = |epoch: &Epoch| epoch.read.with_dependent(|_, lookup| lookup.block_audits());
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        for id in 2..=90 {
            assert_eq!(put(w, 3, id, &payload(id, 64)), 0);
        }
        assert_eq!(ubt_txn_commit(w), 0);

        let r = f.begin(ptr::null_mut(), true);
        let epoch = match &(*r).state {
            State::Read(Some(epoch)) => epoch.clone(),
            _ => unreachable!(),
        };
        assert_eq!(audits(&epoch), 0);
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(r, 3, &mut cursor), 0);
        for _ in 0..100 {
            assert_eq!(get(r, 1), Ok(b"old".to_vec()));
            assert_eq!(get(r, 100), Err(NOTFOUND));
            assert_eq!(cursor_row(cursor, 17, Some(33)), Ok((33, payload(33, 64))));
            assert_eq!(cursor_row(cursor, 4, None), Ok((33, payload(33, 64))));
            assert_eq!(cursor_row(cursor, 8, None), Ok((34, payload(34, 64))));
            assert_eq!(audits(&epoch), 1);
        }
        ubt_cursor_close(cursor);
        drop(epoch);
        ubt_txn_abort(r);

        let w = f.begin(ptr::null_mut(), false);
        let base = match &(*w).state {
            State::Write(root) => root.borrow().base.clone(),
            _ => unreachable!(),
        };
        assert_eq!(audits(&base), 0);
        assert_eq!(put(w, 3, 34, b"staged replacement"), 0);
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(w, 3, &mut cursor), 0);
        for _ in 0..100 {
            assert_eq!(get(w, 1), Ok(b"old".to_vec()));
            assert_eq!(get(w, 100), Err(NOTFOUND));
            assert_eq!(cursor_row(cursor, 17, Some(33)), Ok((33, payload(33, 64))));
            assert_eq!(cursor_row(cursor, 4, None), Ok((33, payload(33, 64))));
            assert_eq!(
                cursor_row(cursor, 8, None),
                Ok((34, b"staged replacement".to_vec()))
            );
            assert_eq!(audits(&base), 1);
        }
        ubt_cursor_close(cursor);
        assert_eq!(ubt_txn_commit(w), 0);
        assert_eq!(audits(&base), 1, "commit repeated the same base audit");
        drop(base);
        let r = f.begin(ptr::null_mut(), true);
        assert_eq!(get(r, 34), Ok(b"staged replacement".to_vec()));
        ubt_txn_abort(r);
    }
}

#[test]
fn blocks_native_writer_range_exact_hits_skip_ordered_seeks_and_keep_stored_views() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let w = f.begin(ptr::null_mut(), false);
        for id in (-128..=128).step_by(2) {
            assert_eq!(put(w, 3, id, &payload(id, 64)), 0);
        }
        assert_eq!(ubt_txn_commit(w), 0);

        let w = f.begin(ptr::null_mut(), false);
        let root = match &(*w).state {
            State::Write(root) => root.clone(),
            _ => unreachable!(),
        };
        let base = root.borrow().base.clone();
        let rows = blocks::SnapshotRows::open_audited(
            base.read.borrow_owner(),
            DATA,
            3,
            blocks::Limits::default(),
        )
        .unwrap();
        assert!(
            rows.audit().blocks >= 4,
            "fixture must contain actual committed groups"
        );
        assert_eq!(rows.audit().rows, 130);
        drop(rows);
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(w, 3, &mut cursor), 0);
        let before = root.borrow().writer_seeks.get();
        for _ in 0..100 {
            for id in [-128, -112, -64, 0, 1, 64, 112, 128] {
                let value = if id == 1 {
                    b"old".to_vec()
                } else {
                    payload(id, 64)
                };
                assert_eq!(cursor_row(cursor, 17, Some(id)), Ok((id, value.clone())));
                assert_eq!(cursor_row(cursor, 4, None), Ok((id, value)));
            }
        }
        assert_eq!(
            root.borrow().writer_seeks.get(),
            before,
            "committed exact SET_RANGE hits must not invoke ordered writer_seek"
        );
        assert_eq!(
            base.read.with_dependent(|_, lookup| lookup.block_audits()),
            1
        );

        let mut input = encoded(64);
        let mut key = val(&input);
        let mut value = val(&[]);
        assert_eq!(ubt_cursor_get(cursor, &mut key, &mut value, 17), 0);
        assert_ne!(key.data, input.as_mut_ptr().cast());
        input.fill(0);
        // The real stored slices survive directory-cache replacement and
        // cursor destruction, without an intervening transaction mutation.
        for id in [-128, 0, 128] {
            assert!(cursor_row(cursor, 17, Some(id)).is_ok());
        }
        ubt_cursor_close(cursor);
        assert_eq!(bytes(&key).unwrap(), encoded(64));
        assert_eq!(bytes(&value).unwrap(), payload(64, 64));
        assert_eq!(root.borrow().writer_seeks.get(), before);
        drop(base);
        drop(root);
        ubt_txn_abort(w);
    }
}

#[test]
fn blocks_native_writer_range_misses_and_pending_rows_match_ordered_model() {
    let f = Fixture::new();
    unsafe {
        f.initialize();
        let mut expected = BTreeMap::from([(1, b"old".to_vec())]);
        let w = f.begin(ptr::null_mut(), false);
        for id in (-128..=128).step_by(2) {
            let value = payload(id, 64);
            assert_eq!(put(w, 3, id, &value), 0);
            expected.insert(id, value);
        }
        assert_eq!(ubt_txn_commit(w), 0);
        let old = f.begin(ptr::null_mut(), true);
        let original = expected.clone();
        let mut old_cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(old, 3, &mut old_cursor), 0);
        let input = encoded(-64);
        let mut retained_key = val(&input);
        let mut retained_value = val(&[]);
        assert_eq!(
            ubt_cursor_get(old_cursor, &mut retained_key, &mut retained_value, 17),
            0
        );
        ubt_cursor_close(old_cursor);

        let w = f.begin(ptr::null_mut(), false);
        let root = match &(*w).state {
            State::Write(root) => root.clone(),
            _ => unreachable!(),
        };
        let mut cursor = ptr::null_mut();
        assert_eq!(ubt_cursor_open(w, 3, &mut cursor), 0);
        let before = root.borrow().writer_seeks.get();
        assert_eq!(
            cursor_row(cursor, 17, Some(-127)),
            Ok((-126, expected[&-126].clone()))
        );
        assert_eq!(root.borrow().writer_seeks.get(), before + 1);
        assert_eq!(
            cursor_row(cursor, 17, Some(-64)),
            Ok((-64, expected[&-64].clone()))
        );
        assert_eq!(ubt_cursor_del(cursor, 0), 0);
        expected.remove(&-64);
        assert_eq!(cursor_row(cursor, 4, None), Err(NOTFOUND));
        let before = root.borrow().writer_seeks.get();
        assert_eq!(
            cursor_row(cursor, 17, Some(-64)),
            Ok((-62, expected[&-62].clone()))
        );
        assert_eq!(
            root.borrow().writer_seeks.get(),
            before + 1,
            "a tombstone must fall through to the ordered successor"
        );
        assert_eq!(
            cursor_row(cursor, 4, None),
            Ok((-62, expected[&-62].clone()))
        );
        assert_eq!(
            cursor_row(cursor, 8, None),
            Ok((-60, expected[&-60].clone()))
        );
        assert_eq!(
            cursor_row(cursor, 12, None),
            Ok((-62, expected[&-62].clone()))
        );

        for (id, size) in [(64, 65_570), (-64, 2048), (65, 3)] {
            let value = payload(id, size);
            assert_eq!(put(w, 3, id, &value), 0);
            expected.insert(id, value.clone());
            let before = root.borrow().writer_seeks.get();
            assert_eq!(cursor_row(cursor, 17, Some(id)), Ok((id, value.clone())));
            assert_eq!(cursor_row(cursor, 4, None), Ok((id, value)));
            assert_eq!(
                root.borrow().writer_seeks.get(),
                before,
                "overwrite, reinsert and new pending exact rows must skip ordered seeks"
            );
        }
        assert_eq!(cursor_row(cursor, 8, None), Ok((66, expected[&66].clone())));
        assert_eq!(
            cursor_row(cursor, 12, None),
            Ok((65, expected[&65].clone()))
        );
        for target in -131..=131 {
            let wanted = expected
                .range(target..)
                .next()
                .map(|(id, value)| (*id, value.clone()));
            assert_eq!(cursor_row(cursor, 17, Some(target)), wanted.ok_or(NOTFOUND));
        }
        assert_eq!(
            cursor_row(cursor, 6, None),
            Ok((128, expected[&128].clone()))
        );
        let before = root.borrow().writer_seeks.get();
        assert_eq!(cursor_row(cursor, 17, Some(129)), Err(NOTFOUND));
        assert_eq!(root.borrow().writer_seeks.get(), before);
        assert_eq!(
            cursor_row(cursor, 4, None),
            Ok((128, expected[&128].clone()))
        );
        ubt_cursor_close(cursor);

        let mut empty = 0;
        assert_eq!(
            ubt_dbi_open(w, c"t00000004".as_ptr(), CREATE, &mut empty),
            0
        );
        assert_eq!(ubt_cursor_open(w, empty, &mut cursor), 0);
        assert_eq!(cursor_row(cursor, 6, None), Err(NOTFOUND));
        let before = root.borrow().writer_seeks.get();
        assert_eq!(cursor_row(cursor, 17, Some(0)), Err(NOTFOUND));
        assert_eq!(root.borrow().writer_seeks.get(), before);
        ubt_cursor_close(cursor);

        // Metadata stays on its existing raw ordered path, even for a hit.
        let mut meta_key = val(&[7]);
        let mut meta_value = val(b"metadata");
        assert_eq!(ubt_put(w, 0, &mut meta_key, &mut meta_value, 0), 0);
        assert_eq!(ubt_cursor_open(w, 0, &mut cursor), 0);
        let before = root.borrow().writer_seeks.get();
        assert_eq!(
            ubt_cursor_get(cursor, &mut meta_key, &mut meta_value, 17),
            0
        );
        assert_eq!(root.borrow().writer_seeks.get(), before + 1);
        assert_eq!(bytes(&meta_key).unwrap(), &[7]);
        assert_eq!(bytes(&meta_value).unwrap(), b"metadata");
        ubt_cursor_close(cursor);
        check_rows(w, &expected);
        drop(root);
        assert_eq!(ubt_txn_commit(w), 0);
        assert_eq!(bytes(&retained_key).unwrap(), encoded(-64));
        assert_eq!(bytes(&retained_value).unwrap(), original[&-64]);
        check_rows(old, &original);
        ubt_txn_abort(old);
        let r = f.begin(ptr::null_mut(), true);
        check_rows(r, &expected);
        ubt_txn_abort(r);
    }
}
