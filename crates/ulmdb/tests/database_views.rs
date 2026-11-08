//! Genuine native branch roots, no comparator emulation or whole database copy.
use ulmdb::{
    Address, CommitIntent, DatabaseKind, DatabaseViewOrigin as Origin, Environment, Error, Prefix,
    PublicationMode, WriteTxn,
};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "native-db-views-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn seed(d: &Dir, m: PublicationMode) -> Environment {
    let e = Environment::create_with_publication(&d.0, m).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"X", DatabaseKind::Kv).unwrap();
    w.create_database(b"META", DatabaseKind::Kv).unwrap();
    w.create_database(b"D", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.put(b"X", b"k", b"B").unwrap();
    w.put(b"META", b"width", b"base").unwrap();
    w.insert_duplicate(b"D", b"k", b"base").unwrap();
    w.commit().unwrap();
    e
}
fn assert_sync<T: Sync>() {}
#[test]
fn distinct_views_start_at_base_and_publish_only_explicit_winner() {
    for mode in MODES {
        for reverse in [false, true] {
            let d = Dir::new();
            let e = seed(&d, mode);
            let old = e.begin_read();
            let mut w = e.begin_write().unwrap();
            let h = w.open_database(b"X").unwrap();
            let bytes = w.dirty_bytes();
            let upper = w.open_database_view(&h).unwrap();
            let lower = w.open_database_view(&h).unwrap();
            assert_eq!(w.dirty_bytes(), bytes);
            assert_eq!(upper.handle(), lower.handle());
            assert_eq!(upper.origin(), Origin::TransactionBase);
            for (v, k, value) in if reverse {
                [
                    (&lower, b"l".as_slice(), b"LOWER".as_slice()),
                    (&upper, b"u", b"UPPER"),
                ]
            } else {
                [
                    (&upper, b"u".as_slice(), b"UPPER".as_slice()),
                    (&lower, b"l", b"LOWER"),
                ]
            } {
                w.view_put(v, b"k", value).unwrap();
                w.view_put(v, k, value).unwrap();
            }
            assert_eq!(w.view_get(&upper, b"k").unwrap(), Some(b"UPPER".as_slice()));
            assert_eq!(w.view_get(&lower, b"k").unwrap(), Some(b"LOWER".as_slice()));
            assert_eq!(w.view_get(&upper, b"l").unwrap(), None);
            assert_eq!(w.view_get(&lower, b"u").unwrap(), None);
            assert_eq!(
                w.view_seek_pair_borrowed(&lower, None, true, true)
                    .unwrap()
                    .unwrap()
                    .0,
                b"l"
            );
            assert_eq!(w.get(b"X", b"k").unwrap(), Some(b"B".as_slice()));
            w.select_database_views(&[&lower]).unwrap();
            assert!(matches!(w.view_get(&upper, b"k"), Err(Error::StaleHandle)));
            assert!(matches!(w.view_get(&lower, b"k"), Err(Error::StaleHandle)));
            w.commit().unwrap();
            assert_eq!(old.get(b"X", b"k").unwrap(), Some(b"B".as_slice()));
            drop(e);
            let e = Environment::open(&d.0).unwrap();
            let r = e.begin_read();
            assert_eq!(r.get(b"X", b"k").unwrap(), Some(b"LOWER".as_slice()));
            assert_eq!(r.get(b"X", b"u").unwrap(), None);
        }
    }
}
#[test]
fn late_open_and_newly_created_aliases_get_authentic_base_or_empty() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let a = w.open_database_view(&h).unwrap();
        w.view_put(&a, b"k", b"private").unwrap();
        let b = w.open_database_view(&h).unwrap();
        assert_eq!(w.view_get(&b, b"k").unwrap(), Some(b"B".as_slice()));
        w.create_database(b"new", DatabaseKind::Kv).unwrap();
        let n = w.open_database(b"new").unwrap();
        w.put_handle(&n, b"k", b"ordinary private").unwrap();
        let empty = w.open_database_view(&n).unwrap();
        assert_eq!(empty.origin(), Origin::CreatedEmpty);
        assert_eq!(w.view_get(&empty, b"k").unwrap(), None);
        w.select_database_views(&[&a, &empty]).unwrap();
        w.commit().unwrap();
        assert_eq!(e.begin_read().get(b"new", b"k").unwrap(), None);
    }
}
#[test]
fn scopes_loan_loaded_parent_but_new_aliases_use_immutable_seed_concurrently() {
    assert_sync::<WriteTxn<'static>>();
    assert_sync::<ulmdb::DatabaseReadScope<'static>>();
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let a = w.open_database_view(&h).unwrap();
        w.view_put(&a, b"k", b"private").unwrap();
        w.put(b"META", b"width", b"current").unwrap();
        let oldcap;
        {
            let first = w.database_read_scope().unwrap();
            let second = w.database_read_scope().unwrap();
            let loaded = first.loan_database_view(&a).unwrap();
            oldcap = first.open_database_view(&h).unwrap();
            let fresh = second.open_database_view(&h).unwrap();
            assert!(matches!(second.get(&oldcap, b"k"), Err(Error::StaleHandle)));
            std::thread::scope(|threads| {
                for _ in 0..4 {
                    threads.spawn(|| {
                        for _ in 0..128 {
                            assert_eq!(
                                first.get(&loaded, b"k").unwrap(),
                                Some(b"private".as_slice())
                            );
                            assert_eq!(second.get(&fresh, b"k").unwrap(), Some(b"B".as_slice()));
                            let ephemeral = first.open_database_view(&h).unwrap();
                            assert_eq!(first.get(&ephemeral, b"k").unwrap(), Some(b"B".as_slice()));
                        }
                    });
                }
            });
            assert_eq!(
                w.transaction_start_snapshot()
                    .unwrap()
                    .get(b"META", b"width")
                    .unwrap(),
                Some(b"base".as_slice())
            );
        }
        let next = w.database_read_scope().unwrap();
        assert!(matches!(next.get(&oldcap, b"k"), Err(Error::StaleHandle)));
        drop(next);
        w.view_put(&a, b"k", b"resumed").unwrap();
        w.select_database_views(&[&a]).unwrap();
        w.commit().unwrap();
    }
}
#[test]
fn savepoints_restore_roots_revoke_child_births_and_release_both_branches() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let a = w.open_database_view(&h).unwrap();
        let b = w.open_database_view(&h).unwrap();
        w.view_put(&a, b"k", b"parent").unwrap();
        let revoked;
        {
            let mut child = w.savepoint().unwrap();
            revoked = child.open_database_view(&h).unwrap();
            child.view_put(&a, b"k", b"childA").unwrap();
            child.view_put(&b, b"k", b"childB").unwrap();
            child.view_put(&revoked, b"k", b"child-new").unwrap();
            {
                let ro = child.database_read_scope().unwrap();
                let loan = ro.loan_database_view(&a).unwrap();
                assert_eq!(ro.get(&loan, b"k").unwrap(), Some(b"childA".as_slice()));
            }
            child.rollback().unwrap();
            assert!(matches!(
                child.view_get(&revoked, b"k"),
                Err(Error::StaleHandle)
            ));
            let replacement = child.open_database_view(&h).unwrap();
            child.view_put(&replacement, b"k", b"reused-tail").unwrap();
            assert!(matches!(
                child.view_get(&revoked, b"k"),
                Err(Error::StaleHandle)
            ));
            child.abort().unwrap();
        }
        assert_eq!(w.view_get(&a, b"k").unwrap(), Some(b"parent".as_slice()));
        assert_eq!(w.view_get(&b, b"k").unwrap(), Some(b"B".as_slice()));
        {
            let mut child = w.savepoint().unwrap();
            child.view_put(&a, b"k", b"releasedA").unwrap();
            child.view_put(&b, b"k", b"releasedB").unwrap();
            child.release().unwrap();
        }
        assert_eq!(w.view_get(&a, b"k").unwrap(), Some(b"releasedA".as_slice()));
        assert_eq!(w.view_get(&b, b"k").unwrap(), Some(b"releasedB".as_slice()));
        w.select_database_views(&[&b]).unwrap();
        w.commit().unwrap();
        assert_eq!(
            e.begin_read().get(b"X", b"k").unwrap(),
            Some(b"releasedB".as_slice())
        );
    }
}
#[test]
fn create_detach_abort_and_recreate_keep_explicit_empty_provenance() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let mut w = e.begin_write().unwrap();
        w.create_database(b"new", DatabaseKind::Kv).unwrap();
        let h = w.open_database(b"new").unwrap();
        w.put_handle(&h, b"k", b"private").unwrap();
        {
            let mut child = w.savepoint().unwrap();
            let detached = child.detach_database_handle(&h).unwrap();
            assert!(matches!(
                child.open_database_view(&h),
                Err(Error::MissingDatabase)
            ));
            assert_eq!(
                child.detached_get(&detached, b"k").unwrap(),
                Some(b"private".as_slice())
            );
            child.abort().unwrap();
        }
        let cap = w.open_database_view(&h).unwrap();
        assert_eq!(cap.origin(), Origin::CreatedEmpty);
        assert_eq!(w.view_get(&cap, b"k").unwrap(), None);
        w.drop_database(b"new").unwrap();
        w.create_database(b"new", DatabaseKind::Kv).unwrap();
        let newer = w.open_database(b"new").unwrap();
        assert_ne!(newer.id(), h.id());
        assert!(matches!(w.open_database_view(&h), Err(Error::StaleHandle)));
        let scope = w.database_read_scope().unwrap();
        let empty = scope.open_database_view(&newer).unwrap();
        assert_eq!(scope.get(&empty, b"k").unwrap(), None);
    }
}
#[test]
fn same_size_overflow_split_delete_and_duplicate_demotion_do_not_join_graphs() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let a = w.open_database_view(&h).unwrap();
        let b = w.open_database_view(&h).unwrap();
        for n in 0u32..180 {
            w.view_put(&a, &n.to_be_bytes(), &vec![1; 2048]).unwrap();
            w.view_put(&b, &n.to_be_bytes(), &vec![2; 2048]).unwrap();
        }
        for n in 0u32..180 {
            w.view_put(&a, &n.to_be_bytes(), &vec![3; 2048]).unwrap();
            if n % 2 == 0 {
                w.view_delete(&a, &n.to_be_bytes()).unwrap();
            }
            assert_eq!(
                w.view_get(&b, &n.to_be_bytes()).unwrap(),
                Some(vec![2; 2048].as_slice())
            );
        }
        let dh = w.open_database(b"D").unwrap();
        let da = w.open_database_view(&dh).unwrap();
        let db = w.open_database_view(&dh).unwrap();
        for n in 0u32..200 {
            w.view_insert_duplicate(&da, b"k", &n.to_be_bytes())
                .unwrap();
        }
        assert_eq!(w.view_duplicate_count(&db, b"k").unwrap(), 1);
        for n in 0u32..195 {
            w.view_delete_duplicate(&da, b"k", &n.to_be_bytes())
                .unwrap();
        }
        assert_eq!(w.view_duplicate_count(&da, b"k").unwrap(), 6);
        assert_eq!(w.view_get(&db, b"k").unwrap(), Some(b"base".as_slice()));
        w.select_database_views(&[&b, &da]).unwrap();
        w.commit().unwrap();
        assert_eq!(
            e.begin_read()
                .duplicates(b"D")
                .unwrap()
                .count(b"k")
                .unwrap(),
            6
        );
    }
}
#[test]
fn selection_preflights_all_groups_and_rejects_foreign_caps_without_partial_change() {
    let d = Dir::new();
    let e = seed(&d, PublicationMode::Legacy);
    let otherdir = Dir::new();
    let other = seed(&otherdir, PublicationMode::Legacy);
    let mut foreign = other.begin_write().unwrap();
    let fh = foreign.open_database(b"X").unwrap();
    let fv = foreign.open_database_view(&fh).unwrap();
    let mut w = e.begin_write().unwrap();
    let h = w.open_database(b"X").unwrap();
    let a = w.open_database_view(&h).unwrap();
    w.view_put(&a, b"k", b"A").unwrap();
    let mh = w.open_database(b"META").unwrap();
    let m = w.open_database_view(&mh).unwrap();
    w.view_put(&m, b"width", b"M").unwrap();
    assert!(w.select_database_views(&[&a]).is_err());
    assert!(w.select_database_views(&[&a, &a, &m]).is_err());
    assert!(w.select_database_views(&[&a, &fv]).is_err());
    assert_eq!(w.get(b"X", b"k").unwrap(), Some(b"B".as_slice()));
    assert_eq!(w.view_get(&a, b"k").unwrap(), Some(b"A".as_slice()));
    w.select_database_views(&[&a, &m]).unwrap();
    w.put(b"X", b"k", b"ordinary-after-selection").unwrap();
    assert!(matches!(w.view_get(&a, b"k"), Err(Error::StaleHandle)));
    w.commit().unwrap();
    let mut later = e.begin_write().unwrap();
    assert!(matches!(later.view_get(&a, b"k"), Err(Error::StaleHandle)));
    let laterh = later.open_database(b"X").unwrap();
    assert!(later.open_database_view(&laterh).is_ok());
}
#[test]
fn clean_close_cycles_prune_and_active_capacity_fails_before_catalog_change() {
    let d = Dir::new();
    let e = seed(&d, PublicationMode::Legacy);
    let mut w = e.begin_write().unwrap();
    let h = w.open_database(b"X").unwrap();
    let held = w.open_database_view(&h).unwrap();
    for _ in 0..(WriteTxn::MAX_DATABASE_VIEWS + 100) {
        let v = w.open_database_view(&h).unwrap();
        assert_eq!(w.view_get(&v, b"k").unwrap(), Some(b"B".as_slice()));
    }
    // Pressure may retire dead clean authorities, while an older live branch
    // remains genuine and readable across more than one registry capacity.
    assert_eq!(w.view_get(&held, b"k").unwrap(), Some(b"B".as_slice()));
    drop(held);
    let scope = w.database_read_scope().unwrap();
    for _ in 0..(WriteTxn::MAX_DATABASE_VIEWS + 100) {
        let v = scope.open_database_view(&h).unwrap();
        assert_eq!(scope.get(&v, b"k").unwrap(), Some(b"B".as_slice()));
    }
    let caps = (0..WriteTxn::MAX_DATABASE_VIEWS)
        .map(|_| scope.open_database_view(&h).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        scope.open_database_view(&h),
        Err(Error::LimitExceeded)
    ));
    drop(caps);
    assert!(scope.open_database_view(&h).is_ok());
    drop(scope);
    let caps = (0..WriteTxn::MAX_DATABASE_VIEWS)
        .map(|_| w.open_database_view(&h).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        w.open_database_view(&h),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        w.preflight_database_view(),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        w.open_database(b"must-not-create"),
        Err(Error::MissingDatabase)
    ));
    assert_eq!(w.get(b"X", b"k").unwrap(), Some(b"B".as_slice()));
    drop(caps);
    assert!(w.open_database_view(&h).is_ok());
    w.put(b"X", b"k", b"usable").unwrap();
    w.commit().unwrap();
}
#[test]
fn unresolved_dirty_views_cannot_silently_commit_or_prepare_and_prefix_is_unchanged() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        for prepare in [false, true] {
            let mut w = e.begin_write().unwrap();
            let h = w.open_database(b"X").unwrap();
            let v = w.open_database_view(&h).unwrap();
            w.view_put(&v, b"k", b"must-select").unwrap();
            assert!(if prepare {
                w.prepare().is_err()
            } else {
                w.commit().is_err()
            });
            assert_eq!(
                e.begin_read().get(b"X", b"k").unwrap(),
                Some(b"B".as_slice())
            );
        }
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        w.view_put(&v, b"k", b"selected").unwrap();
        w.create_database(b"prefix", DatabaseKind::Ipv4).unwrap();
        w.insert_prefix(
            b"prefix",
            Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
            b"route",
        )
        .unwrap();
        w.select_database_views(&[&v]).unwrap();
        let p = w.prepare().unwrap();
        assert_eq!(e.begin_read().sequence(), 1);
        p.commit().unwrap();
        assert_eq!(
            e.begin_read()
                .longest_match(b"prefix", Address::V4(0x0a000001))
                .unwrap()
                .unwrap()
                .1,
            b"route"
        );
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        w.view_put(&v, b"k", b"selected").unwrap();
        w.select_database_views(&[&v]).unwrap();
        assert_eq!(
            w.commit_with_intent(CommitIntent::ForcePublication)
                .unwrap(),
            3
        );
    }
}

