use super::*;
use crate::{
    DatabaseKind,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::{Reference, resident::Config};

fn db(id: u64, offset: u64) -> Database {
    Database {
        ordering: crate::ordering::DatabaseOrdering::default(),
        id,
        kind: DatabaseKind::Kv,
        root: Reference { segment: 1, offset },
    }
}
fn name(n: usize) -> Vec<u8> {
    format!("db{n:04}").into_bytes()
}
fn compare(catalog: &Catalog, model: &BTreeMap<Vec<u8>, Database>) {
    assert_eq!(catalog.len(), model.len());
    let actual: Vec<_> = catalog
        .iter()
        .map(|(n, d)| (n.clone(), super::super::catalog_codec::encode(d)))
        .collect();
    let expected: Vec<_> = model
        .iter()
        .map(|(n, d)| (n.clone(), super::super::catalog_codec::encode(d)))
        .collect();
    assert_eq!(actual, expected);
    for (name, db) in model {
        assert_eq!(catalog.get(name).unwrap().id, db.id);
    }
}

#[test]
fn overlay_random_model_abort_identity_replacement_and_old_snapshots() {
    let mut model: BTreeMap<Vec<u8>, Database> = BTreeMap::new();
    let mut catalog = Catalog::default();
    let mut saved = Vec::new();
    let mut random = 73u64;
    let mut next_id = 1;
    for round in 0..1600 {
        let mut edit = Edit::new(&catalog);
        let mut expected = model.clone();
        for _ in 0..1 + round % 7 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let name = name((random >> 32) as usize % 192);
            if random & 3 == 0 {
                assert_eq!(
                    edit.remove(&catalog, &name).map(|d| d.id),
                    expected.remove(&name).map(|d| d.id)
                );
            } else if let Some(old) = expected.get_mut(&name) {
                old.root.offset = round as u64 * 4096;
                edit.set_root(&catalog, &name, old.root);
            } else {
                let value = db(next_id, round as u64 * 4096);
                next_id += 1;
                edit.insert(&catalog, name.clone(), value.clone());
                expected.insert(name, value);
            }
        }
        assert_eq!(edit.len(), expected.len());
        for n in 0..192 {
            assert_eq!(
                edit.get(&catalog, &name(n)).map(|d| d.id),
                expected.get(&name(n)).map(|d| d.id)
            );
        }
        if round % 11 == 0 {
            drop(edit); // Abort leaves all source metadata and old readers intact.
        } else {
            catalog = edit.prepare(&catalog).unwrap();
            model = expected;
        }
        compare(&catalog, &model);
        if round % 113 == 0 {
            saved.push((catalog.clone(), model.clone()));
        }
        for (old, oracle) in &saved {
            compare(old, oracle);
        }
    }
    // A burned ID may replace a removed database under the same name, while
    // the old immutable slot and descriptor remain visible through old readers.
    let key = model.keys().next().unwrap().clone();
    let old = catalog.clone();
    let old_id = old.get(&key).unwrap().id;
    let mut edit = Edit::new(&catalog);
    edit.remove(&catalog, &key).unwrap();
    edit.insert(&catalog, key.clone(), db(next_id, 8192));
    let replacement = edit.prepare(&catalog).unwrap();
    assert_eq!(replacement.get(&key).unwrap().id, next_id);
    assert_eq!(old.get(&key).unwrap().id, old_id);
}

