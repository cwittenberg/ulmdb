use std::path::{Path, PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode};
use ulmdb_reference::catalog as model;

struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-outcomes-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn modes() -> [PublicationMode; 5] {
    [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
        PublicationMode::RangeV5,
        PublicationMode::Range,
    ]
}
fn prefix() -> Prefix {
    Prefix::new(Address::V4(0x0a000000), 8).unwrap()
}
fn initialize(path: &Path, mode: PublicationMode) -> Environment {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(4) })
        .unwrap();
    w.create_database(b"prefix", DatabaseKind::Ipv4).unwrap();
    w.insert_duplicate(b"dups", b"key", b"base").unwrap();
    w.insert_prefix(b"prefix", prefix(), b"base").unwrap();
    w.commit().unwrap();
    env
}
fn check_other_indexes(env: &Environment) {
    let r = env.begin_read();
    assert_eq!(r.duplicate_count(b"dups", b"key").unwrap(), 1);
    assert_eq!(
        r.longest_match(b"prefix", Address::V4(0x0a000001))
            .unwrap()
            .unwrap()
            .1,
        b"base"
    );
}

#[test]
fn outcomes_match_independent_reference_private_sequences_and_committed_snapshots() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for mode in modes() {
            let path = Directory::new(&parent);
            let env = initialize(&path.0, mode);
            let mut reference = model::Environment::new(42);
            let mut seed = reference.begin_write().unwrap();
            let h = seed.create(b"kv", model::Kind::Kv).unwrap();
            seed.commit().unwrap();
            let mut keys = vec![Vec::new(), vec![0; 4096]];
            keys.extend((0u32..47).map(|x| x.to_be_bytes().to_vec()));
            let mut rng = 0x1234_5678_8765_4321u64;
            for round in 0..8 {
                let old = env.begin_read();
                let (_, old_model) = reference.snapshot();
                let mut w = env.begin_write().unwrap();
                let mut oracle = reference.begin_write().unwrap();
                for step in 0..128 {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let key = &keys[rng as usize % keys.len()];
                    if rng & 3 == 0 {
                        let expected = oracle.private.delete(h, key).unwrap().is_some();
                        assert_eq!(w.delete_with_presence(b"kv", key).unwrap(), expected);
                    } else {
                        let len = match step % 13 {
                            0 => 8192,
                            1 => 0,
                            _ => 17,
                        };
                        let value = vec![(rng >> 32) as u8; len];
                        let expected = oracle.private.put(h, key, &value).unwrap();
                        let actual = w.put_with_previous(b"kv", key, &value).unwrap();
                        assert_eq!(actual, expected, "round {round}, step {step}, {mode:?}");
                    }
                    for k in &keys {
                        assert_eq!(
                            w.get(b"kv", k).unwrap(),
                            oracle.private.kv(h).unwrap().get(k).map(Vec::as_slice)
                        );
                        assert_eq!(
                            old.get(b"kv", k).unwrap(),
                            old_model.kv(h).unwrap().get(k).map(Vec::as_slice)
                        );
                    }
                }
                if round % 3 == 1 {
                    w.abort();
                    oracle.abort();
                } else {
                    w.commit().unwrap();
                    oracle.commit().unwrap();
                }
                let (_, expected) = reference.snapshot();
                for key in &keys {
                    assert_eq!(
                        env.begin_read().get(b"kv", key).unwrap(),
                        expected.kv(h).unwrap().get(key).map(Vec::as_slice)
                    );
                    assert_eq!(
                        old.get(b"kv", key).unwrap(),
                        old_model.kv(h).unwrap().get(key).map(Vec::as_slice)
                    );
                }
                check_other_indexes(&env);
            }
            drop(env);
            let reopened = Environment::open(&path.0).unwrap();
            let (_, expected) = reference.snapshot();
            for key in &keys {
                assert_eq!(
                    reopened.begin_read().get(b"kv", key).unwrap(),
                    expected.kv(h).unwrap().get(key).map(Vec::as_slice)
                );
            }
            check_other_indexes(&reopened);
        }
    }
}

