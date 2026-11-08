use std::fs;
use ulmdb::{Address, DatabaseKind, Environment, PageSize, Prefix, PublicationMode};
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-geometry-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn persisted_geometry_all_file_backends_and_tree_kinds() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for size in [256, 1024, 4096, 8192, 16384, 65536] {
            let dir = Dir::new();
            let export = Dir::new();
            let env =
                Environment::create_with_page_size(&dir.0, mode, PageSize::new(size).unwrap())
                    .unwrap();
            let mut w = env.begin_write().unwrap();
            for (name, kind) in [
                (b"kv".as_slice(), DatabaseKind::Kv),
                (b"dup", DatabaseKind::Duplicates { width: Some(4) }),
                (b"ip", DatabaseKind::Ipv4),
            ] {
                w.create_database(name, kind).unwrap();
            }
            for n in 0..320u32 {
                w.put(b"kv", &n.to_be_bytes(), &vec![(n % 251) as u8; 90])
                    .unwrap();
                w.insert_duplicate(b"dup", b"key", &n.to_be_bytes())
                    .unwrap();
            }
            w.insert_prefix(
                b"ip",
                Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
                b"ten",
            )
            .unwrap();
            if size >= 16384 {
                let key = vec![7; PageSize::new(size).unwrap().lmdb_key_limit()];
                w.put(b"kv", &key, b"long").unwrap();
                w.insert_duplicate(b"dup", &key, b"long").unwrap();
            }
            w.commit().unwrap();
            let old = env.begin_read();
            assert_eq!(old.file_manifest().unwrap().page_size.get(), size);
            assert_eq!(old.duplicate_count(b"dup", b"key").unwrap(), 320);
            let mut w = env.begin_write().unwrap();
            for n in 0..160u32 {
                w.delete(b"kv", &n.to_be_bytes()).unwrap();
            }
            w.commit().unwrap();
            assert_eq!(
                old.get(b"kv", &0u32.to_be_bytes()).unwrap(),
                Some(vec![0; 90].as_slice())
            );
            env.compact().unwrap();
            ulmdb::backup::export(&env, &export.0).unwrap();
            drop(old);
            drop(env);
            for path in [&dir.0, &export.0] {
                let e = Environment::open(path).unwrap();
                let r = e.begin_read();
                assert_eq!(r.file_manifest().unwrap().page_size.get(), size);
                assert_eq!(
                    r.range(
                        b"kv",
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Unbounded,
                        false
                    )
                    .unwrap()
                    .count(),
                    if size >= 16384 { 161 } else { 160 }
                );
                assert_eq!(r.duplicate_count(b"dup", b"key").unwrap(), 320);
                assert_eq!(
                    r.longest_match(b"ip", Address::V4(0x0a010101))
                        .unwrap()
                        .unwrap()
                        .1,
                    b"ten"
                );
            }
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn copies_previous_rollback_and_authenticated_replication_keep_geometry() {
    use ulmdb::{
        OwnedFileCapture, ReadTxn, ResidencyPolicy, backup,
        replication::{self, Cursor, PublisherKey, TransferLimits},
    };
    use ulmdb_storage::coordination::Coordination;
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let dir = Dir::new();
        let raw = Dir::new();
        let compact = Dir::new();
        let empty = Dir::new();
        let replica = Dir::new();
        let stage = Dir::new();
        fs::create_dir(&stage.0).unwrap();
        let size = PageSize::new(16384).unwrap();
        let env = Environment::create_with_page_size(&dir.0, mode, size).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"k", b"A").unwrap();
        w.commit().unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"k", b"B").unwrap();
        w.commit().unwrap();
        let b = env.begin_read();
        let pair = OwnedFileCapture::current_for_copy(
            &dir.0,
            Coordination::capture_directory(&dir.0, Default::default()).unwrap(),
        )
        .unwrap()
        .into_copy_pair()
        .unwrap()
        .admit(ResidencyPolicy::Demand)
        .unwrap();
        backup::export_file_pair(&pair, &raw.0).unwrap();
        let view = OwnedFileCapture::current_for_copy(
            &dir.0,
            Coordination::capture_directory(&dir.0, Default::default()).unwrap(),
        )
        .unwrap()
        .into_copy_pair()
        .unwrap()
        .admit_selected_view(ResidencyPolicy::Demand)
        .unwrap();
        backup::compact_file_copy(&view, &compact.0).unwrap();
        backup::create_empty_file_copy_with_page_size(&empty.0, mode, size).unwrap();
        for path in [&raw.0, &compact.0, &empty.0] {
            let e = Environment::open(path).unwrap();
            assert_eq!(e.begin_read().file_manifest().unwrap().page_size, size);
        }
        let prev =
            ReadTxn::open_previous_coordinated_snapshot(&raw.0, ResidencyPolicy::Demand).unwrap();
        assert_eq!(prev.get(b"kv", b"k").unwrap(), Some(b"A".as_slice()));
        let c = Cursor::from_read(&b).unwrap();
        let key = PublisherKey::new(7, c.uuid, c.epoch, [42; 32]).unwrap();
        let mut bytes = vec![];
        replication::send_snapshot(&env, &b, None, &key, &mut bytes).unwrap();
        let frame = replication::receive(
            &mut std::io::Cursor::new(bytes),
            &key,
            TransferLimits::default(),
            &stage.0,
        )
        .unwrap();
        let r = replication::bootstrap(&frame, &replica.0).unwrap();
        assert_eq!(r.begin_read().file_manifest().unwrap().page_size, size);
        assert_eq!(
            r.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"B".as_slice())
        );
        env.rollback_last_commit(2).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"A".as_slice())
        );
        assert_eq!(b.get(b"kv", b"k").unwrap(), Some(b"B".as_slice()));
    }
}

#[test]
fn invalid_geometry_and_hash_valid_wrong_width_are_rejected() {
    for n in [0, 1, 128, 255, 257, 4095, 65537, 131072] {
        assert!(PageSize::new(n).is_err());
    }
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let dir = Dir::new();
        let env =
            Environment::create_with_page_size(&dir.0, mode, PageSize::new(8192).unwrap()).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"key", b"value").unwrap();
        w.commit().unwrap();
        drop(env);
        // A newly authenticated descriptor cannot relabel existing page bytes.
        let mut wrong = ulmdb_storage::read_current(&dir.0).unwrap();
        wrong.previous = wrong.generation;
        wrong.generation = ulmdb_storage::new_identity().unwrap();
        wrong.page_size = PageSize::new(1024).unwrap();
        if wrong.format_minor == 4 {
            wrong.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::publish(&dir.0, &wrong).unwrap();
        assert!(matches!(
            Environment::open(&dir.0),
            Err(ulmdb::Error::Corrupt(_))
        ));
    }
    let dir = Dir::new();
    assert!(
        Environment::create_with_page_size(
            &dir.0,
            PublicationMode::Range,
            PageSize::new(8192).unwrap()
        )
        .is_err()
    );
    assert!(!dir.0.exists());
}
