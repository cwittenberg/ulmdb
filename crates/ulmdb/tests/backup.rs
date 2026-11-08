use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};
use ulmdb::{Address, DatabaseKind, Environment, Prefix, backup};

struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-backup-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn populate(env: &Environment) {
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    write.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    write
        .create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    write
        .create_database(b"fixed", DatabaseKind::Duplicates { width: Some(4) })
        .unwrap();
    write.put(b"kv", b"counter", &0u64.to_le_bytes()).unwrap();
    write.put(b"kv", b"mirror", &0u64.to_le_bytes()).unwrap();
    write.put(b"kv", b"large", &vec![42; 1_500_000]).unwrap();
    for n in 0..180u32 {
        write
            .put(b"kv", &n.to_be_bytes(), &n.to_be_bytes())
            .unwrap();
        write
            .insert_duplicate(b"dups", b"names", &n.to_be_bytes())
            .unwrap();
        write
            .insert_duplicate(b"fixed", b"addresses", &n.to_be_bytes())
            .unwrap();
    }
    write.insert_duplicate(b"dups", b"empty", b"").unwrap();
    write
        .insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"default")
        .unwrap();
    write
        .insert_prefix(
            b"v4",
            Prefix::new(Address::V4(0x0a00_0000), 8).unwrap(),
            b"network",
        )
        .unwrap();
    write
        .insert_prefix(
            b"v6",
            Prefix::new(Address::V6(0x20010db8u128 << 96), 32).unwrap(),
            b"six",
        )
        .unwrap();
    assert_eq!(write.commit().unwrap(), 1);
}
fn inspect(env: &Environment, expected_counter: Option<u64>) {
    let read = env.begin_read();
    let counter = read.get(b"kv", b"counter").unwrap().unwrap();
    assert_eq!(counter, read.get(b"kv", b"mirror").unwrap().unwrap());
    if let Some(expected) = expected_counter {
        assert_eq!(counter, &expected.to_le_bytes());
    }
    assert_eq!(
        read.get(b"kv", b"large").unwrap().unwrap(),
        &vec![42; 1_500_000]
    );
    assert_eq!(
        read.longest_match(b"v4", Address::V4(0x0a01_0001))
            .unwrap()
            .unwrap()
            .1,
        b"network"
    );
    assert_eq!(
        read.longest_match(b"v6", Address::V6((0x20010db8u128 << 96) | 1))
            .unwrap()
            .unwrap()
            .1,
        b"six"
    );
    assert_eq!(read.duplicate_count(b"dups", b"names").unwrap(), 180);
    assert_eq!(
        read.duplicate_first(b"dups", b"empty").unwrap(),
        Some(&b""[..])
    );
    assert_eq!(read.duplicate_count(b"fixed", b"addresses").unwrap(), 180);
}
fn exercise(parent: &Path) {
    let source = Directory::new(parent);
    let exported = Directory::new(parent);
    let restored = Directory::new(parent);
    let source_env = Environment::create(&source.0).unwrap();
    populate(&source_env);
    let before = ulmdb_storage::read_current(&source.0).unwrap();
    // Files outside the pinned inventory and uncommitted transaction junk must
    // never appear in an exported engine.
    fs::write(source.0.join("segments/unreferenced.seg"), b"junk").unwrap();
    fs::write(source.0.join("transactions/private"), b"private").unwrap();
    let summary = backup::export(&source_env, &exported.0).unwrap();
    assert_eq!(summary.uuid, before.uuid);
    assert_eq!(summary.commit, before.commit);
    assert_eq!(summary.generation, before.generation);
    assert_eq!(summary.sequence, 1);
    assert_eq!(summary.segments, before.segments.len());
    assert_eq!(
        fs::read_dir(exported.0.join("segments")).unwrap().count(),
        before.segments.len()
    );
    assert_eq!(fs::read_dir(exported.0.join("commits")).unwrap().count(), 1);
    assert_eq!(
        fs::read_dir(exported.0.join("transactions"))
            .unwrap()
            .count(),
        0
    );
    assert!(!exported.0.join("PREVIOUS").exists());
    let after = ulmdb_storage::read_current(&exported.0).unwrap();
    assert_eq!(after.parent, before.parent);
    assert_eq!(after.previous, before.previous);
    assert_eq!(
        fs::read(source.0.join("CURRENT")).unwrap(),
        fs::read(exported.0.join("CURRENT")).unwrap()
    );
    for (original, copied) in before.segments.iter().zip(&after.segments) {
        assert_eq!(
            (original.id, original.length, original.hash),
            (copied.id, copied.length, copied.hash)
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        for file in fs::read_dir(exported.0.join("segments")).unwrap() {
            let file = file.unwrap();
            assert_ne!(
                file.metadata().unwrap().ino(),
                fs::metadata(source.0.join("segments").join(file.file_name()))
                    .unwrap()
                    .ino()
            );
            assert_eq!(file.metadata().unwrap().nlink(), 1);
        }
    }
    let reopened = Environment::open(&exported.0).unwrap();
    inspect(&reopened, Some(0));
    drop(reopened);
    assert_eq!(backup::restore(&exported.0, &restored.0).unwrap(), summary);
    let restored_env = Environment::open(&restored.0).unwrap();
    inspect(&restored_env, Some(0));
    let mut write = restored_env.begin_write().unwrap();
    write.put(b"kv", b"counter", &99u64.to_le_bytes()).unwrap();
    write.put(b"kv", b"mirror", &99u64.to_le_bytes()).unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    inspect(&restored_env, Some(99));
    inspect(&source_env, Some(0));
    drop(restored_env);
    let reopened = Environment::open(&restored.0).unwrap();
    inspect(&reopened, Some(99));
}

#[test]
fn export_restore_identity_independent_files_and_reopen_on_disk() {
    exercise(&std::env::temp_dir());
}
#[test]
fn export_restore_on_tmpfs_when_available() {
    let tmpfs = Path::new("/dev/shm");
    if tmpfs.is_dir() {
        exercise(tmpfs);
    }
}

#[test]
fn concurrent_commits_and_exporting_an_older_snapshot_remain_consistent() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let current_target = Directory::new(&parent);
    let old_target = Directory::new(&parent);
    let env = Arc::new(Environment::create(&source.0).unwrap());
    populate(&env);
    let old = env.begin_read();
    let barrier = Arc::new(Barrier::new(2));
    let writer_env = env.clone();
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        writer_barrier.wait();
        for n in 1..=24u64 {
            let mut write = writer_env.begin_write().unwrap();
            write.put(b"kv", b"counter", &n.to_le_bytes()).unwrap();
            write.put(b"kv", b"mirror", &n.to_le_bytes()).unwrap();
            write.commit().unwrap();
        }
    });
    barrier.wait();
    let current = backup::export(&env, &current_target.0).unwrap();
    let old_summary = backup::export_snapshot(&env, &old, &old_target.0).unwrap();
    writer.join().unwrap();
    assert_eq!(old_summary.sequence, 1);
    assert_eq!(env.begin_read().sequence(), 25);
    assert!((1..=25).contains(&current.sequence));
    let exported = Environment::open(&current_target.0).unwrap();
    assert_eq!(exported.begin_read().sequence(), current.sequence);
    inspect(&exported, Some(current.sequence - 1));
    let exported_old = Environment::open(&old_target.0).unwrap();
    inspect(&exported_old, Some(0));
}

