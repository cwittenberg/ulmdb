//! Actual file-backed ordering across CoW publication, reopen and private branches.
use std::{
    collections::BTreeMap,
    ops::Bound,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use ulmdb::ordering::{
    DatabaseComparators, DatabaseOrdering as Spec, Endian, OrderingSpec as O, Width,
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "native-order-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn integer() -> O {
    O::UnsignedInteger {
        width: Width::U32,
        endian: Endian::Little,
    }
}
fn fold(counter: Arc<AtomicUsize>) -> ulmdb::ordering::CustomComparator {
    Arc::new(move |a, b| {
        counter.fetch_add(1, Ordering::Relaxed);
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    })
}
#[test]
fn native_builtin_integer_reverse_bulk_reopen_and_old_snapshot() {
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database_ordered(
            b"integer",
            DatabaseKind::Kv,
            Spec {
                keys: integer(),
                duplicates: O::Bytewise,
            },
        )
        .unwrap();
        w.load_sorted_kv(
            b"integer",
            (0..300u32).map(|n| Ok((n.to_le_bytes(), n.to_le_bytes()))),
        )
        .unwrap();
        w.create_database_ordered(
            b"reverse",
            DatabaseKind::Kv,
            Spec {
                keys: O::ReverseBytes,
                duplicates: O::Bytewise,
            },
        )
        .unwrap();
        for k in [b"az".as_slice(), b"b", b"ba", b"ab", b"aa", b"a"] {
            w.put(b"reverse", k, k).unwrap();
        }
        w.commit().unwrap();
        let old = e.begin_read();
        let borrow = old.get(b"integer", &256u32.to_le_bytes()).unwrap().unwrap();
        assert_eq!(borrow, 256u32.to_le_bytes());
        let expected = [b"a".as_slice(), b"aa", b"ba", b"b", b"ab", b"az"];
        assert_eq!(
            old.kv(b"reverse")
                .unwrap()
                .cursor(Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .map(|p| p.unwrap().0.to_vec())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            old.range(
                b"integer",
                Bound::Included(&255u32.to_le_bytes()),
                Bound::Excluded(&259u32.to_le_bytes()),
                false
            )
            .unwrap()
            .map(|p| u32::from_le_bytes(p.unwrap().0.try_into().unwrap()))
            .collect::<Vec<_>>(),
            [255, 256, 257, 258]
        );
        let mut w = e.begin_write().unwrap();
        assert!(w.put(b"integer", b"bad", b"x").is_err());
        w.put(b"integer", &256u32.to_le_bytes(), b"changed")
            .unwrap();
        w.delete(b"integer", &2u32.to_le_bytes()).unwrap();
        w.commit().unwrap();
        assert_eq!(borrow, 256u32.to_le_bytes());
        assert_eq!(
            e.begin_read()
                .get(b"integer", &256u32.to_le_bytes())
                .unwrap(),
            Some(b"changed".as_slice())
        );
        ulmdb::backup::verify(&e).unwrap();
        e.compact().unwrap();
        ulmdb::backup::verify(&e).unwrap();
        drop(old);
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        assert_eq!(
            e.begin_read().database_ordering(b"integer").unwrap().keys,
            integer()
        );
        assert_eq!(
            e.begin_read().get(b"integer", &2u32.to_le_bytes()).unwrap(),
            None
        );
    }
}
#[test]
fn native_custom_equality_reopen_structural_admission_copy_and_full_audit() {
    for mode in MODES {
        let d = Dir::new();
        let target = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = DatabaseComparators {
            keys: Some(fold(calls.clone())),
            duplicates: Some(fold(calls.clone())),
        };
        let mut w = e.begin_write().unwrap();
        w.create_database_ordered(
            b"custom",
            DatabaseKind::Kv,
            Spec {
                keys: O::Custom,
                duplicates: O::Bytewise,
            },
        )
        .unwrap();
        w.create_database_ordered(
            b"dups",
            DatabaseKind::Duplicates { width: None },
            Spec {
                keys: O::Custom,
                duplicates: O::Custom,
            },
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        w.put_with_ordering(b"custom", b"Alpha", b"A", &c).unwrap();
        w.put_with_ordering(b"custom", b"ALPHA", b"B", &c).unwrap();
        for n in (0..24).rev() {
            let v = format!("v{n:02}");
            assert!(
                w.insert_duplicate_with_ordering(b"dups", b"Key", v.as_bytes(), &c)
                    .unwrap()
            );
        }
        assert!(
            !w.insert_duplicate_with_ordering(b"dups", b"KEY", b"V02", &c)
                .unwrap()
        );
        w.commit().unwrap();
        let old = e.begin_read();
        let before = calls.load(Ordering::Relaxed);
        let view = old.kv_with_ordering(b"custom", &c).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), before);
        assert_eq!(view.get(b"alpha").unwrap(), Some(b"B".as_slice()));
        assert_eq!(
            view.cursor(Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .0,
            b"Alpha"
        );
        let du = old.duplicates_with_ordering(b"dups", &c).unwrap();
        assert_eq!(du.count(b"KEY").unwrap(), 24);
        assert_eq!(
            du.seek_pair(b"key", b"V02").unwrap(),
            Some(b"v02".as_slice())
        );
        let values = du
            .values(b"key", Bound::Included(b"V20"), Bound::Unbounded, false)
            .unwrap()
            .map(|v| v.unwrap().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(values, [b"v20", b"v21", b"v22", b"v23"]);
        assert!(matches!(
            old.get(b"custom", b"a"),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(old.structural_pairs(b"dups").unwrap().count(), 24);
        assert!(matches!(
            ulmdb::backup::verify(&e),
            Err(Error::InvalidInput(_))
        ));
        let mut map = BTreeMap::new();
        map.insert(b"custom".to_vec(), c.clone());
        map.insert(b"dups".to_vec(), c.clone());
        ulmdb::backup::verify_with_comparators(&e, &map).unwrap();
        let bad = DatabaseComparators {
            keys: Some(Arc::new(|a, b| b.cmp(a))),
            duplicates: c.duplicates.clone(),
        };
        map.insert(
            b"dups".to_vec(),
            DatabaseComparators {
                keys: c.keys.clone(),
                duplicates: bad.keys.clone(),
            },
        );
        assert!(ulmdb::backup::verify_with_comparators(&e, &map).is_err());
        ulmdb::backup::export(&e, &target.0).unwrap();
        e.compact().unwrap();
        let exported = Environment::open(&target.0).unwrap();
        assert_eq!(
            exported
                .begin_read()
                .duplicates_with_ordering(b"dups", &c)
                .unwrap()
                .count(b"key")
                .unwrap(),
            24
        );
        drop(exported);
        drop(du);
        drop(view);
        drop(old);
        drop(e);
        let coordinated = ulmdb::CoordinatedEnvironment::open(&d.0).unwrap();
        let read = coordinated.begin_read().unwrap();
        assert_eq!(
            read.kv_with_ordering(b"custom", &c)
                .unwrap()
                .get(b"ALPHA")
                .unwrap(),
            Some(b"B".as_slice())
        );
    }
}
#[test]
fn ordered_duplicates_promote_demote_fixed_integer_and_private_scope_branches() {
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database_ordered(
            b"dups",
            DatabaseKind::Duplicates { width: Some(4) },
            Spec {
                keys: O::ReverseBytes,
                duplicates: integer(),
            },
        )
        .unwrap();
        for n in (0..600u32).rev() {
            w.insert_duplicate(b"dups", b"ab", &n.to_le_bytes())
                .unwrap();
        }
        w.commit().unwrap();
        let old = e.begin_read();
        let olddu = old.duplicates(b"dups").unwrap();
        let retained = olddu
            .seek_pair(b"ab", &256u32.to_le_bytes())
            .unwrap()
            .unwrap();
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"dups").unwrap();
        let a = w.open_database_view(&h).unwrap();
        let b = w.open_database_view(&h).unwrap();
        w.view_insert_duplicate(&a, b"ab", &900u32.to_le_bytes())
            .unwrap();
        assert_eq!(w.view_duplicate_count(&a, b"ab").unwrap(), 601);
        assert_eq!(w.view_duplicate_count(&b, b"ab").unwrap(), 600);
        {
            let scope = w.database_read_scope().unwrap();
            let loan = scope.loan_database_view(&a).unwrap();
            let first = scope
                .seek_pair_borrowed(
                    &loan,
                    Some((b"ab", Some(&899u32.to_le_bytes()))),
                    true,
                    false,
                )
                .unwrap()
                .unwrap();
            assert_eq!(first.1, 900u32.to_le_bytes());
        }
        w.select_database_views(&[&a]).unwrap();
        for n in 0..595u32 {
            w.delete_duplicate(b"dups", b"ab", &n.to_le_bytes())
                .unwrap();
        }
        assert_eq!(w.duplicate_count(b"dups", b"ab").unwrap(), 6);
        w.commit().unwrap();
        assert_eq!(retained, 256u32.to_le_bytes());
        let now = e.begin_read();
        assert_eq!(
            now.duplicates(b"dups")
                .unwrap()
                .values(b"ab", Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .map(|v| u32::from_le_bytes(v.unwrap().try_into().unwrap()))
                .collect::<Vec<_>>(),
            [595, 596, 597, 598, 599, 900]
        );
        ulmdb::backup::verify(&e).unwrap();
        e.compact().unwrap();
        ulmdb::backup::verify(&e).unwrap();
    }
}
#[test]
fn custom_branch_savepoint_and_callback_authority_are_not_global() {
    let d = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let counter = Arc::new(AtomicUsize::new(0));
    let c = DatabaseComparators {
        keys: Some(fold(counter)),
        duplicates: None,
    };
    let mut w = e.begin_write().unwrap();
    w.create_database_ordered(
        b"x",
        DatabaseKind::Kv,
        Spec {
            keys: O::Custom,
            duplicates: O::Bytewise,
        },
    )
    .unwrap();
    w.put_with_ordering(b"x", b"Key", b"base", &c).unwrap();
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    let h = w.open_database(b"x").unwrap();
    let deferred = w.open_database_view(&h).unwrap();
    assert!(w.view_get(&deferred, b"Key").is_err());
    let a = w.open_database_view_with_ordering(&h, &c).unwrap();
    let b = w.open_database_view_with_ordering(&h, &c).unwrap();
    w.view_put(&a, b"KEY", b"A").unwrap();
    {
        let mut child = w.savepoint().unwrap();
        child.view_put(&a, b"key", b"CHILD").unwrap();
        assert_eq!(
            child.view_get(&a, b"Key").unwrap(),
            Some(b"CHILD".as_slice())
        );
        child.abort().unwrap();
    }
    assert_eq!(w.view_get(&a, b"key").unwrap(), Some(b"A".as_slice()));
    assert_eq!(w.view_get(&b, b"key").unwrap(), Some(b"base".as_slice()));
    w.select_database_views(&[&a]).unwrap();
    w.commit().unwrap();
    assert_eq!(
        e.begin_read()
            .kv_with_ordering(b"x", &c)
            .unwrap()
            .get(b"key")
            .unwrap(),
        Some(b"A".as_slice())
    );
}

#[test]
fn typed_builtin_views_validate_empty_roots_before_null_shortcut_and_keep_old_snapshots() {
    for mode in [PublicationMode::Legacy, PublicationMode::Indexed] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        for (name, keys) in [
            (b"byte".as_slice(), O::Bytewise),
            (b"reverse", O::ReverseBytes),
            (b"integer", integer()),
            (b"wide", O::UnsignedInteger { width: Width::U64, endian: Endian::Big }),
            (b"custom", O::Custom),
        ] {
            w.create_database_ordered(name, DatabaseKind::Kv, Spec { keys, duplicates: O::Bytewise }).unwrap();
        }
        w.commit().unwrap();
        let empty = e.begin_read();
        let oversize = vec![0; 4097];
        for (name, key) in [
            (b"byte".as_slice(), b"".as_slice()),
            (b"reverse", b""),
            (b"integer", 12u32.to_le_bytes().as_slice()),
            (b"wide", 34u64.to_be_bytes().as_slice()),
        ] {
            let view = empty.kv(name).unwrap();
            assert_eq!(view.get(key).unwrap(), None);
            assert!(matches!(view.get(&oversize), Err(Error::InvalidInput("key size limit exceeded"))));
        }
        for name in [b"integer".as_slice(), b"wide"] {
            assert!(matches!(empty.kv(name).unwrap().get(b""), Err(Error::InvalidInput("integer key width mismatch"))));
            assert!(matches!(empty.kv(name).unwrap().get(b"wrong"), Err(Error::InvalidInput("integer key width mismatch"))));
        }
        assert_eq!(empty.kv(b"reverse").unwrap().get(b"arbitrary").unwrap(), None);
        let calls = Arc::new(AtomicUsize::new(0));
        let callbacks = DatabaseComparators { keys: Some(fold(calls.clone())), duplicates: None };
        assert_eq!(empty.kv_with_ordering(b"custom", &callbacks).unwrap().get(b"key").unwrap(), None);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(matches!(empty.kv(b"custom"), Err(Error::InvalidInput(_))));

        let mut w = e.begin_write().unwrap();
        w.put(b"byte", b"", b"byte value").unwrap();
        w.put(b"reverse", b"ab", b"reverse value").unwrap();
        w.put(b"integer", &12u32.to_le_bytes(), b"integer value").unwrap();
        w.put(b"wide", &34u64.to_be_bytes(), b"wide value").unwrap();
        w.put_with_ordering(b"custom", b"Key", b"custom value", &callbacks).unwrap();
        w.commit().unwrap();
        let current = e.begin_read();
        assert_eq!(current.kv(b"byte").unwrap().get(b"").unwrap(), Some(b"byte value".as_slice()));
        assert_eq!(current.kv(b"reverse").unwrap().get(b"ab").unwrap(), Some(b"reverse value".as_slice()));
        let borrowed = {
            let query = 12u32.to_le_bytes();
            current.kv(b"integer").unwrap().get(&query).unwrap().unwrap()
        };
        assert_eq!(borrowed, b"integer value");
        assert_eq!(current.kv(b"wide").unwrap().get(&34u64.to_be_bytes()).unwrap(), Some(b"wide value".as_slice()));
        assert_eq!(current.kv_with_ordering(b"custom", &callbacks).unwrap().get(b"KEY").unwrap(), Some(b"custom value".as_slice()));
        assert_eq!(empty.kv(b"integer").unwrap().get(&12u32.to_le_bytes()).unwrap(), None);
        assert!(matches!(empty.kv(b"integer").unwrap().get(b"wrong"), Err(Error::InvalidInput("integer key width mismatch"))));
    }
}
