use std::{
    collections::BTreeMap,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{DatabaseKind, Environment, PublicationMode};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-sorted-compact-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn rows(read: &ulmdb::ReadTxn) -> Vec<(Vec<u8>, Vec<u8>)> {
    read.range(b"kv", Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|pair| pair.map(|(k, v)| (k.to_vec(), v.to_vec())).unwrap())
        .collect()
}
#[test]
fn actual_compaction_rebuilds_sparse_deleted_and_overflow_kv_with_independent_ordered_oracle() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for mode in [PublicationMode::Legacy, PublicationMode::Indexed] {
            let directory = Directory::new(&parent);
            let env = Environment::create_with_publication(&directory.0, mode).unwrap();
            let mut expected = BTreeMap::new();
            expected.insert(Vec::new(), Vec::new());
            for n in 0..3000u64 {
                expected.insert(
                    n.to_be_bytes().to_vec(),
                    if n % 11 == 0 {
                        vec![n as u8; 513]
                    } else {
                        n.to_le_bytes().to_vec()
                    },
                );
            }
            expected.insert(vec![0xfd; 257], vec![0x41; 513]);
            expected.insert(vec![0xfd; 4096], vec![0x62; 65537]);
            let original = expected.clone();
            let mut write = env.begin_write().unwrap();
            write.create_database(b"kv", DatabaseKind::Kv).unwrap();
            write.create_database(b"empty", DatabaseKind::Kv).unwrap();
            for (key, value) in &expected {
                write.put(b"kv", key, value).unwrap();
            }
            write.commit().unwrap();
            let before_deletion = env.begin_read();
            let mut write = env.begin_write().unwrap();
            for n in 0..3000u64 {
                if n % 3 != 0 {
                    write.delete(b"kv", &n.to_be_bytes()).unwrap();
                    expected.remove(n.to_be_bytes().as_slice());
                }
            }
            write.commit().unwrap();
            let sparse = env.begin_read();
            let borrowed = sparse.get(b"kv", &3u64.to_be_bytes()).unwrap().unwrap();
            let handle = sparse.open_database(b"kv").unwrap();
            let report = env.compact().unwrap();
            assert_eq!(report.kv_pairs, expected.len() as u64);
            assert_eq!(report.sequence, sparse.sequence());
            assert!(report.after_segment_bytes < report.before_segment_bytes);
            let now = env.begin_read();
            assert_eq!(
                rows(&now),
                expected
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(rows(&sparse), rows(&now));
            assert_eq!(
                rows(&before_deletion),
                original.into_iter().collect::<Vec<_>>()
            );
            assert_eq!(
                now.kv_handle(&handle)
                    .unwrap()
                    .get(&3u64.to_be_bytes())
                    .unwrap(),
                Some(&3u64.to_le_bytes()[..])
            );
            assert_eq!(
                now.range(b"empty", Bound::Unbounded, Bound::Unbounded, false)
                    .unwrap()
                    .count(),
                0
            );
            assert_eq!(borrowed, 3u64.to_le_bytes());
            ulmdb::backup::verify(&env).unwrap();
            drop(now);
            drop(env);
            let cold = Environment::open(&directory.0).unwrap();
            ulmdb::backup::verify(&cold).unwrap();
            assert_eq!(
                rows(&cold.begin_read()),
                expected.into_iter().collect::<Vec<_>>()
            );
            assert_eq!(borrowed, 3u64.to_le_bytes());
        }
    }
}