#[test]
fn actual_detach_work_is_fixed_top_plus_touched_chunks_not_name_count() {
    for n in [1, 64, 65, 512, 4096] {
        let map = (0..n).map(|i| (name(i), db(i as u64 + 1, 4096))).collect();
        let old = Catalog::from_map(map).unwrap();
        let owners = match &old.repr {
            Representation::Large(c) => {
                Some((Arc::strong_count(&c.names), Arc::strong_count(&c.directory)))
            }
            _ => None,
        };
        let mut edit = Edit::new(&old);
        if let Representation::Large(c) = &old.repr {
            assert_eq!(
                owners,
                Some((Arc::strong_count(&c.names), Arc::strong_count(&c.directory)))
            );
        }
        assert_eq!(edit.changes.len(), 0);
        edit.set_root(
            &old,
            &name(n - 1),
            Reference {
                segment: 1,
                offset: 8192,
            },
        );
        // Repeated puts update the one overlay entry, without new name copies.
        edit.set_root(
            &old,
            &name(n - 1),
            Reference {
                segment: 1,
                offset: 12288,
            },
        );
        assert_eq!(edit.changes.len(), 1);
        let next = edit.prepare(&old).unwrap();
        if n <= BLOCK {
            assert_eq!(
                next.work(),
                Work {
                    small_entries_copied_bound: n,
                    small_map_clones: 1,
                    ..Work::default()
                }
            );
        } else {
            let (Representation::Large(a), Representation::Large(b)) = (&old.repr, &next.repr)
            else {
                unreachable!()
            };
            assert!(Arc::ptr_eq(&a.names, &b.names));
            assert_eq!(
                next.work(),
                Work {
                    directory_detaches: 1,
                    chunk_detaches: 1,
                    names_detaches: 0,
                    slot_writes: 1,
                    ..Work::default()
                }
            );
        }
        assert_eq!(old.get(&name(n - 1)).unwrap().root.offset, 4096);
        assert_eq!(next.get(&name(n - 1)).unwrap().root.offset, 12288);
        eprintln!(
            "catalog names={n} top_pointer_copy_bound={} chunk_entry_copy_bound={} slot_writes={} small_map_entries_clone_bound={} directory_payload_bytes={} chunk_payload_bytes={}",
            if n > BLOCK { BLOCK } else { 0 },
            if n > BLOCK { BLOCK } else { 0 },
            next.work().slot_writes,
            next.work().small_entries_copied_bound,
            std::mem::size_of::<Directory>(),
            std::mem::size_of::<Chunk>()
        );
    }
    let old = Catalog::from_map(
        (0..192)
            .map(|i| (name(i), db(i as u64 + 1, 4096)))
            .collect(),
    )
    .unwrap();
    let mut edit = Edit::new(&old);
    for i in 0..128 {
        edit.set_root(&old, &name(i), Reference::NULL);
    }
    let next = edit.prepare(&old).unwrap();
    assert_eq!(
        next.work(),
        Work {
            directory_detaches: 1,
            chunk_detaches: 2,
            names_detaches: 0,
            slot_writes: 128,
            ..Work::default()
        }
    );
    let (Representation::Large(a), Representation::Large(b)) = (&old.repr, &next.repr) else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(
        a.directory.chunks[2].as_ref().unwrap(),
        b.directory.chunks[2].as_ref().unwrap()
    ));
}

#[test]
fn full_catalog_drop_create_order_empty_boundary_and_cold_identity_checks() {
    // These constructors use the default profile, not the extended slot bound.
    let capacity = crate::CatalogCapacity::DEFAULT.get();
    let old = Catalog::from_map(
        (0..capacity)
            .map(|i| (name(i), db(i as u64 + 1, 4096)))
            .collect(),
    )
    .unwrap();
    let mut edit = Edit::new(&old);
    assert!(edit.remove(&old, &name(capacity - 1)).is_some());
    edit.insert(&old, b"aaa".to_vec(), db(5000, 8192)); // insertion sorts before deletion
    let next = edit.prepare(&old).unwrap();
    assert_eq!(next.len(), capacity);
    assert_eq!(next.get(b"aaa").unwrap().id, 5000);
    assert_eq!(old.get(&name(capacity - 1)).unwrap().id, capacity as u64);
    assert!(next.get(&name(capacity - 1)).is_none());
    assert_eq!(next.work().names_detaches, 1);
    assert!(Catalog::from_map(BTreeMap::from([(b"x".to_vec(), db(0, 0))])).is_err());
    assert!(
        Catalog::from_map(BTreeMap::from([
            (b"a".to_vec(), db(1, 0)),
            (b"b".to_vec(), db(1, 0))
        ]))
        .is_err()
    );
    let empty = Catalog::default();
    let mut edit = Edit::new(&empty);
    assert!(edit.remove(&empty, b"absent").is_none());
    edit.insert(&empty, b"x".to_vec(), db(1, 0));
    edit.remove(&empty, b"x").unwrap();
    assert_eq!(edit.prepare(&empty).unwrap().len(), 0);
}

