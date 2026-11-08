use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb::{DatabaseKind, Environment, Error, backup};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-retention-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn initialize(directory: &Directory) -> Environment {
    let env = Environment::create(&directory.0).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"key", b"initial").unwrap();
    write.commit().unwrap();
    env
}
fn update(env: &Environment, value: &[u8]) {
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"key", value).unwrap();
    write.commit().unwrap();
}
fn segments(directory: &Directory) -> usize {
    fs::read_dir(directory.0.join("segments")).unwrap().count()
}
fn cycle(env: &Environment) {
    env.compact().unwrap();
    env.compact().unwrap();
    env.reclaim().unwrap();
}

fn retained_lifecycle(parent: &Path) {
    let directory = Directory::new(parent);
    let env = initialize(&directory);
    for i in 0..24u64 {
        update(&env, &i.to_be_bytes());
    }
    let pin = env.begin_read();
    let borrowed = pin.get(b"kv", b"key").unwrap().unwrap();
    let info = env.retain_snapshot("checkpoint-1", &pin).unwrap();
    assert_eq!(info.sequence, pin.sequence());
    assert!(env.retain_snapshot("checkpoint-1", &pin).is_err());
    for i in 24..48u64 {
        update(&env, &i.to_be_bytes());
    }
    let before = segments(&directory);
    cycle(&env);
    assert!(segments(&directory) < before);
    assert_eq!(borrowed, 23u64.to_be_bytes());
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(47u64.to_be_bytes().as_slice())
    );
    drop(pin);
    drop(env);
    let env = Environment::open(&directory.0).unwrap();
    assert_eq!(env.retained_snapshots().unwrap(), vec![info]);
    let retained = env.open_retained("checkpoint-1").unwrap();
    assert_eq!(
        retained.get(b"kv", b"key").unwrap(),
        Some(23u64.to_be_bytes().as_slice())
    );
    env.release_snapshot("checkpoint-1").unwrap();
    assert!(env.retained_snapshots().unwrap().is_empty());
    cycle(&env);
    assert!(
        segments(&directory) > 2,
        "opened reader must retain released pin data"
    );
    assert_eq!(
        retained.get(b"kv", b"key").unwrap(),
        Some(23u64.to_be_bytes().as_slice())
    );
    drop(retained);
    let report = env.reclaim().unwrap();
    assert!(report.removed_segments >= 24);
    assert_eq!(segments(&directory), 2);
    assert_eq!(
        fs::read_dir(directory.0.join("commits")).unwrap().count(),
        2
    );
    backup::verify(&env).unwrap();
}
#[test]
fn durable_pins_readers_compaction_and_reclaim_on_disk() {
    retained_lifecycle(&std::env::temp_dir());
}
#[test]
fn durable_pins_readers_compaction_and_reclaim_on_ramdrive() {
    retained_lifecycle(Path::new("/dev/shm"));
}

#[test]
fn corruption_and_stale_predecessor_stop_before_any_unlink() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = initialize(&directory);
    let old_pointer = fs::read(directory.0.join("CURRENT")).unwrap();
    let old = env.begin_read();
    env.retain_snapshot("pin", &old).unwrap();
    update(&env, b"next");
    update(&env, b"latest");
    let orphan = directory
        .0
        .join("segments")
        .join(format!("{}.seg", "a".repeat(64)));
    fs::write(&orphan, b"unreferenced").unwrap();
    let pin_path = directory.0.join("retained/pin.pin");
    let pin_bytes = fs::read(&pin_path).unwrap();
    fs::write(&pin_path, b"broken").unwrap();
    assert!(env.reclaim().is_err());
    assert!(orphan.exists());
    fs::write(&pin_path, pin_bytes).unwrap();
    let previous = fs::read(directory.0.join("PREVIOUS")).unwrap();
    fs::write(directory.0.join("PREVIOUS"), old_pointer).unwrap();
    assert!(matches!(env.reclaim(), Err(Error::Busy)));
    assert!(orphan.exists());
    fs::write(directory.0.join("PREVIOUS"), previous).unwrap();
    env.reclaim().unwrap();
    assert!(!orphan.exists());
    let current = fs::read(directory.0.join("CURRENT")).unwrap();
    fs::write(directory.0.join("CURRENT"), b"broken").unwrap();
    fs::write(&orphan, b"unreferenced").unwrap();
    assert!(env.reclaim().is_err());
    assert!(orphan.exists());
    fs::write(directory.0.join("CURRENT"), current).unwrap();
}

#[test]
fn concurrent_readers_exports_and_reclaim_keep_pinned_paths() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = Arc::new(initialize(&directory));
    let old = env.begin_read();
    for i in 0..40u64 {
        update(&env, &i.to_be_bytes());
    }
    cycle(&env);
    std::thread::scope(|scope| {
        let worker = Arc::clone(&env);
        scope.spawn(move || {
            for _ in 0..6 {
                cycle(&worker);
            }
        });
        for i in 0..6 {
            let target = Directory::new(&std::env::temp_dir());
            backup::export_snapshot(&env, &old, &target.0).unwrap();
            let copy = Environment::open(&target.0).unwrap();
            assert_eq!(
                copy.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"initial".as_slice()),
                "{i}"
            );
        }
    });
    assert_eq!(old.get(b"kv", b"key").unwrap(), Some(b"initial".as_slice()));
    drop(old);
    cycle(&env);
    assert_eq!(segments(&directory), 2);
}

#[test]
fn retention_names_owner_checks_and_restore_without_predecessor() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = initialize(&directory);
    let other_directory = Directory::new(&std::env::temp_dir());
    let other = initialize(&other_directory);
    for name in ["", "../escape", "/absolute", "a/b", "a.pin", "é"] {
        assert!(env.retain_snapshot(name, &env.begin_read()).is_err());
    }
    assert!(matches!(
        env.retain_snapshot("foreign", &other.begin_read()),
        Err(Error::StaleHandle)
    ));
    let exported = Directory::new(&std::env::temp_dir());
    backup::export(&env, &exported.0).unwrap();
    let copy = Environment::open(&exported.0).unwrap();
    copy.reclaim().unwrap();
    assert_eq!(
        copy.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"initial".as_slice())
    );
}

#[test]
fn acquiring_and_dropping_readers_races_with_publication_and_reclaim() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = Arc::new(initialize(&directory));
    update(&env, &0u64.to_be_bytes());
    let old = env.begin_read();
    let handle = old.open_database(b"kv").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let reader = Arc::clone(&env);
            let handle = handle.clone();
            scope.spawn(move || {
                for _ in 0..500 {
                    let read = reader.begin_read();
                    let value = read.get_handle(&handle, b"key").unwrap().unwrap();
                    assert_eq!(value.len(), 8);
                    let _ = u64::from_be_bytes(value.try_into().unwrap());
                    std::thread::yield_now();
                    assert_eq!(value, read.get_handle(&handle, b"key").unwrap().unwrap());
                }
            });
        }
        for n in 1..=80u64 {
            update(&env, &n.to_be_bytes());
            if n % 8 == 0 {
                cycle(&env);
            }
        }
    });
    assert_eq!(
        old.get_handle(&handle, b"key").unwrap(),
        Some(0u64.to_be_bytes().as_slice())
    );
    drop(old);
    cycle(&env);
    assert_eq!(segments(&directory), 2);
}
