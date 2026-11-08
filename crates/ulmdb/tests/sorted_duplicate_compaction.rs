use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    path::PathBuf,
};
use ulmdb::{DatabaseKind, Environment, PublicationMode, ReadTxn};
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
type Oracle = BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>>;
fn variable() -> Oracle {
    let mut result = Oracle::new();
    result.insert(
        Vec::new(),
        [Vec::new(), b"a".to_vec()].into_iter().collect(),
    );
    for n in 0..350u32 {
        let key = if n % 17 == 0 {
            let mut key = vec![0x63; 4096];
            key[..4].copy_from_slice(&n.to_be_bytes());
            key
        } else {
            n.to_be_bytes().to_vec()
        };
        let count = if n % 7 == 0 { 40 } else { 3 };
        let mut values: BTreeSet<_> = (0..count as u32)
            .map(|v| v.to_be_bytes().to_vec())
            .collect();
        if n % 13 == 0 {
            values.insert(vec![0x7f; 65_536]);
        }
        result.insert(key, values);
    }
    result
}
fn check(read: &ReadTxn, name: &[u8], expected: &Oracle) {
    let want: Vec<_> = expected
        .iter()
        .flat_map(|(k, values)| values.iter().map(move |v| (k.clone(), v.clone())))
        .collect();
    for reverse in [false, true] {
        let actual: Vec<_> = read
            .duplicate_pairs(name, Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|r| {
                let (k, v) = r.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect();
        let mut ordered = want.clone();
        if reverse {
            ordered.reverse();
        }
        assert_eq!(actual, ordered);
    }
    for (key, values) in expected {
        assert_eq!(
            read.duplicate_count(name, key).unwrap(),
            values.len() as u64
        );
        for value in values {
            assert!(read.duplicates(name).unwrap().contains(key, value).unwrap());
            assert_eq!(
                read.duplicate_seek_pair(name, key, value).unwrap(),
                Some(value.as_slice())
            );
        }
        let lower = values.first().unwrap();
        let upper = values.last().unwrap();
        let actual: Vec<_> = read
            .duplicate_values(
                name,
                key,
                Bound::Excluded(lower.as_slice()),
                Bound::Included(upper.as_slice()),
                false,
            )
            .unwrap()
            .map(|v| v.unwrap().to_vec())
            .collect();
        let want: Vec<_> = values
            .iter()
            .filter(|v| *v > lower && *v <= upper)
            .cloned()
            .collect();
        assert_eq!(actual, want);
    }
}
#[test]
fn direct_duplicate_and_catalog_compaction_preserves_oracles_handles_and_old_borrows() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for mode in [
            PublicationMode::Legacy,
            PublicationMode::Cohort,
            PublicationMode::Indexed,
        ] {
            let directory = Directory(parent.join(format!(
                "ulmdb-duplicate-bulk-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            )));
            let env = Environment::create_with_publication(&directory.0, mode).unwrap();
            let mut write = env.begin_write().unwrap();
            let mut var = variable();
            let fixed: Oracle = (0..30u32)
                .map(|n| {
                    (
                        n.to_be_bytes().to_vec(),
                        (0..20u32).map(|v| v.to_be_bytes().to_vec()).collect(),
                    )
                })
                .collect();
            let zero: Oracle = (0..20u32)
                .map(|n| (n.to_be_bytes().to_vec(), [Vec::new()].into_iter().collect()))
                .collect();
            let wide: Oracle = [(
                b"wide".to_vec(),
                [vec![0x11; 65_536], vec![0x22; 65_536]]
                    .into_iter()
                    .collect(),
            )]
            .into_iter()
            .collect();
            for (name, width, rows) in [
                (b"var".as_slice(), None, &var),
                (b"fixed", Some(4), &fixed),
                (b"zero", Some(0), &zero),
                (b"wide", Some(65_536), &wide),
            ] {
                write
                    .create_database(name, DatabaseKind::Duplicates { width })
                    .unwrap();
                for (key, values) in rows {
                    for value in values {
                        assert!(write.insert_duplicate(name, key, value).unwrap());
                        assert!(!write.insert_duplicate(name, key, value).unwrap());
                    }
                }
            }
            let mut names = Vec::new();
            for n in 0..300u32 {
                let mut name = format!("empty-{n:04}").into_bytes();
                name.resize(255, 0x7f);
                write.create_database(&name, DatabaseKind::Kv).unwrap();
                names.push(name);
            }
            write.create_database(b"dropped", DatabaseKind::Kv).unwrap();
            write.commit().unwrap();
            let mut write = env.begin_write().unwrap();
            write.drop_database(b"dropped").unwrap();
            let key = 7u32.to_be_bytes();
            for v in 0..38u32 {
                write
                    .delete_duplicate(b"var", &key, &v.to_be_bytes())
                    .unwrap();
                var.get_mut(key.as_slice())
                    .unwrap()
                    .remove(v.to_be_bytes().as_slice());
            }
            write.commit().unwrap();
            let old = env.begin_read();
            let handle = old.open_database(b"var").unwrap();
            let before = old.stats();
            let borrowed = old
                .duplicate_seek_pair(b"wide", b"wide", &vec![0x11; 65_536])
                .unwrap()
                .unwrap();
            let report = env.compact().unwrap();
            assert_eq!(
                report.duplicate_pairs,
                [&var, &fixed, &zero, &wide]
                    .iter()
                    .map(|r| r.values().map(|v| v.len() as u64).sum::<u64>())
                    .sum::<u64>()
            );
            let current = env.begin_read();
            assert_eq!(current.open_database(b"var").unwrap().id(), handle.id());
            assert_eq!(
                (current.stats().sequence, current.stats().commit),
                (before.sequence, before.commit)
            );
            assert_ne!(current.generation(), old.generation());
            for name in &names {
                assert_eq!(
                    current.open_database(name).unwrap().id(),
                    old.open_database(name).unwrap().id()
                );
            }
            assert!(current.open_database(b"dropped").is_err());
            for (name, oracle) in [
                (b"var".as_slice(), &var),
                (b"fixed", &fixed),
                (b"zero", &zero),
                (b"wide", &wide),
            ] {
                check(&old, name, oracle);
                check(&current, name, oracle);
            }
            let mut write = env.begin_write().unwrap();
            let mut savepoint = write.savepoint().unwrap();
            savepoint.delete_duplicates(b"var", b"").unwrap();
            savepoint.rollback().unwrap();
            savepoint.release().unwrap();
            write.commit().unwrap();
            drop(current);
            drop(env);
            assert_eq!(borrowed, &vec![0x11; 65_536]);
            let reopened = Environment::open(&directory.0).unwrap();
            let read = reopened.begin_read();
            for (name, oracle) in [
                (b"var".as_slice(), &var),
                (b"fixed", &fixed),
                (b"zero", &zero),
                (b"wide", &wide),
            ] {
                check(&read, name, oracle);
            }
            assert_eq!(borrowed, &vec![0x11; 65_536]);
        }
    }
}
