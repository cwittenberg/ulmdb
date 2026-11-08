use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix};

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-handles-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        ));
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn old_snapshot_handles_never_rebind_after_drop_recreate_or_reopen() {
    let directory = Directory::new();
    let env = Environment::create(&directory.0).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    let original = write.open_database(b"kv").unwrap();
    write.put_handle(&original, b"key", b"original").unwrap();
    write.commit().unwrap();
    let old = env.begin_read();
    let stats = old.stats();
    assert_eq!(stats.databases, 1);
    assert_eq!(stats.kv_databases, 1);
    assert_eq!(stats.format_minor, 2);
    let mut write = env.begin_write().unwrap();
    write.drop_database_handle(&original).unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    let replacement = write.open_database(b"kv").unwrap();
    assert_ne!(original.id(), replacement.id());
    assert!(matches!(
        write.put_handle(&original, b"key", b"wrong"),
        Err(Error::StaleHandle)
    ));
    write
        .put_handle(&replacement, b"key", b"replacement")
        .unwrap();
    write.commit().unwrap();
    assert_eq!(
        old.get_handle(&original, b"key").unwrap(),
        Some(b"original".as_slice())
    );
    assert!(matches!(
        old.kv_handle(&replacement),
        Err(Error::StaleHandle)
    ));
    let current = env.begin_read();
    assert!(matches!(
        current.get_handle(&original, b"key"),
        Err(Error::StaleHandle)
    ));
    assert_eq!(
        current.get_handle(&replacement, b"key").unwrap(),
        Some(b"replacement".as_slice())
    );
    assert_eq!(old.stats(), stats);
    assert_eq!(current.stats().sequence, stats.sequence + 1);
    assert!(current.stats().segment_bytes > stats.segment_bytes);
    drop(env);
    let reopened = Environment::open(&directory.0).unwrap();
    assert!(matches!(
        reopened.begin_read().kv_handle(&replacement),
        Err(Error::StaleHandle)
    ));
    let fresh = reopened.begin_read().open_database(b"kv").unwrap();
    assert_eq!(fresh.id(), replacement.id());
    // Old mappings/handles still work through their originating pinned reader.
    assert_eq!(
        old.get_handle(&original, b"key").unwrap(),
        Some(b"original".as_slice())
    );
}

#[test]
fn provisional_ids_are_burned_across_aborted_writers_and_reusable_savepoints() {
    let directory = Directory::new();
    let env = Environment::create(&directory.0).unwrap();
    let mut aborted = env.begin_write().unwrap();
    aborted.create_database(b"kv", DatabaseKind::Kv).unwrap();
    let abandoned = aborted.open_database(b"kv").unwrap();
    aborted.abort();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    let current = write.open_database(b"kv").unwrap();
    assert!(current.id() > abandoned.id());
    assert!(matches!(
        write.get_handle(&abandoned, b"key"),
        Err(Error::StaleHandle)
    ));
    let removed = {
        let mut scope = write.savepoint().unwrap();
        scope
            .create_database(b"temporary", DatabaseKind::Kv)
            .unwrap();
        let handle = scope.open_database(b"temporary").unwrap();
        scope.rollback().unwrap();
        scope
            .create_database(b"temporary", DatabaseKind::Kv)
            .unwrap();
        assert!(scope.open_database(b"temporary").unwrap().id() > handle.id());
        assert!(matches!(
            scope.get_handle(&handle, b"key"),
            Err(Error::StaleHandle)
        ));
        scope.release().unwrap();
        handle
    };
    write.commit().unwrap();
    assert!(matches!(
        env.begin_read().kv_handle(&removed),
        Err(Error::StaleHandle)
    ));
    assert!(env.begin_read().open_database(b"temporary").unwrap().id() > removed.id());
}

#[test]
fn typed_handles_reject_foreign_environments_and_wrong_kinds() {
    let a = Directory::new();
    let b = Directory::new();
    let env = Environment::create(&a.0).unwrap();
    let other = Environment::create(&b.0).unwrap();
    let prefix = Prefix::new(Address::V4(0x0a00_0000), 8).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    write
        .create_database(b"duplicates", DatabaseKind::Duplicates { width: Some(8) })
        .unwrap();
    let ip = write.open_database(b"ip").unwrap();
    let duplicates = write.open_database(b"duplicates").unwrap();
    write.insert_prefix_handle(&ip, prefix, b"profile").unwrap();
    write
        .insert_duplicate_handle(&duplicates, b"host", b"12345678")
        .unwrap();
    write.commit().unwrap();
    assert!(matches!(
        other.begin_read().kv_handle(&ip),
        Err(Error::StaleHandle)
    ));
    let read = env.begin_read();
    assert!(matches!(read.kv_handle(&ip), Err(Error::WrongKind)));
    assert_eq!(
        read.prefix_handle(&ip).unwrap().exact(prefix).unwrap(),
        Some((prefix, b"profile".as_slice()))
    );
    assert!(
        read.duplicates_handle(&duplicates)
            .unwrap()
            .contains(b"host", b"12345678")
            .unwrap()
    );
    assert_eq!(ip.name(), b"ip");
    assert_eq!(ip.kind(), DatabaseKind::Ipv4);
}
