use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, WriteTxn};

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-detached-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn environment(directory: &Directory, mode: PublicationMode) -> Environment {
    Environment::create_with_publication(&directory.0, mode).unwrap()
}

#[test]
fn detached_private_roots_survive_other_writes_without_resurrection_or_row_copies() {
    for mode in MODES {
        let directory = Directory::new();
        let env = environment(&directory, mode);
        let mut writer = env.begin_write().unwrap();
        writer.create_database(b"named", DatabaseKind::Kv).unwrap();
        writer.create_database(b"main", DatabaseKind::Kv).unwrap();
        let original = writer.open_database(b"named").unwrap();
        let large = vec![0x51; 131_072];
        writer.put_handle(&original, b"a", b"first").unwrap();
        writer.put_handle(&original, b"b", &large).unwrap();
        writer.put_handle(&original, b"c", b"last").unwrap();
        let private_bytes = writer.dirty_bytes();
        let retained = writer.detach_database_handle(&original).unwrap();
        assert_eq!(
            writer.dirty_bytes(),
            private_bytes,
            "detach copies no arena bytes"
        );
        assert_eq!(
            (retained.name(), retained.id(), retained.kind()),
            (b"named".as_slice(), original.id(), DatabaseKind::Kv)
        );
        assert!(matches!(
            writer.open_database(b"named"),
            Err(Error::MissingDatabase)
        ));
        assert!(matches!(
            writer.get_handle(retained.handle(), b"a"),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            writer.put_handle(retained.handle(), b"a", b"forbidden"),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            writer.delete_handle(retained.handle(), b"a"),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            writer.detach_database_handle(retained.handle()),
            Err(Error::StaleHandle)
        ));
        writer.create_database(b"named", DatabaseKind::Kv).unwrap();
        let replacement = writer.open_database(b"named").unwrap();
        assert_ne!(replacement.id(), retained.id());
        writer
            .put_handle(&replacement, b"a", b"replacement")
            .unwrap();
        for number in 0u32..256 {
            writer
                .put(b"main", &number.to_be_bytes(), &large[..2048])
                .unwrap();
        }
        assert_eq!(
            writer.detached_get(&retained, b"b").unwrap(),
            Some(large.as_slice())
        );
        assert_eq!(writer.detached_get(&retained, b"absent").unwrap(), None);
        assert_eq!(
            writer
                .detached_seek_pair_borrowed(&retained, None, true, false)
                .unwrap(),
            Some((b"a".as_slice(), b"first".as_slice()))
        );
        assert_eq!(
            writer
                .detached_seek_pair_borrowed(&retained, Some((b"b", None)), false, true)
                .unwrap(),
            Some((b"a".as_slice(), b"first".as_slice()))
        );
        assert_eq!(
            writer
                .detached_seek_pair_borrowed(&retained, None, true, true)
                .unwrap(),
            Some((b"c".as_slice(), b"last".as_slice()))
        );
        writer.commit().unwrap();
        let read = env.begin_read();
        assert_eq!(
            read.get(b"named", b"a").unwrap(),
            Some(b"replacement".as_slice())
        );
        assert_eq!(read.get(b"named", b"b").unwrap(), None);
        drop(env);
        let reopened = Environment::open(&directory.0).unwrap();
        assert_eq!(reopened.begin_read().get(b"named", b"b").unwrap(), None);
    }
}