#[test]
fn small_direct_binding_promotion_demotion_and_failed_prepare_preserve_sources() {
    let small =
        Catalog::from_map((0..64).map(|i| (name(i), db(i as u64 + 1, 4096))).collect()).unwrap();
    assert!(matches!(small.repr, Representation::Small { .. }));
    let mut edit = Edit::new(&small);
    edit.insert(&small, name(64), db(65, 4096));
    let large = edit.prepare(&small).unwrap();
    assert!(matches!(large.repr, Representation::Large(_)));
    let mut edit = Edit::new(&large);
    edit.remove(&large, &name(63)).unwrap();
    let demoted = edit.prepare(&large).unwrap();
    assert!(matches!(demoted.repr, Representation::Small { .. }));
    assert!(demoted.get(&name(63)).is_none());
    assert_eq!(small.get(&name(63)).unwrap().id, 64);
    assert_eq!(large.get(&name(63)).unwrap().id, 64);
    for old in [small, large] {
        let mut edit = Edit::new(&old);
        let mut wrong = old.get(&name(0)).unwrap().clone();
        wrong.kind = DatabaseKind::Ipv6;
        edit.insert(&old, name(0), wrong);
        assert!(edit.prepare(&old).is_err());
        assert_eq!(old.get(&name(0)).unwrap().kind, DatabaseKind::Kv);
    }
    let capacity = crate::CatalogCapacity::DEFAULT.get();
    let full = Catalog::from_map(
        (0..capacity)
            .map(|i| (name(i), db(i as u64 + 1, 4096)))
            .collect(),
    )
    .unwrap();
    let mut bad = Edit::new(&full);
    bad.insert(&full, b"excess".to_vec(), db(9999, 4096));
    assert!(matches!(bad.prepare(&full), Err(Error::LimitExceeded)));
    assert_eq!(full.len(), capacity);
    assert!(full.get(b"excess").is_none());
}

struct Path(std::path::PathBuf);
impl Drop for Path {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn real_catalog_transactions_abort_drop_recreate_collect_and_reopen() {
    for directory in [std::env::temp_dir(), std::path::PathBuf::from("/dev/shm")] {
        if !directory.is_dir() {
            continue;
        }
        for (format, encoding) in [
            (RedoFormat::LiteralV1, RedoEncoding::Owned),
            (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
            (RedoFormat::CompactV2, RedoEncoding::Borrowed),
        ] {
            let path = Path(directory.join(format!(
                "ulmdb-catalog-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            )));
            let options = Options {
                pool: Config {
                    kv_pages: 192,
                    tiny_pages: 16,
                    max_reserved_bytes: 64 << 20,
                    max_metadata_bytes: 64 << 20,
                    max_views: 128,
                },
                wal_capacity: 8 << 20,
                graph_read_budget: 2_000_000,
                auto_collect_below: 16,
            };
            let env =
                Environment::create_with_encoding(&path.0, options, format, encoding).unwrap();
            let mut write = env.begin_write().unwrap();
            for n in 0..160 {
                write.create_database(&name(n), DatabaseKind::Kv).unwrap();
            }
            write.put(&name(0), b"k", b"old").unwrap();
            write.commit().unwrap();
            let old = env.begin_read();
            let escaped = old.get(&name(0), b"k").unwrap().unwrap();
            {
                let mut abort = env.begin_write().unwrap();
                abort.drop_database(&name(0)).unwrap();
                abort
                    .create_database(b"aborted", DatabaseKind::Ipv4)
                    .unwrap();
            }
            assert_eq!(env.begin_read().database_count(), 160);
            assert!(env.begin_read().kv(b"aborted").is_err());
            let old_id = env.current.load().catalog.get(&name(0)).unwrap().id;
            let mut write = env.begin_write().unwrap();
            write.drop_database(&name(0)).unwrap();
            write.create_database(&name(0), DatabaseKind::Ipv6).unwrap();
            write.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
            let p6 = crate::Prefix::new(crate::Address::V6(0), 0).unwrap();
            let p4 = crate::Prefix::new(crate::Address::V4(0), 0).unwrap();
            write.insert_prefix(&name(0), p6, b"six").unwrap();
            write.insert_prefix(b"v4", p4, b"four").unwrap();
            write.put(&name(159), b"tail", b"tail").unwrap();
            write.commit().unwrap();
            assert_ne!(env.current.load().catalog.get(&name(0)).unwrap().id, old_id);
            assert_eq!(escaped, b"old");
            assert_eq!(old.database_count(), 160);
            assert!(old.prefix_index(&name(0)).is_err());
            env.collect().unwrap();
            let sequence = env.sequence();
            drop(env);
            let reopened =
                Environment::open_with_encoding(&path.0, options, format, encoding).unwrap();
            assert_eq!(reopened.sequence(), sequence);
            let read = reopened.begin_read();
            assert_eq!(read.database_count(), 161);
            assert_eq!(
                read.get(&name(159), b"tail").unwrap(),
                Some(b"tail".as_slice())
            );
            assert_eq!(
                read.longest_match(&name(0), crate::Address::V6(1))
                    .unwrap()
                    .unwrap()
                    .1,
                b"six"
            );
            assert_eq!(
                read.longest_match(b"v4", crate::Address::V4(1))
                    .unwrap()
                    .unwrap()
                    .1,
                b"four"
            );
            assert_eq!(escaped, b"old");
        }
    }
}
