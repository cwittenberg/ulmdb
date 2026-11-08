//! Absolute segment IDs outlive any one bounded live lookup directory.
use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, backup};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-window-life-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn high_lifecycle(parent: &Path) {
    let directory = Directory::new(parent);
    ulmdb_storage::create_layout(&directory.0).unwrap();
    let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
    manifest.next_segment = (1u64 << 48) + 255;
    ulmdb_storage::install_snapshot(&directory.0, &manifest).unwrap();
    let env = Environment::open(&directory.0).unwrap();
    let prefix = Prefix::new(Address::V4(0xc0000200), 24).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    for n in 0..200u64 {
        w.put(b"kv", &n.to_be_bytes(), &n.to_be_bytes()).unwrap();
    }
    w.insert_prefix(b"v4", prefix, b"profile").unwrap();
    w.commit().unwrap();
    let old = env.begin_read();
    let borrowed = old.get(b"kv", &50u64.to_be_bytes()).unwrap().unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", &50u64.to_be_bytes(), b"updated!").unwrap();
    w.commit().unwrap();
    env.retain_snapshot("high-id", &old).unwrap();
    env.compact().unwrap();
    env.prune_inventory().unwrap();
    env.compact().unwrap();
    env.reclaim().unwrap();
    assert_eq!(borrowed, 50u64.to_be_bytes());
    assert_eq!(
        env.open_retained("high-id")
            .unwrap()
            .get(b"kv", &50u64.to_be_bytes())
            .unwrap(),
        Some(50u64.to_be_bytes().as_slice())
    );
    assert_eq!(
        env.begin_read()
            .longest_match(b"v4", Address::V4(0xc0000280))
            .unwrap()
            .map(|(_, v)| v),
        Some(b"profile".as_slice())
    );
    let exported = Directory::new(parent);
    backup::export(&env, &exported.0).unwrap();
    let restored = Directory::new(parent);
    backup::restore(&exported.0, &restored.0).unwrap();
    let restored_env = Environment::open(&restored.0).unwrap();
    assert_eq!(
        restored_env
            .begin_read()
            .get(b"kv", &50u64.to_be_bytes())
            .unwrap(),
        Some(b"updated!".as_slice())
    );
    assert!(
        ulmdb_storage::read_current(&restored.0)
            .unwrap()
            .segments
            .iter()
            .all(|s| s.id > 1 << 48)
    );
    env.release_snapshot("high-id").unwrap();
    drop(old);
    drop(env);
    let reopened = Environment::open(&directory.0).unwrap();
    assert_eq!(
        reopened
            .begin_read()
            .get(b"kv", &50u64.to_be_bytes())
            .unwrap(),
        Some(b"updated!".as_slice())
    );
    backup::verify(&reopened).unwrap();
}
#[test]
fn high_ids_reopen_maintain_export_and_restore_on_disk() {
    high_lifecycle(&std::env::temp_dir());
}
#[test]
fn high_ids_reopen_maintain_export_and_restore_on_ramdrive() {
    let parent = Path::new("/dev/shm");
    if parent.is_dir() {
        high_lifecycle(parent);
    }
}
#[test]
fn compaction_rebases_a_full_window_without_reusing_absolute_ids() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&directory.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.commit().unwrap();
    drop(env);
    let mut manifest = ulmdb_storage::read_current(&directory.0).unwrap();
    manifest.previous = manifest.generation;
    manifest.generation = ulmdb_storage::new_identity().unwrap();
    manifest.next_segment = 4097 * 256;
    ulmdb_storage::publish(&directory.0, &manifest).unwrap();
    let env = Environment::open(&directory.0).unwrap();
    let old = env.begin_read();
    let value = old.get(b"kv", b"key").unwrap().unwrap();
    let pointer = fs::read(directory.0.join("CURRENT")).unwrap();
    let count = fs::read_dir(directory.0.join("segments")).unwrap().count();
    assert!(matches!(env.begin_write(), Err(Error::LimitExceeded)));
    assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), pointer);
    assert_eq!(
        fs::read_dir(directory.0.join("segments")).unwrap().count(),
        count
    );
    env.compact().unwrap();
    let rebased = ulmdb_storage::read_current(&directory.0).unwrap();
    assert!(
        rebased
            .segments
            .iter()
            .all(|s| s.id >= manifest.next_segment)
    );
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"new").unwrap();
    w.commit().unwrap();
    assert_eq!(value, b"old");
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"new".as_slice())
    );
    backup::verify(&env).unwrap();
}
