use std::{fs, path::PathBuf};
use ulmdb::{
    Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ReadTxn, RollbackError,
    backup,
};

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-rollback-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn prefix() -> Prefix {
    Prefix::new(Address::V4(0xc000_0200), 24).unwrap()
}
fn prefix6() -> Prefix {
    Prefix::new(Address::V6(0x2001_0db8_0000_0000_0000_0000_0000_0000), 32).unwrap()
}
fn seed(path: &std::path::Path, mode: PublicationMode) -> Environment {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"ip6", DatabaseKind::Ipv6).unwrap();
    w.create_database(b"original", DatabaseKind::Kv).unwrap();
    w.put(b"original", b"key", b"original").unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.insert_duplicate(b"dups", b"key", b"a").unwrap();
    w.insert_duplicate(b"dups", b"key", b"b").unwrap();
    w.insert_prefix(b"ip", prefix(), b"old").unwrap();
    w.insert_prefix(b"ip6", prefix6(), b"old").unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    env
}
fn update(env: &Environment) -> ulmdb::DatabaseHandle {
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"new").unwrap();
    w.delete_duplicates(b"dups", b"key").unwrap();
    w.insert_duplicate(b"dups", b"key", b"new").unwrap();
    w.insert_prefix(b"ip", prefix(), b"new").unwrap();
    w.insert_prefix(b"ip6", prefix6(), b"new").unwrap();
    w.drop_database(b"original").unwrap();
    w.create_database(b"new", DatabaseKind::Kv).unwrap();
    let handle = w.open_database(b"new").unwrap();
    w.put_handle(&handle, b"key", b"new").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
    handle
}
fn inspect(read: &ReadTxn, newer: bool) {
    let value: &[u8] = if newer { b"new" } else { b"old" };
    assert_eq!(read.get(b"kv", b"key").unwrap(), Some(value));
    assert_eq!(
        read.exact_prefix(b"ip", prefix()).unwrap(),
        Some((prefix(), value))
    );
    assert_eq!(
        read.exact_prefix(b"ip6", prefix6()).unwrap(),
        Some((prefix6(), value))
    );
    let d = read.duplicates(b"dups").unwrap();
    assert_eq!(d.count(b"key").unwrap(), if newer { 1 } else { 2 });
    assert!(
        d.contains(b"key", if newer { b"new" } else { b"a" })
            .unwrap()
    );
    assert_eq!(
        read.database_names()
            .unwrap()
            .contains(&b"original".to_vec()),
        !newer
    );
    assert_eq!(
        read.database_names().unwrap().contains(&b"new".to_vec()),
        newer
    );
}

