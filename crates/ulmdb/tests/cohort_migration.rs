use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, backup};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-cohort-migration-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn initialize(dir: &Directory) -> Environment {
    let env = Environment::create(&dir.0).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"key", b"old").unwrap();
    write.commit().unwrap();
    env
}
fn exercise(parent: &Path) {
    let dir = Directory::new(parent);
    let env = initialize(&dir);
    assert_eq!(env.publication_mode(), PublicationMode::Legacy);
    // Simulate the directory structure of a pre-cohort installation.
    fs::remove_dir(dir.0.join("cohorts")).unwrap();
    let old = env.begin_read();
    let before = old.stats();
    let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
    env.retain_snapshot("legacy", &old).unwrap();
    let generation = env.upgrade_publication().unwrap();
    assert_ne!(generation, before.generation);
    assert_eq!(env.publication_mode(), PublicationMode::Cohort);
    let after = env.stats();
    assert_eq!(
        (after.sequence, after.commit, after.databases),
        (before.sequence, before.commit, before.databases)
    );
    assert_eq!(after.format_minor, 3);
    let pointer = fs::read(dir.0.join("CURRENT")).unwrap();
    assert_eq!(env.upgrade_publication().unwrap(), generation);
    assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), pointer);
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"key", b"new").unwrap();
    let mut child = write.savepoint().unwrap();
    child.put(b"kv", b"key", b"bad").unwrap();
    child.rollback().unwrap();
    drop(child);
    write.commit().unwrap();
    assert_eq!(borrowed, b"old");
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"new".as_slice())
    );
    env.compact().unwrap();
    env.prune_inventory().unwrap();
    env.reclaim().unwrap();
    assert_eq!(
        env.open_retained("legacy")
            .unwrap()
            .get(b"kv", b"key")
            .unwrap(),
        Some(b"old".as_slice())
    );
    env.release_snapshot("legacy").unwrap();
    drop(env);
    let reopened = Environment::open(&dir.0).unwrap();
    assert_eq!(reopened.publication_mode(), PublicationMode::Cohort);
    assert_eq!(
        reopened.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"new".as_slice())
    );
    assert_eq!(borrowed, b"old");
    backup::verify(&reopened).unwrap();
}
#[test]
fn migration_from_legacy_layout_preserves_snapshots_and_logical_identity_on_disk() {
    exercise(&std::env::temp_dir());
}
#[test]
fn migration_from_legacy_layout_preserves_snapshots_and_logical_identity_on_ramdrive() {
    if Path::new("/dev/shm").is_dir() {
        exercise(Path::new("/dev/shm"));
    }
}
#[test]
fn invalid_cohort_directory_prevents_migration_before_publication() {
    let dir = Directory::new(&std::env::temp_dir());
    let env = initialize(&dir);
    fs::remove_dir(dir.0.join("cohorts")).unwrap();
    fs::write(dir.0.join("cohorts"), b"blocked").unwrap();
    let before = fs::read(dir.0.join("CURRENT")).unwrap();
    assert!(matches!(env.upgrade_publication(), Err(Error::Corrupt(_))));
    assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), before);
    assert_eq!(env.publication_mode(), PublicationMode::Legacy);
    env.begin_write().unwrap().abort();
}
