use crate::{DatabaseKind, Environment, PublicationMode};
use std::{fs, path::PathBuf};
#[test]
fn indexed_env_writes_keep_inventory_lazy_across_maintenance_reopen_and_pinned_reads() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let path = parent.join(format!(
            "ulmdb-inventory-env-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let env = Environment::create_with_publication(&path, PublicationMode::Indexed).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for n in 0..1024u64 {
            write
                .put(b"kv", &n.to_be_bytes(), &n.to_le_bytes())
                .unwrap();
        }
        write.commit().unwrap();
        let held = env.begin_read();
        for n in 1..=96u64 {
            let mut write = env.begin_write().unwrap();
            write
                .put(b"kv", &0u64.to_be_bytes(), &n.to_le_bytes())
                .unwrap();
            write.commit().unwrap();
            assert!(
                !env.current.load().manifest.segments.is_materialized(),
                "write {n} materialized inventory"
            );
            assert_eq!(
                held.get(b"kv", &0u64.to_be_bytes()).unwrap(),
                Some(0u64.to_le_bytes().as_slice())
            );
        }
        env.prune_inventory().unwrap();
        let mut write = env.begin_write().unwrap();
        write.put(b"kv", b"after-prune", b"value").unwrap();
        write.commit().unwrap();
        assert!(!env.current.load().manifest.segments.is_materialized());
        env.compact().unwrap();
        let mut write = env.begin_write().unwrap();
        write.put(b"kv", b"after-compact", b"value").unwrap();
        write.commit().unwrap();
        assert!(!env.current.load().manifest.segments.is_materialized());
        drop(env);
        let reopened = Environment::open(&path).unwrap();
        let mut write = reopened.begin_write().unwrap();
        write.put(b"kv", b"after-reopen", b"value").unwrap();
        write.commit().unwrap();
        assert!(!reopened.current.load().manifest.segments.is_materialized());
        for n in 1..1024u64 {
            assert_eq!(
                reopened.begin_read().get(b"kv", &n.to_be_bytes()).unwrap(),
                Some(n.to_le_bytes().as_slice())
            );
        }
        assert_eq!(
            held.get(b"kv", &0u64.to_be_bytes()).unwrap(),
            Some(0u64.to_le_bytes().as_slice())
        );
        drop(reopened);
        drop(held);
        fs::remove_dir_all(path).unwrap();
    }
}
