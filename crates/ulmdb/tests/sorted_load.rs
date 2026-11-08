use std::{collections::BTreeMap, ops::Bound, path::PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode};

struct Directory(PathBuf);
impl Directory {
    fn new(ram: bool) -> Self {
        let base = if ram {
            PathBuf::from("/dev/shm")
        } else {
            std::env::temp_dir()
        };
        Self(base.join(format!(
            "ulmdb-sorted-load-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn initialize(directory: &Directory, mode: PublicationMode) -> Environment {
    let environment = Environment::create_with_publication(&directory.0, mode).unwrap();
    let mut writer = environment.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
        (b"keep", DatabaseKind::Kv),
    ] {
        writer.create_database(name, kind).unwrap();
    }
    writer.put(b"keep", b"old", b"stable").unwrap();
    assert_eq!(writer.commit().unwrap(), 1);
    environment
}
fn prefixes(v6: bool) -> Vec<(Prefix, [u8; 2])> {
    (0..=if v6 { 128 } else { 32 })
        .map(|length| {
            (
                Prefix::new(if v6 { Address::V6(0) } else { Address::V4(0) }, length).unwrap(),
                u16::from(length).to_be_bytes(),
            )
        })
        .collect()
}

#[test]
fn sorted_load_is_atomic_across_indexes_and_preserves_old_borrows_after_reopen() {
    for ram in [false, true] {
        for mode in [
            PublicationMode::Legacy,
            PublicationMode::Cohort,
            PublicationMode::Indexed,
            PublicationMode::Range,
        ] {
            let directory = Directory::new(ram);
            let environment = initialize(&directory, mode);
            let old = environment.begin_read();
            let borrowed = old.get(b"keep", b"old").unwrap().unwrap();
            let mut expected: BTreeMap<Vec<u8>, Vec<u8>> = (0..512u32)
                .map(|n| (n.to_be_bytes().to_vec(), n.to_le_bytes().to_vec()))
                .collect();
            expected.insert(Vec::new(), Vec::new());
            expected.insert(vec![255; 4096], vec![71; 65_536]);
            let mut writer = environment.begin_write().unwrap();
            assert_eq!(
                writer
                    .load_sorted_kv(b"kv", expected.iter().map(|(k, v)| Ok((k, v))))
                    .unwrap(),
                514
            );
            for v6 in [false, true] {
                let input = prefixes(v6);
                assert_eq!(
                    writer
                        .load_sorted_prefixes(
                            if v6 { b"v6" } else { b"v4" },
                            input.iter().map(|(p, v)| Ok((*p, v)))
                        )
                        .unwrap(),
                    input.len() as u64
                );
            }
            assert!(old.get(b"kv", &0u32.to_be_bytes()).unwrap().is_none());
            writer.put(b"kv", &7u32.to_be_bytes(), b"changed").unwrap();
            expected.insert(7u32.to_be_bytes().to_vec(), b"changed".to_vec());
            assert_eq!(writer.commit().unwrap(), 2);
            assert_eq!(old.sequence(), 1);
            assert!(
                old.exact_prefix(b"v4", prefixes(false)[0].0)
                    .unwrap()
                    .is_none()
            );
            let current = environment.begin_read();
            let actual: BTreeMap<Vec<u8>, Vec<u8>> = current
                .range(b"kv", Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .map(|p| {
                    let (k, v) = p.unwrap();
                    (k.to_vec(), v.to_vec())
                })
                .collect();
            assert_eq!(actual, expected);
            for v6 in [false, true] {
                for (prefix, value) in prefixes(v6) {
                    assert_eq!(
                        current
                            .exact_prefix(if v6 { b"v6" } else { b"v4" }, prefix)
                            .unwrap()
                            .unwrap()
                            .1,
                        value
                    );
                }
                let found = current
                    .longest_match(
                        if v6 { b"v6" } else { b"v4" },
                        if v6 { Address::V6(1) } else { Address::V4(1) },
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(found.0.length(), if v6 { 127 } else { 31 });
            }
            ulmdb::backup::verify(&environment).unwrap();
            drop(current);
            drop(environment);
            let reopened = Environment::open(&directory.0).unwrap();
            let read = reopened.begin_read();
            for (key, value) in &expected {
                assert_eq!(read.get(b"kv", key).unwrap(), Some(value.as_slice()));
            }
            assert_eq!(read.sequence(), 2);
            assert_eq!(borrowed, b"stable");
            assert!(old.get(b"kv", &7u32.to_be_bytes()).unwrap().is_none());
        }
    }
}

#[test]
fn empty_load_and_precondition_errors_preserve_sequence_and_existing_data() {
    for mode in [PublicationMode::Indexed, PublicationMode::Range] {
        let directory = Directory::new(false);
        let environment = initialize(&directory, mode);
        let mut writer = environment.begin_write().unwrap();
        assert_eq!(
            writer
                .load_sorted_kv(b"kv", std::iter::empty::<ulmdb::Result<(&[u8], &[u8])>>())
                .unwrap(),
            0
        );
        assert_eq!(
            writer
                .load_sorted_prefixes(b"v4", std::iter::empty::<ulmdb::Result<(Prefix, &[u8])>>())
                .unwrap(),
            0
        );
        assert!(matches!(
            writer.load_sorted_kv(b"v4", [Ok((b"a", b"b"))]),
            Err(Error::WrongKind)
        ));
        assert!(matches!(
            writer.load_sorted_prefixes(b"kv", [Ok((prefixes(false)[0].0, b"b"))]),
            Err(Error::WrongKind)
        ));
        assert!(matches!(
            writer.load_sorted_kv(b"absent", [Ok((b"a", b"b"))]),
            Err(Error::MissingDatabase)
        ));
        assert!(writer.load_sorted_kv(b"keep", [Ok((b"a", b"b"))]).is_err());
        assert_eq!(
            writer.get(b"keep", b"old").unwrap(),
            Some(b"stable".as_slice())
        );
        assert_eq!(writer.commit().unwrap(), 1);
    }
}

#[test]
fn late_input_errors_poison_and_savepoint_rollback_restores_atomic_loading() {
    for mode in [PublicationMode::Indexed, PublicationMode::Range] {
        let directory = Directory::new(false);
        let environment = initialize(&directory, mode);
        let mut writer = environment.begin_write().unwrap();
        let dirty = writer.dirty_bytes();
        {
            let mut scope = writer.savepoint().unwrap();
            let records = (0..512u32)
                .map(|n| Ok((n.to_be_bytes(), n.to_le_bytes())))
                .chain(std::iter::once(Err(Error::InvalidInput(
                    "late source error",
                ))));
            assert!(scope.load_sorted_kv(b"kv", records).is_err());
            assert!(scope.put(b"keep", b"new", b"hidden").is_err());
            scope.rollback().unwrap();
            assert_eq!(scope.dirty_bytes(), dirty);
            let wrong = vec![
                Ok((Prefix::new(Address::V4(0), 0).unwrap(), b"a")),
                Ok((Prefix::new(Address::V4(0), 8).unwrap(), b"b")),
                Ok((Prefix::new(Address::V6(0), 16).unwrap(), b"c")),
            ];
            assert!(scope.load_sorted_prefixes(b"v4", wrong).is_err());
            assert!(scope.release().is_err()); // A failed release rolls back the scope.
        }
        assert_eq!(writer.dirty_bytes(), dirty);
        assert!(writer.get(b"kv", &0u32.to_be_bytes()).unwrap().is_none());
        assert_eq!(
            writer
                .load_sorted_kv(b"kv", [Ok((b"a", b"published"))])
                .unwrap(),
            1
        );
        assert_eq!(writer.commit().unwrap(), 2);
        let mut failed = environment.begin_write().unwrap();
        assert!(
            failed
                .load_sorted_prefixes(
                    b"v4",
                    [
                        Ok((Prefix::new(Address::V4(0), 8).unwrap(), b"x")),
                        Ok((Prefix::new(Address::V4(0), 0).unwrap(), b"y")),
                    ]
                )
                .is_err()
        );
        assert!(failed.commit().is_err());
        let read = environment.begin_read();
        assert_eq!(read.sequence(), 2);
        assert!(
            read.exact_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            read.get(b"kv", b"a").unwrap(),
            Some(b"published".as_slice())
        );
    }
}
