use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix};

struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-duplicates-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn values(read: &ulmdb::ReadTxn, key: &[u8], reverse: bool) -> Vec<Vec<u8>> {
    read.duplicate_values(b"dups", key, Bound::Unbounded, Bound::Unbounded, reverse)
        .unwrap()
        .map(|item| item.unwrap().to_vec())
        .collect()
}
fn exercise(parent: &Path) {
    let path = Directory::new(parent);
    let env = Environment::create(&path.0).unwrap();
    let mut write = env.begin_write().unwrap();
    write
        .create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write
        .create_database(b"prefix", DatabaseKind::Ipv4)
        .unwrap();
    let mut expected = BTreeSet::new();
    for n in (0..1600u64).rev() {
        let value = n.to_be_bytes();
        assert!(write.insert_duplicate(b"dups", b"key", &value).unwrap());
        expected.insert(value.to_vec());
    }
    assert!(
        !write
            .insert_duplicate(b"dups", b"key", &100u64.to_be_bytes())
            .unwrap()
    );
    assert_eq!(write.duplicate_count(b"dups", b"key").unwrap(), 1600);
    assert!(
        write
            .contains_duplicate(b"dups", b"key", &100u64.to_be_bytes())
            .unwrap()
    );
    assert!(write.insert_duplicate(b"dups", b"", b"").unwrap());
    let large_key = vec![0xa5; 4096];
    let large_value = vec![0x81; 65_536];
    assert!(
        write
            .insert_duplicate(b"dups", &large_key, &large_value)
            .unwrap()
    );
    write.put(b"kv", b"status", b"ready").unwrap();
    let prefix = Prefix::new(Address::V4(0x0a000000), 8).unwrap();
    write.insert_prefix(b"prefix", prefix, b"profile").unwrap();
    let before = env.begin_read();
    assert!(matches!(
        before.duplicates(b"dups"),
        Err(Error::MissingDatabase)
    ));
    assert_eq!(write.commit().unwrap(), 1);
    let stable = env.begin_read();
    let prepared = stable.duplicates(b"dups").unwrap();
    assert_eq!(prepared.fixed_width(), None);
    assert_eq!(prepared.count(b"key").unwrap(), 1600);
    assert_eq!(prepared.count(b"missing").unwrap(), 0);
    assert_eq!(prepared.first(b"").unwrap(), Some(b"".as_slice()));
    assert_eq!(prepared.first(b"missing").unwrap(), None);
    assert!(prepared.contains(b"key", &100u64.to_be_bytes()).unwrap());
    assert!(!prepared.contains(b"key", b"missing").unwrap());
    assert_eq!(
        prepared.seek_pair(b"key", &100u64.to_be_bytes()).unwrap(),
        Some(100u64.to_be_bytes().as_slice())
    );
    assert_eq!(prepared.seek_pair(b"key", b"missing").unwrap(), None);
    let borrowed = prepared
        .seek_pair(&large_key, &large_value)
        .unwrap()
        .unwrap();
    let borrowed_from_temporary_query = {
        let query = large_value.clone();
        prepared.seek_pair(&large_key, &query).unwrap().unwrap()
    };
    assert_eq!(borrowed_from_temporary_query, large_value);
    assert_ne!(borrowed.as_ptr(), large_value.as_ptr());
    assert_eq!(borrowed, large_value);
    let expected: Vec<_> = expected.into_iter().collect();
    assert_eq!(values(&stable, b"key", false), expected);
    assert_eq!(
        values(&stable, b"key", true),
        expected.iter().rev().cloned().collect::<Vec<_>>()
    );
    let lower = 400u64.to_be_bytes();
    let upper = 405u64.to_be_bytes();
    let range = prepared
        .values(
            b"key",
            Bound::Excluded(&lower),
            Bound::Included(&upper),
            true,
        )
        .unwrap()
        .map(|item| item.unwrap().to_vec())
        .collect::<Vec<_>>();
    assert_eq!(
        range,
        (401..=405u64)
            .rev()
            .map(|n| n.to_be_bytes().to_vec())
            .collect::<Vec<_>>()
    );
    assert!(matches!(stable.get(b"dups", b"key"), Err(Error::WrongKind)));
    assert!(matches!(
        stable.prefix_index(b"dups"),
        Err(Error::WrongKind)
    ));
    assert!(matches!(stable.duplicates(b"kv"), Err(Error::WrongKind)));

    // Existing pairs and absent deletions are successful transaction no-ops.
    let mut noop = env.begin_write().unwrap();
    assert!(
        !noop
            .insert_duplicate(b"dups", b"key", &100u64.to_be_bytes())
            .unwrap()
    );
    assert!(!noop.delete_duplicate(b"dups", b"key", b"missing").unwrap());
    assert_eq!(noop.delete_duplicates(b"dups", b"missing").unwrap(), 0);
    assert_eq!(noop.dirty_bytes(), 0);
    assert_eq!(noop.commit().unwrap(), 1);

    let mut abort = env.begin_write().unwrap();
    abort.delete_duplicates(b"dups", b"key").unwrap();
    abort.put(b"kv", b"status", b"aborted").unwrap();
    abort.abort();
    assert_eq!(
        env.begin_read().duplicate_count(b"dups", b"key").unwrap(),
        1600
    );

    let mut write = env.begin_write().unwrap();
    assert!(
        write
            .delete_duplicate(b"dups", b"key", &100u64.to_be_bytes())
            .unwrap()
    );
    assert!(
        !write
            .delete_duplicate(b"dups", b"key", &100u64.to_be_bytes())
            .unwrap()
    );
    assert_eq!(write.duplicate_count(b"dups", b"key").unwrap(), 1599);
    assert_eq!(write.delete_duplicates(b"dups", &large_key).unwrap(), 1);
    assert!(write.delete_duplicate(b"dups", b"", b"").unwrap());
    assert_eq!(write.duplicate_count(b"dups", b"").unwrap(), 0);
    // In-place private mutations can retain roots; revision tracks them.
    assert!(write.insert_duplicate(b"dups", b"new", b"one").unwrap());
    assert!(write.insert_duplicate(b"dups", b"new", b"two").unwrap());
    assert!(write.delete_duplicate(b"dups", b"new", b"one").unwrap());
    write.put(b"kv", b"status", b"updated").unwrap();
    write.delete_prefix(b"prefix", prefix).unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    let now = env.begin_read();
    assert_eq!(now.duplicate_count(b"dups", b"key").unwrap(), 1599);
    assert_eq!(now.duplicate_count(b"dups", b"new").unwrap(), 1);
    assert_eq!(
        now.get(b"kv", b"status").unwrap(),
        Some(b"updated".as_slice())
    );
    assert!(
        now.longest_match(b"prefix", Address::V4(0x0a000001))
            .unwrap()
            .is_none()
    );
    assert_eq!(stable.duplicate_count(b"dups", b"key").unwrap(), 1600);
    assert_eq!(
        stable.get(b"kv", b"status").unwrap(),
        Some(b"ready".as_slice())
    );
    assert_eq!(borrowed, large_value);
    drop(env);
    assert_eq!(borrowed, large_value);
    let reopened = Environment::open(&path.0).unwrap();
    let read = reopened.begin_read();
    assert_eq!(
        read.database_kind(b"dups").unwrap(),
        DatabaseKind::Duplicates { width: None }
    );
    assert_eq!(read.duplicate_count(b"dups", b"key").unwrap(), 1599);
    assert_eq!(
        read.duplicate_first(b"dups", b"new").unwrap(),
        Some(b"two".as_slice())
    );
}