#[test]
fn restores_exact_logical_identity_and_all_kinds_without_rewriting_history() {
    for mode in MODES {
        let dir = Directory::new();
        let env = seed(&dir.0, mode);
        let a = ulmdb_storage::read_current(&dir.0).unwrap();
        let a_bytes = fs::read(ulmdb_storage::descriptor_path(&dir.0, &a)).unwrap();
        let old = env.begin_read();
        let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
        let original = old.open_database(b"original").unwrap();
        let undone = update(&env);
        let b = ulmdb_storage::read_current(&dir.0).unwrap();
        let b_bytes = fs::read(ulmdb_storage::descriptor_path(&dir.0, &b)).unwrap();
        let current_reader = env.begin_read();
        let independent = ReadTxn::open_coordinated_snapshot(&dir.0).unwrap();
        env.retain_snapshot("undone", &current_reader).unwrap();
        let report = env.rollback_last_commit(2).unwrap();
        assert_eq!(report.sequence, a.sequence);
        assert_eq!(report.commit, a.commit);
        assert_eq!(report.undone_commit, b.commit);
        assert_eq!(report.undone_generation, b.generation);
        assert_ne!(report.generation, a.generation);
        assert_ne!(report.generation, b.generation);
        let r = ulmdb_storage::read_current(&dir.0).unwrap();
        assert_eq!(r.previous, 0);
        assert_eq!(r.parent, a.parent);
        assert_eq!(r.catalog, a.catalog);
        assert_eq!(r.next_database, b.next_database.max(a.next_database));
        assert!(ulmdb_storage::read_previous(&dir.0).unwrap().is_none());
        assert_eq!(
            fs::read(ulmdb_storage::descriptor_path(&dir.0, &a)).unwrap(),
            a_bytes
        );
        assert_eq!(
            fs::read(ulmdb_storage::descriptor_path(&dir.0, &b)).unwrap(),
            b_bytes
        );
        inspect(&env.begin_read(), false);
        inspect(&old, false);
        inspect(&current_reader, true);
        inspect(&independent, true);
        assert_eq!(borrowed, b"old");
        assert!(matches!(
            env.begin_read().get_handle(&undone, b"key"),
            Err(Error::StaleHandle)
        ));
        assert_eq!(
            env.begin_read().get_handle(&original, b"key").unwrap(),
            Some(b"original".as_slice())
        );
        env.reclaim().unwrap();
        inspect(&current_reader, true);
        inspect(&independent, true);
        inspect(&env.open_retained("undone").unwrap(), true);
        let exported = Directory::new();
        backup::export(&env, &exported.0).unwrap();
        let copied = Environment::open(&exported.0).unwrap();
        inspect(&copied.begin_read(), false);
        assert_eq!(copied.begin_read().sequence(), 1);
        let old_export = Directory::new();
        backup::export_snapshot(&env, &current_reader, &old_export.0).unwrap();
        inspect(
            &Environment::open(&old_export.0).unwrap().begin_read(),
            true,
        );
        drop(env);
        let reopened = Environment::open(&dir.0).unwrap();
        inspect(&reopened.begin_read(), false);
        assert_eq!(reopened.begin_read().sequence(), 1);
        assert!(matches!(
            reopened.rollback_last_commit(1),
            Err(RollbackError::CannotRollback)
        ));
    }
}

#[test]
fn eligibility_is_business_history_and_later_sequence_reuse_preserves_handle_authority() {
    for mode in MODES {
        let dir = Directory::new();
        let empty = Environment::create_with_publication(&dir.0, mode).unwrap();
        assert!(matches!(
            empty.rollback_last_commit(0),
            Err(RollbackError::CannotRollback)
        ));
        drop(empty);
        fs::remove_dir_all(&dir.0).unwrap();
        let env = seed(&dir.0, mode);
        assert!(matches!(
            env.rollback_last_commit(1),
            Err(RollbackError::CannotRollback)
        ));
        let undone = update(&env);
        let head = fs::read(dir.0.join("CURRENT")).unwrap();
        for bad in [0, 1, 3, u64::MAX] {
            assert!(matches!(
                env.rollback_last_commit(bad),
                Err(RollbackError::CannotRollback)
            ));
            assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), head);
        }
        env.rollback_last_commit(2).unwrap();
        assert!(matches!(
            env.rollback_last_commit(1),
            Err(RollbackError::CannotRollback)
        ));
        let mut w = env.begin_write().unwrap();
        w.create_database(b"new", DatabaseKind::Kv).unwrap();
        let replacement = w.open_database(b"new").unwrap();
        assert!(replacement.id() > undone.id());
        assert!(matches!(
            w.put_handle(&undone, b"key", b"forbidden"),
            Err(Error::StaleHandle)
        ));
        w.put_handle(&replacement, b"key", b"later").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        assert!(matches!(
            env.begin_read().get_handle(&undone, b"key"),
            Err(Error::StaleHandle)
        ));
        assert_eq!(env.rollback_last_commit(2).unwrap().sequence, 1);
        assert!(matches!(
            env.begin_read().get_handle(&replacement, b"key"),
            Err(Error::StaleHandle)
        ));
    }
}

