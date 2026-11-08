use super::*;
use std::{
    collections::HashSet,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn config(kind: &str) -> Config {
    Config {
        rows: 128,
        queries: 256,
        samples: 3,
        seed: 0xd1b54a32d192ed03,
        backend: "legacy".into(),
        key_kind: kind.into(),
        directory: PathBuf::new(),
    }
}

#[test]
fn fixtures_and_queries_have_declared_hit_miss_semantics() {
    for kind in ["fixed16", "hostname"] {
        let c = config(kind);
        let rows = workload::records(&c);
        let keys: HashSet<_> = rows.iter().map(|r| r.key.clone()).collect();
        assert_eq!(keys.len(), c.rows);
        assert!(rows.windows(2).all(|pair| pair[0].key < pair[1].key));
        for query in workload::queries(&c, "hit") {
            assert!(keys.contains(&query));
        }
        for query in workload::queries(&c, "miss") {
            assert!(!keys.contains(&query));
        }
        let mixed = workload::queries(&c, "mixed50");
        let hit_count = mixed.iter().filter(|q| keys.contains(*q)).count();
        assert!(hit_count > c.queries / 3 && hit_count < c.queries * 2 / 3);
    }
}

#[test]
fn query_seed_changes_stream_reproducibly() {
    let mut c = config("hostname");
    let first = workload::queries(&c, "hot90");
    assert_eq!(first, workload::queries(&c, "hot90"));
    c.seed += 1;
    assert_ne!(first, workload::queries(&c, "hot90"));
}

#[test]
fn hash_values_borrow_and_retain_correct_snapshot_for_both_backends() -> Result<()> {
    for backend in ["legacy", "resident"] {
        let mut c = config("hostname");
        c.backend = backend.into();
        c.directory = std::env::temp_dir().join(format!(
            "ulmdb-hash-validation-{}-{}-{backend}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let rows = workload::records(&c);
        {
            let env = Environment::create(&c, &rows)?;
            let old = env.snapshot();
            let view = old.view()?;
            let (hash, _) = HashSideIndex::build(&view, c.rows)?;
            for row in &rows {
                assert_eq!(hash.get(&row.key), view.get(&row.key));
                assert_eq!(
                    hash.get(&row.key).unwrap().as_ptr(),
                    view.get(&row.key).unwrap().as_ptr()
                );
                assert_eq!(hash.scan(&row.key, 100), view.scan(&row.key, 100));
            }
            env.update(&rows[0].key, &[0x44; 32])?;
            let new = env.snapshot();
            let new_view = new.view()?;
            let (new_hash, _) = HashSideIndex::build(&new_view, c.rows)?;
            assert_ne!(old.commit(), new.commit());
            assert_eq!(hash.get(&rows[0].key), Some(rows[0].value.as_slice()));
            assert_eq!(new_hash.get(&rows[0].key), Some([0x44; 32].as_slice()));
            for row in &rows[1..] {
                assert_eq!(new_hash.get(&row.key), Some(row.value.as_slice()));
            }
        }
        std::fs::remove_dir_all(&c.directory)?;
    }
    Ok(())
}