#[test]
fn nested_abort_reuse_and_release_preserve_only_active_capability_identity() {
    for mode in MODES {
        let directory = Directory::new();
        let env = environment(&directory, mode);
        let mut writer = env.begin_write().unwrap();
        for name in [b"parent".as_slice(), b"same", b"nested"] {
            writer.create_database(name, DatabaseKind::Kv).unwrap();
            writer.put(name, b"key", b"original").unwrap();
        }
        let parent = writer
            .detach_database_handle(&writer.open_database(b"parent").unwrap())
            .unwrap();
        let same = writer.open_database(b"same").unwrap();
        let aborted = {
            let mut child = writer.savepoint().unwrap();
            child.put_handle(&same, b"key", b"modified").unwrap();
            let retained = child.detach_database_handle(&same).unwrap();
            assert_eq!(
                child.detached_get(&retained, b"key").unwrap(),
                Some(b"modified".as_slice())
            );
            assert_eq!(
                child.detached_get(&parent, b"key").unwrap(),
                Some(b"original".as_slice())
            );
            child.abort().unwrap();
            retained
        };
        assert_eq!(
            writer.get_handle(&same, b"key").unwrap(),
            Some(b"original".as_slice())
        );
        let reused = writer.detach_database_handle(&same).unwrap();
        assert_eq!(
            reused.id(),
            aborted.id(),
            "rollback restores the same database identity"
        );
        assert!(matches!(
            writer.detached_get(&aborted, b"key"),
            Err(Error::StaleHandle)
        ));
        assert_eq!(
            writer.detached_get(&reused, b"key").unwrap(),
            Some(b"original".as_slice())
        );
        let nested = writer.open_database(b"nested").unwrap();
        let revoked = {
            let mut outer = writer.savepoint().unwrap();
            let token = {
                let mut inner = outer.savepoint().unwrap();
                inner.put_handle(&nested, b"key", b"changed!").unwrap();
                let token = inner.detach_database_handle(&nested).unwrap();
                inner.release().unwrap();
                token
            };
            assert_eq!(
                outer.detached_get(&token, b"key").unwrap(),
                Some(b"changed!".as_slice())
            );
            outer.rollback().unwrap();
            assert!(matches!(
                outer.detached_get(&token, b"key"),
                Err(Error::StaleHandle)
            ));
            outer.release().unwrap();
            token
        };
        assert!(matches!(
            writer.detached_get(&revoked, b"key"),
            Err(Error::StaleHandle)
        ));
        let released = {
            let mut child = writer.savepoint().unwrap();
            let token = child.detach_database_handle(&nested).unwrap();
            child.release().unwrap();
            token
        };
        assert_eq!(
            writer.detached_get(&released, b"key").unwrap(),
            Some(b"original".as_slice())
        );
        assert_eq!(
            writer.detached_get(&parent, b"key").unwrap(),
            Some(b"original".as_slice())
        );
        writer.commit().unwrap();
        assert!(env.begin_read().database_names().unwrap().is_empty());
    }
}

#[test]
fn aborted_writer_capability_cannot_read_a_new_writer_with_the_same_database_id() {
    let directory = Directory::new();
    let env = environment(&directory, PublicationMode::Legacy);
    let mut seed = env.begin_write().unwrap();
    seed.create_database(b"named", DatabaseKind::Kv).unwrap();
    seed.put(b"named", b"key", b"persisted").unwrap();
    let handle = seed.open_database(b"named").unwrap();
    seed.commit().unwrap();
    let abandoned = {
        let mut writer = env.begin_write().unwrap();
        writer.put_handle(&handle, b"key", b"private!!").unwrap();
        let retained = writer.detach_database_handle(&handle).unwrap();
        writer.abort();
        retained
    };
    let mut writer = env.begin_write().unwrap();
    assert!(matches!(
        writer.detached_get(&abandoned, b"key"),
        Err(Error::StaleHandle)
    ));
    let fresh = writer.detach_database_handle(&handle).unwrap();
    assert_eq!(fresh.id(), abandoned.id());
    assert!(matches!(
        writer.detached_get(&abandoned, b"key"),
        Err(Error::StaleHandle)
    ));
    assert_eq!(
        writer.detached_get(&fresh, b"key").unwrap(),
        Some(b"persisted".as_slice())
    );
}