#[test]
fn sorted_duplicates_transactions_and_reopen_on_disk() {
    exercise(&std::env::temp_dir());
}
#[test]
fn sorted_duplicates_transactions_and_reopen_on_ramdrive() {
    assert!(Path::new("/dev/shm").is_dir());
    exercise(Path::new("/dev/shm"));
}

#[test]
fn fixed_width_zero_and_errors_require_abort() {
    let path = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&path.0).unwrap();
    let mut write = env.begin_write().unwrap();
    for width in [0, 4, 65_536] {
        write
            .create_database(
                width.to_string().as_bytes(),
                DatabaseKind::Duplicates { width: Some(width) },
            )
            .unwrap();
    }
    write
        .create_database(b"variable", DatabaseKind::Duplicates { width: None })
        .unwrap();
    write.insert_duplicate(b"0", b"", b"").unwrap();
    write.insert_duplicate(b"4", b"key", b"four").unwrap();
    write
        .insert_duplicate(b"65536", b"key", &vec![1; 65_536])
        .unwrap();
    write.commit().unwrap();
    let read = env.begin_read();
    assert_eq!(read.duplicates(b"0").unwrap().fixed_width(), Some(0));
    assert_eq!(
        read.duplicate_first(b"0", b"").unwrap(),
        Some(b"".as_slice())
    );
    assert!(read.duplicate_seek_pair(b"4", b"key", b"bad").is_err());
    for scenario in 0..5 {
        let mut write = env.begin_write().unwrap();
        let result = match scenario {
            0 => write.insert_duplicate(b"4", b"key", b"bad"),
            1 => write.insert_duplicate(b"variable", &vec![0; 4097], b"value"),
            2 => write.insert_duplicate(b"variable", b"key", &vec![0; 65_537]),
            3 => write.delete_duplicate(b"4", b"key", b"bad"),
            _ => write.insert_duplicate(b"missing", b"key", b"value"),
        };
        assert!(result.is_err());
        assert!(write.insert_duplicate(b"4", b"key", b"next").is_err());
        assert!(write.commit().is_err());
    }
    let mut write = env.begin_write().unwrap();
    assert!(
        write
            .create_database(
                b"too-wide",
                DatabaseKind::Duplicates {
                    width: Some(65_537)
                }
            )
            .is_err()
    );
    write.abort();
    drop(env);
    let reopened = Environment::open(&path.0).unwrap();
    assert_eq!(
        reopened.begin_read().database_kind(b"4").unwrap(),
        DatabaseKind::Duplicates { width: Some(4) }
    );
}

