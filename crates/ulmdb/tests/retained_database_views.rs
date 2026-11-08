//! Genuine removed branches and root transfer; no rows copied or IDs rebound.
use ulmdb::{
    CommitIntent, DatabaseKind as K, DatabaseViewTransfer as T, Durability, Environment, Error,
    PublicationMode as M, WriteTxn,
};
const MODES: [M; 3] = [M::Legacy, M::Cohort, M::Indexed];
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "retained-view-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn seed(d: &Dir, m: M) -> Environment {
    let e = Environment::create_with_publication(&d.0, m).unwrap();
    let mut w = e.begin_write().unwrap();
    for name in [b"X".as_slice(), b"other"] {
        w.create_database(name, K::Kv).unwrap();
        w.put(name, b"k", b"B").unwrap();
    }
    w.create_database(b"D", K::Duplicates { width: None })
        .unwrap();
    w.insert_duplicate(b"D", b"k", b"base").unwrap();
    w.commit().unwrap();
    e
}
fn value(w: &WriteTxn<'_>, v: &ulmdb::DatabaseView, expected: &[u8]) {
    assert_eq!(w.view_get(v, b"k").unwrap(), Some(expected));
}
#[test]
fn retained_recreate_preserves_disjoint_roots_and_transfers_once() {
    for m in MODES {
        for winner_lower in [true, false] {
            let d = Dir::new();
            let e = seed(&d, m);
            let old = e.begin_read();
            let mut w = e.begin_write().unwrap();
            let a = w.open_database(b"X").unwrap();
            let upper = w.open_database_view(&a).unwrap();
            let lower = w.open_database_view(&a).unwrap();
            let bytes = w.dirty_bytes();
            let g = w.retain_database_views(&a, &[&upper, &lower]).unwrap();
            assert_eq!(bytes, w.dirty_bytes());
            let r = w.retained_database_view(&g, &lower).unwrap();
            assert_eq!(r.group().original_handle(), &a);
            assert!(w.put_handle(&a, b"k", b"bad").is_err());
            assert!(w.view_put(&lower, b"k", b"bad").is_err());
            assert!(w.open_database_view(&a).is_err());
            w.create_database(b"X", K::Kv).unwrap();
            let b = w.open_database(b"X").unwrap();
            assert_ne!(a.id(), b.id());
            let fresh = w.open_database_view(&b).unwrap();
            assert_eq!(w.view_get(&fresh, b"k").unwrap(), None);
            w.view_put(&fresh, b"k", b"NEW").unwrap();
            w.retained_view_put(&r, b"k", b"OLD").unwrap();
            w.retained_view_put(&r, b"only-old", &vec![7; 12000])
                .unwrap();
            value(&w, &upper, b"B");
            value(&w, &lower, b"OLD");
            value(&w, &fresh, b"NEW");
            {
                let scope = w.database_read_scope().unwrap();
                let loan = scope.loan_database_view(r.as_database_view()).unwrap();
                assert_eq!(scope.get(&loan, b"k").unwrap(), Some(b"OLD".as_slice()));
                assert!(scope.open_database_view(&a).is_err());
                let empty = scope.open_database_view(&b).unwrap();
                assert_eq!(scope.get(&empty, b"k").unwrap(), None);
            }
            let transfer = [T {
                source: &r,
                destination: &b,
            }];
            assert!(
                w.finalize_database_view_groups(&[], &transfer, &[], &[])
                    .is_err()
            );
            value(&w, &lower, b"OLD");
            value(&w, &fresh, b"NEW");
            if winner_lower {
                w.finalize_database_view_groups(&[], &transfer, &[], &[&b])
                    .unwrap();
            } else {
                w.finalize_database_view_groups(&[&fresh], &[], &[&g], &[])
                    .unwrap();
            }
            for v in [&upper, &lower, &fresh] {
                assert!(matches!(w.view_get(v, b"k"), Err(Error::StaleHandle)));
            }
            assert!(w.retained_view_put(&r, b"k", b"bad").is_err());
            assert!(w.put_handle(&a, b"k", b"bad").is_err());
            assert_eq!(
                w.get(b"X", b"k").unwrap(),
                Some(if winner_lower {
                    b"OLD".as_slice()
                } else {
                    b"NEW".as_slice()
                })
            );
            w.put_handle(&b, b"after", b"ordinary").unwrap();
            w.commit().unwrap();
            assert_eq!(old.get(b"X", b"k").unwrap(), Some(b"B".as_slice()));
            drop(e);
            let e = Environment::open(&d.0).unwrap();
            let read = e.begin_read();
            assert_eq!(
                read.get(b"X", b"k").unwrap(),
                Some(if winner_lower {
                    b"OLD".as_slice()
                } else {
                    b"NEW".as_slice()
                })
            );
            assert_eq!(read.get(b"X", b"only-old").unwrap().is_some(), winner_lower);
        }
    }
}
#[test]
fn batch_covers_every_generation_live_group_and_dirty_destination_atomically() {
    for m in MODES {
        let d = Dir::new();
        let e = seed(&d, m);
        let mut w = e.begin_write().unwrap();
        let a = w.open_database(b"X").unwrap();
        let av = w.open_database_view(&a).unwrap();
        w.view_put(&av, b"k", b"A").unwrap();
        assert!(w.retain_database_views(&a, &[]).is_err());
        let a2 = w.open_database_view(&a).unwrap();
        assert!(w.retain_database_views(&a, &[&a2]).is_err());
        value(&w, &av, b"A");
        let ag = w.retain_database_views(&a, &[&av]).unwrap();
        let ar = w.retained_database_view(&ag, &av).unwrap();
        w.create_database(b"X", K::Kv).unwrap();
        let b = w.open_database(b"X").unwrap();
        let bv = w.open_database_view(&b).unwrap();
        w.view_put(&bv, b"k", b"B2").unwrap();
        let bg = w.retain_database_views(&b, &[&bv]).unwrap();
        w.create_database(b"lower-name", K::Kv).unwrap();
        let c = w.open_database(b"lower-name").unwrap();
        let cv = w.open_database_view(&c).unwrap();
        w.view_put(&cv, b"k", b"C").unwrap();
        let other = w.open_database(b"other").unwrap();
        let ov = w.open_database_view(&other).unwrap();
        w.view_put(&ov, b"k", b"OTHER").unwrap();
        let t = [T {
            source: &ar,
            destination: &c,
        }];
        for (live, discard) in [(vec![], vec![&bg]), (vec![&ov], vec![])] {
            assert!(
                w.finalize_database_view_groups(&live, &t, &discard, &[&c])
                    .is_err()
            );
            value(&w, &av, b"A");
            value(&w, &bv, b"B2");
            value(&w, &cv, b"C");
            value(&w, &ov, b"OTHER");
        }
        assert!(
            w.finalize_database_view_groups(&[&ov], &t, &[&ag, &bg], &[&c])
                .is_err()
        );
        w.create_database(b"another", K::Kv).unwrap();
        let another = w.open_database(b"another").unwrap();
        assert!(
            w.finalize_database_view_groups(
                &[&ov],
                &[
                    T {
                        source: &ar,
                        destination: &c
                    },
                    T {
                        source: &ar,
                        destination: &another
                    }
                ],
                &[&bg],
                &[&c, &another]
            )
            .is_err()
        );
        // A committed destination is genuine but lacks this writer's creation proof.
        assert!(
            w.finalize_database_view_groups(
                &[],
                &[T {
                    source: &ar,
                    destination: &other
                }],
                &[&bg],
                &[&other]
            )
            .is_err()
        );
        w.finalize_database_view_groups(&[&ov], &t, &[&bg], &[&c])
            .unwrap();
        assert!(w.open_database(b"X").is_err());
        assert_eq!(w.get(b"lower-name", b"k").unwrap(), Some(b"A".as_slice()));
        w.commit().unwrap();
    }
}
#[test]
fn savepoint_abort_restores_removed_members_and_revokes_child_authority() {
    for m in MODES {
        let d = Dir::new();
        let e = seed(&d, m);
        let mut w = e.begin_write().unwrap();
        let a = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&a).unwrap();
        w.view_put(&v, b"k", b"P").unwrap();
        let child_group;
        let child_view;
        {
            let mut child = w.savepoint().unwrap();
            child_group = child.retain_database_views(&a, &[&v]).unwrap();
            child_view = child.retained_database_view(&child_group, &v).unwrap();
            child.retained_view_put(&child_view, b"k", b"C").unwrap();
            child.abort().unwrap();
        }
        value(&w, &v, b"P");
        assert!(w.retained_view_put(&child_view, b"k", b"bad").is_err());
        let g = w.retain_database_views(&a, &[&v]).unwrap();
        let retained = w.retained_database_view(&g, &v).unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.create_database(b"x", K::Kv).unwrap();
            let b = child.open_database(b"x").unwrap();
            child
                .finalize_database_view_groups(
                    &[],
                    &[T {
                        source: &retained,
                        destination: &b,
                    }],
                    &[],
                    &[&b],
                )
                .unwrap();
            child.put(b"x", b"k", b"same-size-edit").unwrap();
            child.abort().unwrap();
        }
        value(&w, &v, b"P");
        w.retained_view_put(&retained, b"k", b"AFTER").unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.create_database(b"x", K::Kv).unwrap();
            let b = child.open_database(b"x").unwrap();
            child
                .finalize_database_view_groups(
                    &[],
                    &[T {
                        source: &retained,
                        destination: &b,
                    }],
                    &[],
                    &[&b],
                )
                .unwrap();
            child.release().unwrap();
        }
        assert!(w.retained_view_put(&retained, b"k", b"bad").is_err());
        w.commit().unwrap();
        assert_eq!(
            e.begin_read().get(b"x", b"k").unwrap(),
            Some(b"AFTER".as_slice())
        );
    }
}
#[test]
fn retained_duplicate_mutations_and_policy_prepare_keep_real_tree_shape() {
    for m in MODES {
        let d = Dir::new();
        let e = seed(&d, m);
        e.set_durability(Durability::NoSync).unwrap();
        let mut w = e.begin_write().unwrap();
        let a = w.open_database(b"D").unwrap();
        let v = w.open_database_view(&a).unwrap();
        let g = w.retain_database_views(&a, &[&v]).unwrap();
        let r = w.retained_database_view(&g, &v).unwrap();
        for i in 0u32..180 {
            assert!(
                w.retained_view_insert_duplicate(&r, b"k", &i.to_be_bytes())
                    .unwrap()
            );
        }
        assert_eq!(w.view_duplicate_count(&v, b"k").unwrap(), 181);
        assert!(w.retained_view_delete_duplicate(&r, b"k", b"base").unwrap());
        assert_eq!(
            w.view_seek_pair_borrowed(&v, None, true, true)
                .unwrap()
                .unwrap()
                .1,
            179u32.to_be_bytes()
        );
        w.create_database(b"bad-kind", K::Kv).unwrap();
        let bad = w.open_database(b"bad-kind").unwrap();
        assert!(matches!(
            w.finalize_database_view_groups(
                &[],
                &[T {
                    source: &r,
                    destination: &bad
                }],
                &[],
                &[&bad]
            ),
            Err(Error::WrongKind)
        ));
        w.create_database(b"dups", K::Duplicates { width: None })
            .unwrap();
        let b = w.open_database(b"dups").unwrap();
        w.finalize_database_view_groups(
            &[],
            &[T {
                source: &r,
                destination: &b,
            }],
            &[],
            &[&b],
        )
        .unwrap();
        let prepared = w
            .prepare_with_intent(CommitIntent::ForcePublication)
            .unwrap();
        e.set_durability(Durability::Full).unwrap();
        prepared.commit().unwrap();
        e.sync().unwrap();
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        assert_eq!(e.begin_read().duplicate_count(b"dups", b"k").unwrap(), 180);
    }
}
#[test]
fn dirty_retained_ownership_blocks_every_prepare_even_if_callers_drop_caps() {
    for m in MODES {
        for ordinary in [false, true] {
            let d = Dir::new();
            let e = seed(&d, m);
            let mut w = e.begin_write().unwrap();
            let a = w.open_database(b"X").unwrap();
            let v = w.open_database_view(&a).unwrap();
            let g = w.retain_database_views(&a, &[&v]).unwrap();
            let r = w.retained_database_view(&g, &v).unwrap();
            w.retained_view_put(&r, b"k", b"DIRTY").unwrap();
            drop(r);
            drop(g);
            drop(v);
            assert!(w.select_database_views(&[]).is_err());
            let result = if ordinary {
                w.prepare_for_commit_with_intent(CommitIntent::ForcePublication)
            } else {
                w.prepare_with_intent(CommitIntent::ForcePublication)
            };
            assert!(result.is_err());
            assert_eq!(
                e.begin_read().get(b"X", b"k").unwrap(),
                Some(b"B".as_slice())
            );
        }
    }
}
#[test]
fn generic_removal_cannot_be_upgraded_and_foreign_writer_cannot_use_authority() {
    let d = Dir::new();
    let e = seed(&d, M::Legacy);
    let mut w = e.begin_write().unwrap();
    let a = w.open_database(b"X").unwrap();
    let v = w.open_database_view(&a).unwrap();
    w.drop_database(b"X").unwrap();
    assert!(w.retain_database_views(&a, &[&v]).is_err());
    w.abort();
    let mut w = e.begin_write().unwrap();
    let a = w.open_database(b"X").unwrap();
    let v = w.open_database_view(&a).unwrap();
    let g = w.retain_database_views(&a, &[&v]).unwrap();
    let r = w.retained_database_view(&g, &v).unwrap();
    w.abort();
    let mut w = e.begin_write().unwrap();
    assert!(w.retained_view_put(&r, b"k", b"bad").is_err());
    assert!(w.retained_database_view(&g, &v).is_err());
    assert_eq!(w.get(b"X", b"k").unwrap(), Some(b"B".as_slice()));
    w.abort();
    let d2 = Dir::new();
    let e2 = seed(&d2, M::Legacy);
    let mut w = e2.begin_write().unwrap();
    assert!(w.retained_view_put(&r, b"k", b"bad").is_err());
    w.abort();
}
#[test]
fn retained_delete_clear_and_fixed_duplicates_keep_sibling_bytes_immutable() {
    for m in MODES {
        let d = Dir::new();
        let e = seed(&d, m);
        let mut w = e.begin_write().unwrap();
        let a = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&a).unwrap();
        let sibling = w.open_database_view(&a).unwrap();
        let g = w.retain_database_views(&a, &[&v, &sibling]).unwrap();
        let r = w.retained_database_view(&g, &v).unwrap();
        w.retained_view_put(&r, b"large", &vec![9; 24000]).unwrap();
        w.retained_view_delete(&r, b"k").unwrap();
        assert_eq!(w.view_get(&v, b"k").unwrap(), None);
        value(&w, &sibling, b"B");
        w.retained_view_clear(&r).unwrap();
        assert_eq!(w.view_get(&v, b"large").unwrap(), None);
        value(&w, &sibling, b"B");
        w.create_database(b"fixed", K::Duplicates { width: Some(4) })
            .unwrap();
        let f = w.open_database(b"fixed").unwrap();
        let fv = w.open_database_view(&f).unwrap();
        let fg = w.retain_database_views(&f, &[&fv]).unwrap();
        let fr = w.retained_database_view(&fg, &fv).unwrap();
        for i in 0u32..180 {
            w.retained_view_insert_duplicate(&fr, b"k", &i.to_be_bytes())
                .unwrap();
        }
        assert_eq!(w.retained_view_delete_duplicates(&fr, b"k").unwrap(), 180);
        assert_eq!(w.view_duplicate_count(&fv, b"k").unwrap(), 0);
        w.retained_view_insert_duplicate(&fr, b"k", b"abcd")
            .unwrap();
        w.retained_view_clear(&fr).unwrap();
        w.finalize_database_view_groups(&[], &[], &[&g, &fg], &[])
            .unwrap();
        w.commit().unwrap();
        assert!(e.begin_read().open_database(b"X").is_err());
    }
}
#[test]
fn clean_close_reopen_beyond_limit_and_full_dirty_admission_remain_bounded() {
    let d = Dir::new();
    let e = seed(&d, M::Legacy);
    let mut w = e.begin_write().unwrap();
    for _ in 0..(WriteTxn::MAX_DATABASE_VIEWS + 17) {
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        let g = w.retain_database_views(&h, &[&v]).unwrap();
        let r = w.retained_database_view(&g, &v).unwrap();
        if r.as_database_view().origin() == ulmdb::DatabaseViewOrigin::TransactionBase {
            value(&w, r.as_database_view(), b"B");
        } else {
            assert_eq!(w.view_get(r.as_database_view(), b"k").unwrap(), None);
        }
        drop(r);
        drop(g);
        drop(v);
        w.create_database(b"X", K::Kv).unwrap();
    }
    w.abort();
    let mut w = e.begin_write().unwrap();
    let a = w.open_database(b"X").unwrap();
    let mut views = Vec::new();
    for _ in 0..WriteTxn::MAX_DATABASE_VIEWS {
        let v = w.open_database_view(&a).unwrap();
        w.view_put(&v, b"k", b"dirty").unwrap();
        views.push(v);
    }
    assert!(matches!(
        w.preflight_database_view(),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        w.open_database_view(&a),
        Err(Error::LimitExceeded)
    ));
    let refs: Vec<_> = views.iter().collect();
    let g = w.retain_database_views(&a, &refs).unwrap();
    let r = w.retained_database_view(&g, &views[0]).unwrap();
    w.retained_view_put(&r, b"k", b"winner").unwrap();
    drop(views);
    assert!(matches!(
        w.preflight_database_view(),
        Err(Error::LimitExceeded)
    ));
    w.create_database(b"x", K::Kv).unwrap();
    let b = w.open_database(b"x").unwrap();
    w.finalize_database_view_groups(
        &[],
        &[T {
            source: &r,
            destination: &b,
        }],
        &[],
        &[&b],
    )
    .unwrap();
    w.preflight_database_view().unwrap();
    let new = w.open_database_view(&b).unwrap();
    assert_eq!(w.view_get(&new, b"k").unwrap(), None);
    drop(new);
    w.commit().unwrap();
    assert_eq!(
        e.begin_read().get(b"x", b"k").unwrap(),
        Some(b"winner".as_slice())
    );
}
#[test]
fn sibling_private_read_scopes_loan_retained_views_and_new_aliases_are_empty() {
    fn sync<T: Sync>() {}
    sync::<WriteTxn<'static>>();
    for m in MODES {
        let d = Dir::new();
        let e = seed(&d, m);
        let mut w = e.begin_write().unwrap();
        let a = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&a).unwrap();
        let g = w.retain_database_views(&a, &[&v]).unwrap();
        let r = w.retained_database_view(&g, &v).unwrap();
        w.retained_view_put(&r, b"k", b"PRIVATE").unwrap();
        w.create_database(b"X", K::Kv).unwrap();
        let b = w.open_database(b"X").unwrap();
        std::thread::scope(|threads| {
            for _ in 0..4 {
                let (r, b, w) = (&r, &b, &w);
                threads.spawn(move || {
                    for _ in 0..100 {
                        let scope = w.database_read_scope().unwrap();
                        let old = scope.loan_database_view(r.as_database_view()).unwrap();
                        let fresh = scope.open_database_view(b).unwrap();
                        assert_eq!(scope.get(&old, b"k").unwrap(), Some(b"PRIVATE".as_slice()));
                        assert_eq!(scope.get(&fresh, b"k").unwrap(), None);
                        assert!(
                            scope
                                .open_database_view(r.group().original_handle())
                                .is_err()
                        );
                    }
                });
            }
        });
        w.retained_view_put(&r, b"k", b"AFTER").unwrap();
        w.finalize_database_view_groups(
            &[],
            &[T {
                source: &r,
                destination: &b,
            }],
            &[],
            &[&b],
        )
        .unwrap();
        w.commit().unwrap();
    }
}
#[test]
fn retained_transfer_policy_matrix_preserves_data_debt_and_local_savepoint_rules() {
    let policies = [Durability::Full, Durability::NoMetaSync, Durability::NoSync];
    for m in MODES {
        for data in policies {
            for metadata in policies {
                for local in policies {
                    let d = Dir::new();
                    let e = seed(&d, m);
                    let before = std::fs::read(d.0.join("CURRENT")).unwrap();
                    e.set_durability(data).unwrap();
                    let mut w = e.begin_write().unwrap();
                    {
                        let mut child = w.savepoint().unwrap();
                        child.weaken_durability(Durability::NoSync).unwrap();
                        child.abort().unwrap();
                    }
                    assert_eq!(w.durability(), data);
                    {
                        let mut child = w.savepoint().unwrap();
                        child.weaken_durability(local).unwrap();
                        child.release().unwrap();
                    }
                    let a = w.open_database(b"X").unwrap();
                    let v = w.open_database_view(&a).unwrap();
                    let g = w.retain_database_views(&a, &[&v]).unwrap();
                    let r = w.retained_database_view(&g, &v).unwrap();
                    w.retained_view_put(&r, b"k", b"TRANSFER").unwrap();
                    w.create_database(b"x", K::Kv).unwrap();
                    let b = w.open_database(b"x").unwrap();
                    let bytes = w.dirty_bytes();
                    w.finalize_database_view_groups(
                        &[],
                        &[T {
                            source: &r,
                            destination: &b,
                        }],
                        &[],
                        &[&b],
                    )
                    .unwrap();
                    assert_eq!(bytes, w.dirty_bytes());
                    let ready = w.prepare().unwrap();
                    assert_eq!(ready.data_policy(), data.weaken(local));
                    assert_eq!(std::fs::read(d.0.join("CURRENT")).unwrap(), before);
                    e.set_durability(metadata).unwrap();
                    ready.commit().unwrap();
                    let (_, capture, pin, state) =
                        ulmdb_storage::coordination::Coordination::capture_directory(
                            &d.0,
                            Default::default(),
                        )
                        .unwrap()
                        .capture_current_with_durability()
                        .unwrap();
                    drop(pin);
                    drop(capture);
                    let debt = data.weaken(local) == Durability::NoSync;
                    assert_eq!(state.has_data_debt(), debt);
                    assert_eq!(
                        state.is_weak(),
                        debt || metadata.weaken(local) != Durability::Full
                    );
                    assert_eq!(
                        e.begin_read().get(b"x", b"k").unwrap(),
                        Some(b"TRANSFER".as_slice())
                    );
                }
            }
        }
    }
}