#[test]
fn detached_inline_and_tree_duplicates_support_borrowed_get_and_directional_pair_seeks() {
    for mode in MODES {
        let directory = Directory::new();
        let env = environment(&directory, mode);
        let mut writer = env.begin_write().unwrap();
        writer
            .create_database(b"dups", DatabaseKind::Duplicates { width: None })
            .unwrap();
        let handle = writer.open_database(b"dups").unwrap();
        for value in [b"a".as_slice(), b"b", b"c"] {
            writer
                .insert_duplicate_handle(&handle, b"inline", value)
                .unwrap();
        }
        for number in 0u32..256 {
            let mut value = vec![0x62; 256];
            value[..4].copy_from_slice(&number.to_be_bytes());
            writer
                .insert_duplicate_handle(&handle, b"tree", &value)
                .unwrap();
        }
        let bytes = writer.dirty_bytes();
        let retained = writer.detach_database_handle(&handle).unwrap();
        assert_eq!(writer.dirty_bytes(), bytes);
        assert_eq!(
            writer.detached_get(&retained, b"inline").unwrap(),
            Some(b"a".as_slice())
        );
        assert_eq!(writer.detached_get(&retained, b"missing").unwrap(), None);
        assert_eq!(
            writer
                .detached_seek_pair_borrowed(&retained, Some((b"inline", Some(b"b"))), false, false)
                .unwrap(),
            Some((b"inline".as_slice(), b"c".as_slice()))
        );
        assert_eq!(
            writer
                .detached_seek_pair_borrowed(&retained, Some((b"inline", Some(b"b"))), false, true)
                .unwrap(),
            Some((b"inline".as_slice(), b"a".as_slice()))
        );
        let first = writer.detached_get(&retained, b"tree").unwrap().unwrap();
        assert_eq!(&first[..4], &0u32.to_be_bytes());
        let last = writer
            .detached_seek_pair_borrowed(&retained, None, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(last.0, b"tree");
        assert_eq!(&last.1[..4], &255u32.to_be_bytes());
        assert!(matches!(
            writer.insert_duplicate_handle(retained.handle(), b"inline", b"d"),
            Err(Error::StaleHandle)
        ));
        writer.commit().unwrap();
        assert!(matches!(
            env.begin_read().open_database(b"dups"),
            Err(Error::MissingDatabase)
        ));
    }
}

#[test]
fn detached_budget_rejects_before_removing_the_live_database() {
    let directory = Directory::new();
    let env = environment(&directory, PublicationMode::Legacy);
    let mut writer = env.begin_write().unwrap();
    for number in 0..WriteTxn::MAX_DETACHED_DATABASES {
        let name = number.to_be_bytes();
        writer.create_database(&name, DatabaseKind::Kv).unwrap();
        let handle = writer.open_database(&name).unwrap();
        writer.detach_database_handle(&handle).unwrap();
    }
    writer
        .create_database(b"still-live", DatabaseKind::Kv)
        .unwrap();
    writer.put(b"still-live", b"key", b"preserved").unwrap();
    let handle = writer.open_database(b"still-live").unwrap();
    let bytes = writer.dirty_bytes();
    assert!(matches!(
        writer.detach_database_handle(&handle),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(writer.dirty_bytes(), bytes);
    assert_eq!(
        writer.get_handle(&handle, b"key").unwrap(),
        Some(b"preserved".as_slice())
    );
    writer.commit().unwrap();
    assert_eq!(
        env.begin_read().get(b"still-live", b"key").unwrap(),
        Some(b"preserved".as_slice())
    );
}

#[test]
fn foreign_handles_wrong_kinds_and_failed_writers_do_not_gain_read_or_write_authority() {
    let directory = Directory::new();
    let other_directory = Directory::new();
    let env = environment(&directory, PublicationMode::Legacy);
    let other = environment(&other_directory, PublicationMode::Legacy);
    let mut writer = env.begin_write().unwrap();
    let mut foreign = other.begin_write().unwrap();
    writer.create_database(b"named", DatabaseKind::Kv).unwrap();
    foreign.create_database(b"named", DatabaseKind::Kv).unwrap();
    let foreign_handle = foreign.open_database(b"named").unwrap();
    assert!(matches!(
        writer.detach_database_handle(&foreign_handle),
        Err(Error::StaleHandle)
    ));
    let local_handle = writer.open_database(b"named").unwrap();
    let retained = writer.detach_database_handle(&local_handle).unwrap();
    assert!(matches!(
        foreign.detached_get(&retained, b"key"),
        Err(Error::StaleHandle)
    ));
    writer
        .create_database(b"prefix", DatabaseKind::Ipv4)
        .unwrap();
    let prefix = writer
        .detach_database_handle(&writer.open_database(b"prefix").unwrap())
        .unwrap();
    assert!(matches!(
        writer.detached_get(&prefix, b"key"),
        Err(Error::WrongKind)
    ));
    assert!(matches!(
        writer.detached_seek_pair_borrowed(&prefix, None, true, false),
        Err(Error::WrongKind)
    ));
    assert!(matches!(
        writer.detached_get(&retained, &vec![0; 4097]),
        Err(Error::InvalidInput(_))
    ));
    writer.create_database(b"live", DatabaseKind::Kv).unwrap();
    // A native mutation failure poisons the writer; detached capabilities must
    // honor the same liveness fence as every ordinary native reader operation.
    assert!(matches!(
        writer.insert_duplicate(b"live", b"key", b"value"),
        Err(Error::WrongKind)
    ));
    assert!(matches!(
        writer.detached_get(&retained, b"key"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        writer.detached_seek_pair_borrowed(&retained, None, true, false),
        Err(Error::InvalidInput(_))
    ));
}