#[test]
fn native_dirty_authority_budget_survives_dropped_clones_until_explicit_selection() {
    assert_eq!(WriteTxn::MAX_DATABASE_VIEWS, 8194);
    let d = Dir::new();
    let e = seed(&d, PublicationMode::Legacy);
    let mut w = e.begin_write().unwrap();
    let h = w.open_database(b"X").unwrap();
    let caps = (0..WriteTxn::MAX_DATABASE_VIEWS)
        .map(|_| w.open_database_view(&h).unwrap())
        .collect::<Vec<_>>();
    for (index, cap) in caps.iter().enumerate() {
        w.view_put(cap, b"k", &(index as u64).to_be_bytes())
            .unwrap();
    }
    let winner = caps.last().unwrap().clone();
    drop(caps);
    assert!(matches!(
        w.preflight_database_view(),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        w.open_database_view(&h),
        Err(Error::LimitExceeded)
    ));
    w.select_database_views(&[&winner]).unwrap();
    assert!(w.preflight_database_view().is_ok());
    w.commit().unwrap();
    assert_eq!(
        e.begin_read().get(b"X", b"k").unwrap(),
        Some(
            ((WriteTxn::MAX_DATABASE_VIEWS - 1) as u64)
                .to_be_bytes()
                .as_slice()
        )
    );
}
#[test]
fn prefix_handles_cannot_enter_comparator_alias_view_api() {
    let d = Dir::new();
    let e = seed(&d, PublicationMode::Legacy);
    let mut w = e.begin_write().unwrap();
    w.create_database(b"prefix", DatabaseKind::Ipv4).unwrap();
    let h = w.open_database(b"prefix").unwrap();
    assert!(matches!(w.open_database_view(&h), Err(Error::WrongKind)));
    {
        let scope = w.database_read_scope().unwrap();
        assert!(matches!(
            scope.open_database_view(&h),
            Err(Error::WrongKind)
        ));
    }
    w.insert_prefix_handle(
        &h,
        Prefix::new(Address::V4(0), 0).unwrap(),
        b"ordinary-prefix",
    )
    .unwrap();
    w.commit().unwrap();
}

