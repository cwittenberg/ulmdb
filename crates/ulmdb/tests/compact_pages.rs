//! Real mapped transactions exercise V2 alongside the unchanged arbitrary-byte API.
use std::{collections::BTreeMap, ops::Bound, path::PathBuf};
use ulmdb::{
    DatabaseKind,
    hybrid::{Environment, Options},
};
fn paths() -> impl Iterator<Item = PathBuf> {
    [std::env::temp_dir(), PathBuf::from("/dev/shm")]
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|p| {
            p.join(format!(
                "ulmdb-compact-pages-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            ))
        })
}
fn check(env: &Environment, model: &BTreeMap<Vec<u8>, Vec<u8>>) {
    let r = env.begin_read();
    let kv = r.kv(b"kv").unwrap();
    for (key, value) in model {
        assert_eq!(kv.get(key).unwrap(), Some(value.as_slice()));
    }
    for reverse in [false, true] {
        let got = kv
            .cursor(Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|p| {
                let (k, v) = p.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect::<Vec<_>>();
        let mut want: Vec<_> = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if reverse {
            want.reverse();
        }
        assert_eq!(got, want);
    }
}
#[test]
fn uniform_mixed_overflow_abort_savepoint_and_escaped_reopen() {
    for path in paths() {
        let mut options = Options::default();
        options.chunk.capacity = 16 * 1024 * 1024;
        options.max_image_bytes = 64 * 1024 * 1024;
        let env = Environment::create(&path, options).unwrap();
        let mut model = BTreeMap::new();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            for n in 0..1800u64 {
                let k = n.to_be_bytes().to_vec();
                let v = n.to_le_bytes().to_vec();
                w.put(b"kv", &k, &v).unwrap();
                model.insert(k, v);
            }
            w.commit().unwrap();
        }
        let old = env.begin_read();
        let held = old.get(b"kv", &700u64.to_be_bytes()).unwrap().unwrap();
        {
            let mut w = env.begin_write().unwrap();
            for n in 200..1000u64 {
                let k = n.to_be_bytes().to_vec();
                let v = if n % 7 == 0 {
                    vec![n as u8; 2049]
                } else if n % 5 == 0 {
                    vec![]
                } else {
                    (!n).to_le_bytes().to_vec()
                };
                w.put(b"kv", &k, &v).unwrap();
                model.insert(k, v);
            }
            {
                let mut save = w.savepoint().unwrap();
                save.put(b"kv", b"aborted", b"bad").unwrap();
                save.delete(b"kv", &700u64.to_be_bytes()).unwrap();
                save.rollback().unwrap();
            }
            for n in (0..1800u64).step_by(9) {
                let k = n.to_be_bytes().to_vec();
                assert!(w.delete(b"kv", &k).unwrap());
                model.remove(&k);
            }
            w.put(b"kv", b"", b"").unwrap();
            model.insert(vec![], vec![]);
            let long = vec![0x66; 513];
            w.put(b"kv", &long, b"long-key").unwrap();
            model.insert(long, b"long-key".to_vec());
            w.commit().unwrap();
        }
        {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"writer-abort", b"bad").unwrap();
        }
        check(&env, &model);
        assert_eq!(held, 700u64.to_le_bytes());
        drop(env);
        let env = Environment::open(&path, options).unwrap();
        check(&env, &model);
        assert_eq!(held, 700u64.to_le_bytes());
        {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"continued", b"after-reopen").unwrap();
            w.commit().unwrap();
            model.insert(b"continued".to_vec(), b"after-reopen".to_vec());
        }
        check(&env, &model);
        drop(env);
        drop(old);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn resident_full_compact_leaf_empty_value_endpoint_survives_cold_replay() {
    use ulmdb::resident::{Environment as Resident, Options as ResidentOptions, RedoFormat};
    for path in paths() {
        let mut options = ResidentOptions::default();
        options.pool.kv_pages = 64;
        options.pool.tiny_pages = 64;
        options.auto_collect_below = 8;
        let env = Resident::create_with_redo(&path, options, RedoFormat::CompactV2).unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            for n in 0..254u16 {
                let mut k = [0; 16];
                k[14..].copy_from_slice(&n.to_be_bytes());
                w.put(b"kv", &k, b"").unwrap();
            }
            w.commit().unwrap();
        }
        let old = env.begin_read();
        let mut last = [0; 16];
        last[14..].copy_from_slice(&253u16.to_be_bytes());
        let held = old.get(b"kv", &last).unwrap().unwrap();
        assert!(held.is_empty());
        drop(env);
        let env = Resident::open_with_redo(&path, options, RedoFormat::CompactV2).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", &last).unwrap(),
            Some([].as_slice())
        );
        assert!(held.is_empty());
        {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", &last, b"changed").unwrap();
            w.commit().unwrap();
        }
        assert_eq!(
            env.begin_read().get(b"kv", &last).unwrap(),
            Some(b"changed".as_slice())
        );
        assert!(held.is_empty());
        drop(env);
        drop(old);
        std::fs::remove_dir_all(path).unwrap();
    }
}