#[test]
fn rejects_existing_target_unrelated_snapshot_and_corrupted_export() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let unrelated = Directory::new(&parent);
    let target = Directory::new(&parent);
    let restored = Directory::new(&parent);
    let env = Environment::create(&source.0).unwrap();
    populate(&env);
    fs::create_dir(&target.0).unwrap();
    fs::write(target.0.join("marker"), b"preserve").unwrap();
    assert!(backup::export(&env, &target.0).is_err());
    assert_eq!(fs::read(target.0.join("marker")).unwrap(), b"preserve");
    let other_env = Environment::create(&unrelated.0).unwrap();
    assert!(backup::export_snapshot(&env, &other_env.begin_read(), &restored.0).is_err());
    assert!(!restored.0.exists());
    fs::remove_dir_all(&target.0).unwrap();
    backup::export(&env, &target.0).unwrap();
    let file = fs::read_dir(target.0.join("segments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o600);
    }
    fs::set_permissions(&file, permissions).unwrap();
    let mut corrupt = OpenOptions::new().write(true).open(file).unwrap();
    corrupt.seek(SeekFrom::Start(4096)).unwrap();
    corrupt.write_all(b"corrupt!").unwrap();
    corrupt.sync_all().unwrap();
    drop(corrupt);
    assert!(backup::restore(&target.0, &restored.0).is_err());
    assert!(!restored.0.join("CURRENT").exists());
}

#[test]
fn empty_initial_snapshot_can_export_restore_and_accept_writes() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let target = Directory::new(&parent);
    let restored = Directory::new(&parent);
    let env = Environment::create(&source.0).unwrap();
    let summary = backup::export(&env, &target.0).unwrap();
    assert_eq!(summary.sequence, 0);
    assert_eq!(summary.segment_bytes, 0);
    assert_eq!(backup::restore(&target.0, &restored.0).unwrap(), summary);
    let env = Environment::open(&restored.0).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"new", DatabaseKind::Kv).unwrap();
    write.put(b"new", b"key", b"value").unwrap();
    assert_eq!(write.commit().unwrap(), 1);
}