#[test]
fn native_identity_removal_supersedes_dirty_views_but_abort_restores_authority() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d, mode);
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        w.view_put(&v, b"k", b"view-private").unwrap();
        {
            let mut child = w.savepoint().unwrap();
            let retained = child.detach_database_handle(&h).unwrap();
            assert_eq!(
                child.detached_get(&retained, b"k").unwrap(),
                Some(b"B".as_slice())
            );
            assert_eq!(
                child.view_get(&v, b"k").unwrap(),
                Some(b"view-private".as_slice())
            );
            assert!(matches!(
                child.view_put(&v, b"k", b"forbidden"),
                Err(Error::MissingDatabase)
            ));
            child.abort().unwrap();
        }
        w.view_put(&v, b"k", b"restored-write").unwrap();
        w.drop_database(b"X").unwrap();
        assert_eq!(
            w.view_get(&v, b"k").unwrap(),
            Some(b"restored-write".as_slice())
        );
        assert!(matches!(
            w.view_put(&v, b"k", b"forbidden"),
            Err(Error::MissingDatabase)
        ));
        w.prepare().unwrap().commit().unwrap();
        assert!(matches!(
            e.begin_read().open_database(b"X"),
            Err(Error::MissingDatabase)
        ));
    }
    let d = Dir::new();
    let e = seed(&d, PublicationMode::Legacy);
    let mut w = e.begin_write().unwrap();
    for _ in 0..(WriteTxn::MAX_DATABASE_VIEWS + 10) {
        w.create_database(b"temporary", DatabaseKind::Kv).unwrap();
        let h = w.open_database(b"temporary").unwrap();
        let v = w.open_database_view(&h).unwrap();
        w.view_put(&v, b"k", b"discarded by native deletion")
            .unwrap();
        w.drop_database(b"temporary").unwrap();
    }
    assert!(w.preflight_database_view().is_ok());
    w.prepare().unwrap().commit().unwrap();
    assert!(matches!(
        e.begin_read().open_database(b"temporary"),
        Err(Error::MissingDatabase)
    ));
}