#[test]
fn differential_duplicate_mutations_against_ordered_sets() {
    let path = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&path.0).unwrap();
    let mut write = env.begin_write().unwrap();
    write
        .create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    let mut model: BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>> = BTreeMap::new();
    let mut seed = 0x1234_5678_9abc_def0u64;
    for step in 0..4000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let key = vec![(seed % 19) as u8];
        let value = ((seed >> 8) % 127).to_be_bytes().to_vec();
        match step % 11 {
            0 => {
                assert_eq!(
                    write.delete_duplicates(b"dups", &key).unwrap(),
                    model.remove(&key).map_or(0, |set| set.len() as u64)
                );
            }
            1..=3 => {
                let expected = model.get_mut(&key).is_some_and(|set| set.remove(&value));
                assert_eq!(
                    write.delete_duplicate(b"dups", &key, &value).unwrap(),
                    expected
                );
            }
            _ => {
                let expected = model.entry(key.clone()).or_default().insert(value.clone());
                assert_eq!(
                    write.insert_duplicate(b"dups", &key, &value).unwrap(),
                    expected
                );
            }
        }
        assert_eq!(
            write.duplicate_count(b"dups", &key).unwrap(),
            model.get(&key).map_or(0, |set| set.len() as u64)
        );
    }
    write.commit().unwrap();
    let read = env.begin_read();
    for n in 0..19 {
        let key = vec![n];
        assert_eq!(
            values(&read, &key, false),
            model
                .remove(&key)
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>()
        );
    }
}