#[test]
fn restored_clone_reader_is_rejected_even_with_identical_persisted_identity() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let cloned = Directory::new(&parent);
    let rejected = Directory::new(&parent);
    let env = Environment::create(&source.0).unwrap();
    populate(&env);
    backup::export(&env, &cloned.0).unwrap();
    let clone = Environment::open(&cloned.0).unwrap();
    assert_eq!(
        ulmdb_storage::read_current(&source.0).unwrap().uuid,
        ulmdb_storage::read_current(&cloned.0).unwrap().uuid
    );
    assert!(backup::export_snapshot(&env, &clone.begin_read(), &rejected.0).is_err());
    assert!(!rejected.0.exists());
}

#[test]
fn damaged_pointer_descriptor_or_missing_segment_never_activates_restore() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let env = Environment::create(&source.0).unwrap();
    populate(&env);
    for damage in ["pointer", "descriptor", "missing segment"] {
        let exported = Directory::new(&parent);
        let rejected = Directory::new(&parent);
        backup::export(&env, &exported.0).unwrap();
        let path = match damage {
            "pointer" => exported.0.join("CURRENT"),
            "descriptor" => fs::read_dir(exported.0.join("commits"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
            _ => fs::read_dir(exported.0.join("segments"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        };
        if damage == "missing segment" {
            fs::remove_file(path).unwrap();
        } else {
            let mut bytes = fs::read(&path).unwrap();
            bytes[20] ^= 1;
            fs::write(path, bytes).unwrap();
        }
        assert!(
            backup::restore(&exported.0, &rejected.0).is_err(),
            "{damage}"
        );
        assert!(!rejected.0.join("CURRENT").exists());
    }
}

#[test]
fn exponentially_aliased_empty_subtrees_exhaust_validation_budget_before_activation() {
    use ulmdb_core::storage::{Reference, WriteStore};
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let rejected = Directory::new(&parent);
    ulmdb_storage::create_layout(&source.0).unwrap();
    let mut arena = ulmdb_storage::Arena::new(ulmdb_storage::SegmentTable::default(), 1).unwrap();
    let mut page = [0u8; 4096];
    page[..2].copy_from_slice(&2u16.to_le_bytes());
    page[2..4].copy_from_slice(&1u16.to_le_bytes());
    page[8..10].copy_from_slice(&32u16.to_le_bytes());
    page[10..12].copy_from_slice(&4096u16.to_le_bytes());
    let mut child = arena.allocate(&page, 4096).unwrap();
    for level in 1..=5u16 {
        let mut branch = [0u8; 4096];
        branch[..2].copy_from_slice(&3u16.to_le_bytes());
        branch[2..4].copy_from_slice(&1u16.to_le_bytes());
        branch[4..6].copy_from_slice(&level.to_le_bytes());
        branch[6..8].copy_from_slice(&128u16.to_le_bytes());
        branch[8..10].copy_from_slice(&288u16.to_le_bytes());
        branch[10..12].copy_from_slice(&1280u16.to_le_bytes());
        branch[16..32].copy_from_slice(&child.encode());
        for index in 0..128usize {
            let start = 1280 + index * 22;
            branch[32 + index * 2..34 + index * 2].copy_from_slice(&(start as u16).to_le_bytes());
            branch[start..start + 2].copy_from_slice(&1u16.to_le_bytes());
            branch[start + 4..start + 20].copy_from_slice(&child.encode());
            branch[start + 20] = index as u8;
        }
        child = arena.allocate(&branch, 4096).unwrap();
    }
    let mut descriptor = [0u8; 80];
    descriptor[..8].copy_from_slice(&1u64.to_le_bytes());
    descriptor[8..10].copy_from_slice(&1u16.to_le_bytes());
    descriptor[10..12].copy_from_slice(&1u16.to_le_bytes());
    descriptor[16..32].copy_from_slice(&child.encode());
    descriptor[32..40].copy_from_slice(&4096u64.to_le_bytes());
    descriptor[40..48].copy_from_slice(&67_108_864u64.to_le_bytes());
    descriptor[48..56].copy_from_slice(&65_536u64.to_le_bytes());
    descriptor[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    let catalog = ulmdb_kv::put(&mut arena, Reference::NULL, b"kv", &descriptor).unwrap();
    let (segments, _) = arena.seal(&source.0).unwrap();
    let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
    manifest.catalog = catalog;
    manifest.next_database = 2;
    manifest.next_segment = 2;
    manifest.segments = (segments.entries()).into();
    ulmdb_storage::install_snapshot(&source.0, &manifest).unwrap();
    let env = Environment::open(&source.0).unwrap();
    let error = backup::export(&env, &rejected.0).unwrap_err();
    assert!(matches!(error, ulmdb::Error::Corrupt(_)));
    assert!(!rejected.0.exists());
}