#[test]
fn maintenance_root_is_not_an_eligible_business_commit() {
    for mode in MODES {
        let dir = Directory::new();
        let env = seed(&dir.0, mode);
        update(&env);
        env.compact().unwrap();
        let head = fs::read(dir.0.join("CURRENT")).unwrap();
        assert!(matches!(
            env.rollback_last_commit(2),
            Err(RollbackError::CannotRollback)
        ));
        assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), head);
        inspect(&env.begin_read(), true);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"later").unwrap();
        assert_eq!(w.commit().unwrap(), 3);
        assert_eq!(env.rollback_last_commit(3).unwrap().sequence, 2);
        inspect(&env.begin_read(), true);
    }
    let dir = Directory::new();
    let env = seed(&dir.0, PublicationMode::Legacy);
    update(&env);
    env.upgrade_inventory().unwrap();
    assert!(matches!(
        env.rollback_last_commit(2),
        Err(RollbackError::CannotRollback)
    ));
    inspect(&env.begin_read(), true);
}

#[test]
fn range_is_explicitly_unsupported_and_existing_native_lock_excludes_another_writer_owner() {
    for mode in [PublicationMode::RangeV5]
        .into_iter()
        .chain(cfg!(feature = "range-lifecycle").then_some(PublicationMode::Range))
    {
        let dir = Directory::new();
        let env = Environment::create_with_publication(&dir.0, mode).unwrap();
        assert!(matches!(
            env.rollback_last_commit(0),
            Err(RollbackError::Engine(Error::InvalidInput(_)))
        ));
        assert!(matches!(
            env.raise_database_id_floor(2),
            Err(Error::InvalidInput(_))
        ));
    }
    for mode in MODES {
        let dir = Directory::new();
        let env = seed(&dir.0, mode);
        assert!(matches!(Environment::open(&dir.0), Err(Error::Busy)));
        update(&env);
        env.rollback_last_commit(2).unwrap();
        assert!(matches!(Environment::open(&dir.0), Err(Error::Busy)));
    }
}

#[test]
fn replicas_and_historyless_export_are_rejected_without_selecting_new_roots() {
    use ulmdb::replication::{self, Cursor, PublisherKey, TransferLimits};
    let dir = Directory::new();
    let env = seed(&dir.0, PublicationMode::Indexed);
    update(&env);
    let read = env.begin_read();
    let cursor = Cursor::from_read(&read).unwrap();
    let key = PublisherKey::new(7, cursor.uuid, cursor.epoch, [0x37; 32]).unwrap();
    let mut wire = Vec::new();
    replication::send_snapshot(&env, &read, None, &key, &mut wire).unwrap();
    let stage = Directory::new();
    fs::create_dir(&stage.0).unwrap();
    let frame = replication::receive(
        &mut std::io::Cursor::new(wire),
        &key,
        TransferLimits::default(),
        &stage.0,
    )
    .unwrap();
    let replica_path = Directory::new();
    let replica = replication::bootstrap(&frame, &replica_path.0).unwrap();
    let head = fs::read(replica_path.0.join("CURRENT")).unwrap();
    assert!(matches!(
        replica.rollback_last_commit(2),
        Err(RollbackError::Engine(Error::ReadOnly))
    ));
    assert!(matches!(
        replica.raise_database_id_floor(100),
        Err(Error::ReadOnly)
    ));
    assert_eq!(fs::read(replica_path.0.join("CURRENT")).unwrap(), head);
    let exported = Directory::new();
    backup::export(&env, &exported.0).unwrap();
    let copy = Environment::open(&exported.0).unwrap();
    let head = fs::read(exported.0.join("CURRENT")).unwrap();
    assert!(matches!(
        copy.rollback_last_commit(2),
        Err(RollbackError::CannotRollback)
    ));
    assert_eq!(fs::read(exported.0.join("CURRENT")).unwrap(), head);
}

