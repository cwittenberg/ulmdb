use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{DatabaseKind, Environment, PublicationMode, backup};

struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-indexed-upgrade-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn exercise(parent: &Path, initial: PublicationMode) {
    let dir = Directory::new(parent);
    let env = Environment::create_with_publication(&dir.0, initial).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"key", b"old").unwrap();
    write.commit().unwrap();
    // Migration must not copy borrowed data, change logical lineage or make
    // a retained legacy snapshot depend on the new metadata format.
    let old = env.begin_read();
    let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
    env.retain_snapshot("before", &old).unwrap();
    let before = old.stats();
    let prior_manifest = ulmdb_storage::read_current(&dir.0).unwrap();
    let generation = env.upgrade_inventory().unwrap();
    let current_manifest = ulmdb_storage::read_current(&dir.0).unwrap();
    let after = env.stats();
    assert_ne!(generation, before.generation);
    assert_eq!(
        (after.sequence, after.commit),
        (before.sequence, before.commit)
    );
    assert_eq!(
        (current_manifest.epoch, current_manifest.uuid),
        (prior_manifest.epoch, prior_manifest.uuid)
    );
    assert_eq!(env.publication_mode(), PublicationMode::Indexed);
    assert_eq!(after.format_minor, 4);
    let pointer = fs::read(dir.0.join("CURRENT")).unwrap();
    assert_eq!(&pointer[16..24], b"ULMDBT04");
    assert_eq!(env.upgrade_inventory().unwrap(), generation);
    assert_eq!(env.upgrade_publication().unwrap(), generation);
    assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), pointer);
    for i in 0u64..40 {
        let mut write = env.begin_write().unwrap();
        write.put(b"kv", b"key", &i.to_le_bytes()).unwrap();
        {
            let mut child = write.savepoint().unwrap();
            child.put(b"kv", b"key", b"rollback").unwrap();
            child.rollback().unwrap();
        }
        write.commit().unwrap();
    }
    assert_eq!(borrowed, b"old");
    env.prune_inventory().unwrap();
    env.reclaim().unwrap();
    assert_eq!(
        env.open_retained("before")
            .unwrap()
            .get(b"kv", b"key")
            .unwrap(),
        Some(b"old".as_slice())
    );
    env.compact().unwrap();
    assert_eq!(env.publication_mode(), PublicationMode::Indexed);
    env.release_snapshot("before").unwrap();
    drop(old);
    env.reclaim().unwrap();
    drop(env);
    let reopened = Environment::open(&dir.0).unwrap();
    assert_eq!(reopened.publication_mode(), PublicationMode::Indexed);
    assert_eq!(
        reopened.begin_read().get(b"kv", b"key").unwrap(),
        Some(39u64.to_le_bytes().as_slice())
    );
    backup::verify(&reopened).unwrap();
}

#[test]
fn disk_and_ramdrive_upgrades_preserve_logical_state_and_old_readers() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for mode in [PublicationMode::Legacy, PublicationMode::Cohort] {
            exercise(&parent, mode);
        }
    }
}