#[test]
fn present_empty_outcomes_are_owned_across_private_rewrites_savepoints_abort_and_reopen() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for mode in modes() {
            let path = Directory::new(&parent);
            let env = initialize(&path.0, mode);
            let mut w = env.begin_write().unwrap();
            assert_eq!(w.put_with_previous(b"kv", b"", b"").unwrap(), None);
            assert_eq!(
                w.put_with_previous(b"kv", b"", b"private").unwrap(),
                Some(Vec::new())
            );
            let prior = w
                .put_with_previous(b"kv", b"", &vec![7; 8192])
                .unwrap()
                .unwrap();
            assert_eq!(prior, b"private");
            w.commit().unwrap();
            let old = env.begin_read();
            let mut w = env.begin_write().unwrap();
            {
                let mut outer = w.savepoint().unwrap();
                let before = outer
                    .put_with_previous(b"kv", b"", &vec![8; 8192])
                    .unwrap()
                    .unwrap();
                {
                    let mut inner = outer.savepoint().unwrap();
                    assert!(inner.delete_with_presence(b"kv", b"").unwrap());
                    assert!(!inner.delete_with_presence(b"kv", b"").unwrap());
                    assert_eq!(inner.put_with_previous(b"kv", b"", b"").unwrap(), None);
                    inner.release().unwrap();
                }
                assert_eq!(before, vec![7; 8192]);
                outer.rollback().unwrap();
                assert_eq!(
                    outer.put_with_previous(b"kv", b"", b"accepted").unwrap(),
                    Some(vec![7; 8192])
                );
                outer.release().unwrap();
            }
            assert_eq!(prior, b"private");
            w.commit().unwrap();
            assert_eq!(old.get(b"kv", b"").unwrap(), Some(vec![7; 8192].as_slice()));
            let mut w = env.begin_write().unwrap();
            assert!(w.delete_with_presence(b"kv", b"").unwrap());
            w.abort();
            drop(env);
            let reopened = Environment::open(&path.0).unwrap();
            assert_eq!(
                reopened.begin_read().get(b"kv", b"").unwrap(),
                Some(b"accepted".as_slice())
            );
            assert_eq!(old.get(b"kv", b"").unwrap(), Some(vec![7; 8192].as_slice()));
            check_other_indexes(&reopened);
        }
    }
}

#[test]
fn validation_is_nonmutating_and_failed_writer_remains_ineligible_until_child_rollback() {
    let path = Directory::new(&std::env::temp_dir());
    let env = initialize(&path.0, PublicationMode::Indexed);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"base", b"preserved").unwrap();
    let dirty = w.dirty_bytes();
    assert!(matches!(
        w.put_with_previous(b"missing", b"key", b"v"),
        Err(Error::MissingDatabase)
    ));
    assert!(matches!(
        w.put_with_previous(b"prefix", b"key", b"v"),
        Err(Error::WrongKind)
    ));
    assert!(matches!(
        w.delete_with_presence(b"prefix", b"key"),
        Err(Error::WrongKind)
    ));
    assert!(matches!(
        w.delete_with_presence(b"missing", b"key"),
        Err(Error::MissingDatabase)
    ));
    assert!(matches!(
        w.put_with_previous(b"kv", &vec![0; 4097], b"v"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        w.delete_with_presence(b"kv", &vec![0; 4097]),
        Err(Error::InvalidInput(_))
    ));
    assert!(!w.delete_with_presence(b"kv", b"absent").unwrap());
    assert_eq!(w.dirty_bytes(), dirty);
    assert_eq!(
        w.get(b"kv", b"base").unwrap(),
        Some(b"preserved".as_slice())
    );
    {
        let mut child = w.savepoint().unwrap();
        assert!(
            child
                .insert_duplicate(b"dups", b"key", b"wrong-width")
                .is_err()
        );
        assert!(child.put_with_previous(b"kv", b"base", b"wrong").is_err());
        assert!(child.delete_with_presence(b"kv", b"base").is_err());
        child.rollback().unwrap();
        assert_eq!(
            child
                .put_with_previous(b"kv", b"base", b"recovered")
                .unwrap(),
            Some(b"preserved".to_vec())
        );
        child.abort().unwrap();
    }
    assert_eq!(
        w.get(b"kv", b"base").unwrap(),
        Some(b"preserved".as_slice())
    );
    w.commit().unwrap();
}