#[test]
fn malformed_legacy_predecessor_is_not_selected_and_head_drift_poison_precedes_its_error() {
    let dir = Directory::new();
    let env = seed(&dir.0, PublicationMode::Legacy);
    update(&env);
    let old = ulmdb_storage::read_previous(&dir.0).unwrap().unwrap();
    let path = ulmdb_storage::descriptor_path(&dir.0, &old);
    let original = fs::read(&path).unwrap();
    let mut invalid = original.clone();
    invalid[20] ^= 1;
    fs::write(&path, &invalid).unwrap();
    let head = fs::read(dir.0.join("CURRENT")).unwrap();
    assert!(matches!(
        env.rollback_last_commit(2),
        Err(RollbackError::Engine(Error::Corrupt(_)))
    ));
    assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), head);
    env.begin_write().unwrap().abort();
    fs::write(&path, original).unwrap();
    // Deliberately simulate a broken native-LOCK contract at the raw storage
    // layer. A fully authenticated but changed selected head must poison before
    // decoding the independently damaged predecessor, as in reclaim.
    let b = ulmdb_storage::read_current(&dir.0).unwrap();
    let mut newer = b.clone();
    newer.previous = b.generation;
    newer.parent = b.commit;
    newer.sequence += 1;
    newer.generation = ulmdb_storage::new_identity().unwrap();
    newer.commit = newer.generation;
    ulmdb_storage::publish(&dir.0, &newer).unwrap();
    let b_path = ulmdb_storage::descriptor_path(&dir.0, &b);
    let mut invalid = fs::read(&b_path).unwrap();
    invalid[20] ^= 1;
    fs::write(b_path, invalid).unwrap();
    assert!(matches!(
        env.rollback_last_commit(2),
        Err(RollbackError::Engine(Error::OutcomeUnknown))
    ));
    assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
    assert!(matches!(
        env.raise_database_id_floor(100),
        Err(Error::OutcomeUnknown)
    ));
}

#[test]
fn rollback_waits_for_writer_and_stale_legacy_previous_cannot_block_reclaim_or_later_commit() {
    use std::{sync::mpsc, time::Duration};
    for mode in MODES {
        let dir = Directory::new();
        let env = seed(&dir.0, mode);
        update(&env);
        let writer = env.begin_write().unwrap();
        std::thread::scope(|scope| {
            let (starting, started) = mpsc::channel();
            let (done, completed) = mpsc::channel();
            let env = &env;
            scope.spawn(move || {
                starting.send(()).unwrap();
                done.send(env.rollback_last_commit(2)).unwrap();
            });
            started.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(completed.recv_timeout(Duration::from_millis(20)).is_err());
            assert_eq!(env.begin_read().sequence(), 2);
            writer.abort();
            assert_eq!(
                completed
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .unwrap()
                    .sequence,
                1
            );
        });
        if mode == PublicationMode::Legacy {
            fs::write(dir.0.join("PREVIOUS"), b"obsolete damaged ancillary record").unwrap();
        }
        env.reclaim().unwrap();
        inspect(&env.begin_read(), false);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"later").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        assert_eq!(env.rollback_last_commit(2).unwrap().sequence, 1);
        inspect(&env.begin_read(), false);
    }
}

#[test]
fn bounded_locked_residency_denial_preserves_head_locks_and_pin_count() {
    for mode in MODES {
        let dir = Directory::new();
        let env = seed(&dir.0, mode);
        update(&env);
        drop(env);
        let measured = Environment::open_with_residency(
            &dir.0,
            ulmdb::ResidencyPolicy::Prefault {
                max_bytes: u64::MAX,
            },
        )
        .unwrap();
        let footprint = measured.residency_report().page_rounded_bytes.unwrap();
        drop(measured);
        assert!(
            footprint > 0 && footprint < 1_048_576,
            "bounded fixture: {footprint}"
        );
        // The current immutable mapping consumes this exact global lock budget.
        // Loading A for rollback would require an additional independent lease,
        // so admission must refuse it before any selection or candidate pin.
        let env = Environment::open_with_residency(
            &dir.0,
            ulmdb::ResidencyPolicy::RequiredLocked {
                max_bytes: footprint,
            },
        )
        .unwrap();
        let head = fs::read(dir.0.join("CURRENT")).unwrap();
        let status = env.residency_status();
        let pins = || {
            fs::read_dir(dir.0.join(".ulmdb-coordination/pins"))
                .unwrap()
                .count()
        };
        let before_pins = pins();
        assert!(matches!(
            env.rollback_last_commit(2),
            Err(RollbackError::Engine(Error::LimitExceeded))
        ));
        assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), head);
        assert_eq!(env.residency_status(), status);
        assert_eq!(pins(), before_pins);
        inspect(&env.begin_read(), true);
        env.begin_write().unwrap().abort();
        drop(env);
        let env = Environment::open(&dir.0).unwrap();
        assert_eq!(env.rollback_last_commit(2).unwrap().sequence, 1);
        inspect(&env.begin_read(), false);
    }
}

#[test]
fn replication_rejects_undone_cursor_forks_and_full_resnapshot_restores_continuation() {
    use ulmdb::replication::{
        self, Cursor, PublisherKey, ReplicationError as RError, TransferLimits,
    };
    for mode in MODES {
        let dir = Directory::new();
        let env = seed(&dir.0, mode);
        update(&env);
        let undone = env.begin_read();
        let b = Cursor::from_read(&undone).unwrap();
        let key = PublisherKey::new(11, b.uuid, b.epoch, [0x4f; 32]).unwrap();
        let staging = Directory::new();
        fs::create_dir(&staging.0).unwrap();
        let receive = |wire: Vec<u8>| {
            replication::receive(
                &mut std::io::Cursor::new(wire),
                &key,
                TransferLimits::default(),
                &staging.0,
            )
            .unwrap()
        };
        let mut full_b = Vec::new();
        replication::send_snapshot(&env, &undone, None, &key, &mut full_b).unwrap();
        let b_frame = receive(full_b);
        let b_target = Directory::new();
        let b_replica = replication::bootstrap(&b_frame, &b_target.0).unwrap();
        env.rollback_last_commit(2).unwrap();
        let restored = env.begin_read();
        let r = Cursor::from_read(&restored).unwrap();
        assert_eq!(r.sequence, 1);
        assert_eq!(r.uuid, b.uuid);
        assert_eq!(r.epoch, b.epoch);
        let mut stale_delta = Vec::new();
        assert!(matches!(
            replication::send_snapshot(&env, &restored, Some(&undone), &key, &mut stale_delta),
            Err(RError::Gap)
        ));
        assert!(stale_delta.is_empty());
        let mut full_r = Vec::new();
        replication::send_snapshot(&env, &restored, None, &key, &mut full_r).unwrap();
        let r_frame = receive(full_r);
        assert!(matches!(
            b_replica.apply_replication(&r_frame),
            Err(RError::Gap)
        ));
        assert_eq!(Cursor::from_read(&b_replica.begin_read()).unwrap(), b);
        // Existing replicas do not accept a branch reset through apply. A full
        // authenticated snapshot must bootstrap a fresh target explicitly.
        let r_target = Directory::new();
        let r_replica = replication::bootstrap(&r_frame, &r_target.0).unwrap();
        assert_eq!(Cursor::from_read(&r_replica.begin_read()).unwrap(), r);
        inspect(&r_replica.begin_read(), false);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"branch").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        let branch = env.begin_read();
        let c = Cursor::from_read(&branch).unwrap();
        assert_eq!(c.sequence, b.sequence);
        assert_ne!(c.commit, b.commit);
        assert_ne!(c.checkpoint, b.checkpoint);
        let mut stale_delta = Vec::new();
        assert!(matches!(
            replication::send_snapshot(&env, &branch, Some(&undone), &key, &mut stale_delta),
            Err(RError::Gap)
        ));
        assert!(stale_delta.is_empty());
        let mut continuation = Vec::new();
        replication::send_snapshot(&env, &branch, Some(&restored), &key, &mut continuation)
            .unwrap();
        let delta = receive(continuation);
        assert!(matches!(
            b_replica.apply_replication(&delta),
            Err(RError::Gap)
        ));
        r_replica.apply_replication(&delta).unwrap();
        assert_eq!(Cursor::from_read(&r_replica.begin_read()).unwrap(), c);
        assert_eq!(
            r_replica.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"branch".as_slice())
        );
        assert_eq!(Cursor::from_read(&b_replica.begin_read()).unwrap(), b);
    }
}
