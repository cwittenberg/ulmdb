use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{
    Address, CommitIntent, CoordinatedEnvironment, DatabaseKind, Environment, Error, Prefix,
    PreviousError, PreviousSession, PublicationMode, ReadTxn, ResidencyPolicy,
};
#[path = "support/pin_records.rs"]
mod pin_records;
// G02 may lend immutable WriteTxn views to NOTLS readers. Adding a selection
// plan must preserve the native source's Sync requirement (compile-time proof).
const _: fn() = || {
    fn require_sync<T: Sync>() {}
    require_sync::<ulmdb::WriteTxn<'static>>();
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
            "ulmdb-previous-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn pins(p: &Path) -> usize {
    pin_records::paths(p).len()
}
fn v4() -> Prefix {
    Prefix::new(Address::V4(0xc0000200), 24).unwrap()
}
fn v6() -> Prefix {
    Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap()
}
fn seed(
    path: &Path,
    mode: PublicationMode,
) -> (
    Environment,
    ulmdb_storage::Manifest,
    ulmdb_storage::Manifest,
) {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"dups", DatabaseKind::Duplicates { width: None }),
        (b"fixed", DatabaseKind::Duplicates { width: Some(2) }),
        (b"ip", DatabaseKind::Ipv4),
        (b"ip6", DatabaseKind::Ipv6),
        (b"original", DatabaseKind::Kv),
    ] {
        w.create_database(name, kind).unwrap();
    }
    w.put(b"kv", b"key", b"A").unwrap();
    w.put(b"original", b"key", b"A").unwrap();
    w.insert_duplicate(b"dups", b"key", b"A").unwrap();
    w.insert_duplicate(b"fixed", b"key", b"AA").unwrap();
    w.insert_prefix(b"ip", v4(), b"A").unwrap();
    w.insert_prefix(b"ip6", v6(), b"A").unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    let a = ulmdb_storage::read_current(path).unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"B").unwrap();
    w.drop_database(b"original").unwrap();
    w.create_database(b"undone", DatabaseKind::Kv).unwrap();
    w.put(b"undone", b"key", b"B").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
    let b = ulmdb_storage::read_current(path).unwrap();
    (env, a, b)
}
#[test]
fn previous_preparation_branches_every_kind_and_keeps_both_historical_readers() {
    for mode in MODES {
        let d = Directory::new();
        let (env, a, b) = seed(&d.0, mode);
        let old_b = env.begin_read();
        let b_bytes = old_b.get(b"kv", b"key").unwrap().unwrap();
        let a_file = ulmdb_storage::descriptor_path(&d.0, &a);
        let a_bytes = fs::read(&a_file).unwrap();
        let b_file = ulmdb_storage::descriptor_path(&d.0, &b);
        let b_file_bytes = fs::read(&b_file).unwrap();
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        assert!(session.is_previous());
        let old_a = session.begin_read();
        let borrowed = old_a.get(b"kv", b"key").unwrap().unwrap();
        assert_eq!(borrowed, b"A");
        assert!(matches!(Environment::open(&d.0), Err(Error::Busy)));
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let baseline = pins(&d.0);
        let mut abandoned = session.begin_write().unwrap();
        abandoned
            .create_database(b"abandoned", DatabaseKind::Kv)
            .unwrap();
        let burned = abandoned.open_database(b"abandoned").unwrap().id();
        assert!(burned >= b.next_database);
        abandoned.abort();
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        w.delete_duplicates(b"dups", b"key").unwrap();
        w.insert_duplicate(b"dups", b"key", b"C").unwrap();
        w.insert_duplicate(b"fixed", b"key", b"CC").unwrap();
        w.insert_prefix(b"ip", v4(), b"C").unwrap();
        w.insert_prefix(b"ip6", v6(), b"C").unwrap();
        w.drop_database(b"original").unwrap();
        w.create_database(b"replacement", DatabaseKind::Kv).unwrap();
        let id = w.open_database(b"replacement").unwrap().id();
        assert!(id > burned);
        let prepared = w.prepare().unwrap();
        assert_eq!(prepared.sequence(), 2);
        assert_eq!(pins(&d.0), baseline + 1);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        assert!(session.is_previous());
        assert_eq!(
            session.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"A".as_slice())
        );
        assert_eq!(
            ReadTxn::open_coordinated_snapshot(&d.0)
                .unwrap()
                .get(b"kv", b"key")
                .unwrap(),
            Some(b"B".as_slice())
        );
        assert_eq!(prepared.commit().unwrap(), 2);
        assert!(!session.is_previous());
        let c = ulmdb_storage::read_current(&d.0).unwrap();
        assert_eq!(c.previous, a.generation);
        assert_eq!(c.parent, a.commit);
        assert_ne!(c.commit, b.commit);
        assert_eq!(
            ulmdb_storage::read_previous(&d.0)
                .unwrap()
                .unwrap()
                .generation,
            a.generation
        );
        assert!(c.next_database > b.next_database);
        let read = session.begin_read();
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"C".as_slice()));
        assert_eq!(
            read.duplicates(b"dups").unwrap().first(b"key").unwrap(),
            Some(b"C".as_slice())
        );
        assert!(
            read.duplicates(b"fixed")
                .unwrap()
                .contains(b"key", b"CC")
                .unwrap()
        );
        assert_eq!(
            read.exact_prefix(b"ip", v4()).unwrap(),
            Some((v4(), b"C".as_slice()))
        );
        assert_eq!(
            read.exact_prefix(b"ip6", v6()).unwrap(),
            Some((v6(), b"C".as_slice()))
        );
        assert!(matches!(
            read.open_database(b"original"),
            Err(Error::MissingDatabase)
        ));
        assert!(matches!(
            read.open_database(b"undone"),
            Err(Error::MissingDatabase)
        ));
        let env = session.into_environment().ok().unwrap();
        env.reclaim().unwrap();
        assert_eq!(borrowed, b"A");
        assert_eq!(b_bytes, b"B");
        assert_eq!(fs::read(a_file).unwrap(), a_bytes);
        assert_eq!(fs::read(b_file).unwrap(), b_file_bytes);
        assert_eq!(
            old_a.get(b"original", b"key").unwrap(),
            Some(b"A".as_slice())
        );
        drop(env);
        let independent = CoordinatedEnvironment::open(&d.0).unwrap();
        let next = independent
            .update(|w| {
                w.create_database(b"later", DatabaseKind::Kv)?;
                Ok(w.open_database(b"later")?.id())
            })
            .unwrap();
        assert!(next > id);
        assert_eq!(
            independent
                .begin_read()
                .unwrap()
                .get(b"kv", b"key")
                .unwrap(),
            Some(b"C".as_slice())
        );
    }
}
#[test]
fn clean_abort_and_prepared_drop_retain_previous_mode_without_leaking_pins() {
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let baseline = pins(&d.0);
        assert_eq!(session.begin_write().unwrap().commit().unwrap(), 1);
        let clean = session.begin_write().unwrap().prepare().unwrap();
        assert!(!clean.will_publish());
        assert_eq!(clean.commit().unwrap(), 1);
        assert!(session.is_previous());
        for abort in [true, false] {
            let mut w = session.begin_write().unwrap();
            w.put(b"kv", b"key", b"discard").unwrap();
            let p = w.prepare().unwrap();
            assert_eq!(pins(&d.0), baseline + 1);
            if abort {
                p.abort()
            } else {
                drop(p)
            };
            assert_eq!(pins(&d.0), baseline);
            assert!(session.is_previous());
            assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        }
        session.begin_write().unwrap().abort();
        let session = session.into_environment().err().unwrap();
        let kept = session.begin_read();
        drop(session);
        assert_eq!(kept.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
        let env = Environment::open(&d.0).unwrap();
        env.reclaim().unwrap();
        assert_eq!(kept.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"B".as_slice())
        );
    }
}
#[test]
fn genesis_and_actual_zero_predecessor_are_real_roots_and_force_noop_publishes() {
    for mode in MODES {
        for history in [0, 1] {
            let d = Directory::new();
            let env = Environment::create_with_publication(&d.0, mode).unwrap();
            if history == 1 {
                let mut w = env.begin_write().unwrap();
                w.create_database(b"onlyB", DatabaseKind::Kv).unwrap();
                w.commit().unwrap();
            }
            let b = ulmdb_storage::read_current(&d.0).unwrap();
            drop(env);
            let session = PreviousSession::open(&d.0).unwrap();
            assert_eq!(session.begin_read().sequence(), 0);
            assert!(session.begin_read().database_names().unwrap().is_empty());
            assert_eq!(session.begin_write().unwrap().commit().unwrap(), 0);
            assert!(session.is_previous());
            let prepared = session
                .begin_write()
                .unwrap()
                .prepare_with_intent(CommitIntent::ForcePublication)
                .unwrap();
            assert!(prepared.will_publish());
            assert_eq!(prepared.commit().unwrap(), 1);
            assert!(!session.is_previous());
            let c = ulmdb_storage::read_current(&d.0).unwrap();
            assert!(c.next_database >= b.next_database);
            assert_eq!(session.begin_read().sequence(), 1);
            assert!(session.begin_read().database_names().unwrap().is_empty());
        }
    }
}
#[test]
fn historyless_exports_and_authenticated_rollback_roots_do_not_invent_history() {
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        let exported = Directory::new();
        ulmdb::backup::export(&env, &exported.0).unwrap();
        assert!(matches!(
            PreviousSession::open(&exported.0),
            Err(PreviousError::NoPrevious)
        ));
        env.rollback_last_commit(2).unwrap();
        drop(env);
        assert!(matches!(
            PreviousSession::open(&d.0),
            Err(PreviousError::NoPrevious)
        ));
    }
}
#[test]
fn previous_reader_outlives_temporary_writer_fence_and_normal_reclamation() {
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        drop(env);
        let read =
            ReadTxn::open_previous_coordinated_snapshot(&d.0, ResidencyPolicy::Demand).unwrap();
        let bytes = read.get(b"kv", b"key").unwrap().unwrap();
        let env = Environment::open(&d.0).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"D").unwrap();
        w.commit().unwrap();
        env.reclaim().unwrap();
        assert_eq!(bytes, b"A");
        assert_eq!(read.sequence(), 1);
    }
}
#[test]
fn admission_denial_releases_open_and_candidate_ownership_without_poison() {
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        drop(env);
        let baseline = pins(&d.0);
        assert!(matches!(
            PreviousSession::open_with_residency(&d.0, ResidencyPolicy::Prefault { max_bytes: 0 }),
            Err(PreviousError::Engine(Error::LimitExceeded))
        ));
        assert_eq!(pins(&d.0), baseline);
        let read =
            ReadTxn::open_previous_coordinated_snapshot(&d.0, ResidencyPolicy::Demand).unwrap();
        let budget = read.residency_report().mapped_bytes.unwrap() + 4096;
        drop(read);
        let session = PreviousSession::open_with_residency(
            &d.0,
            ResidencyPolicy::Prefault { max_bytes: budget },
        )
        .unwrap();
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let baseline = pins(&d.0);
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", &vec![0x41; 131072]).unwrap();
        assert!(matches!(w.prepare(), Err(Error::LimitExceeded)));
        assert_eq!(pins(&d.0), baseline);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        assert!(session.is_previous());
        session.begin_write().unwrap().abort();
    }
}
#[test]
fn stale_legacy_pair_selected_corruption_and_foreign_tokens_are_rejected() {
    let d = Directory::new();
    let (env, a, b) = seed(&d.0, PublicationMode::Legacy);
    drop(env);
    let old = fs::read(d.0.join("PREVIOUS")).unwrap();
    fs::write(
        d.0.join("PREVIOUS"),
        ulmdb_storage::checkpoint_bytes(&b).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        PreviousSession::open(&d.0),
        Err(PreviousError::Engine(Error::Corrupt(_)))
    ));
    fs::write(d.0.join("PREVIOUS"), old).unwrap();
    let descriptor = ulmdb_storage::descriptor_path(&d.0, &b);
    let bbytes = fs::read(&descriptor).unwrap();
    fs::write(&descriptor, b"bad selected descriptor").unwrap();
    assert!(ulmdb_storage::read_previous(&d.0).unwrap().is_some());
    assert!(matches!(
        PreviousSession::open(&d.0),
        Err(PreviousError::Engine(Error::Corrupt(_)))
    ));
    fs::write(descriptor, bbytes).unwrap();
    assert_eq!(
        ulmdb_storage::read_previous(&d.0)
            .unwrap()
            .unwrap()
            .generation,
        a.generation
    );
    for mode in MODES {
        let x = Directory::new();
        let (env, a, _) = seed(&x.0, mode);
        let selected = ulmdb_storage::capture_previous_selection(&x.0)
            .unwrap()
            .unwrap();
        let y = Directory::new();
        let other = Environment::create_with_publication(&y.0, mode).unwrap();
        let base = ulmdb_storage::read_current(&y.0).unwrap();
        let mut foreign = base.clone();
        foreign.previous = base.generation;
        foreign.parent = base.commit;
        foreign.sequence = 1;
        foreign.generation = ulmdb_storage::new_identity().unwrap();
        foreign.commit = foreign.generation;
        if foreign.format_minor == 4 {
            foreign.prepare_indexed(Some(&base)).unwrap();
        }
        let token = ulmdb_storage::prepare_publication(&y.0, &foreign).unwrap();
        assert!(ulmdb_storage::prepare_previous_publication(&selected, &a, token).is_err());
        drop(other);
        drop(env);
    }
}
#[test]
fn range_and_replicas_are_explicitly_unsupported() {
    for mode in [PublicationMode::RangeV5]
        .into_iter()
        .chain(cfg!(feature = "range-lifecycle").then_some(PublicationMode::Range))
    {
        let d = Directory::new();
        drop(Environment::create_with_publication(&d.0, mode).unwrap());
        assert!(matches!(
            PreviousSession::open(&d.0),
            Err(PreviousError::Engine(Error::InvalidInput(_)))
        ));
    }
    for mode in MODES {
        use ulmdb::replication::{self, Cursor, PublisherKey, TransferLimits};
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        let read = env.begin_read();
        let cursor = Cursor::from_read(&read).unwrap();
        let key = PublisherKey::new(1, cursor.uuid, cursor.epoch, [7; 32]).unwrap();
        let mut wire = vec![];
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
        let target = Directory::new();
        drop(replication::bootstrap(&frame, &target.0).unwrap());
        assert!(matches!(
            PreviousSession::open(&target.0),
            Err(PreviousError::Engine(Error::ReadOnly))
        ));
    }
}

#[test]
fn damaged_b_user_data_does_not_prevent_loading_authenticated_a() {
    for mode in MODES {
        let d = Directory::new();
        let (env, a, b) = seed(&d.0, mode);
        drop(env);
        let entry = b
            .segments
            .iter()
            .find(|entry| !a.segments.iter().any(|old| old.id == entry.id))
            .unwrap();
        let path = ulmdb_storage::segment_path(&d.0, entry);
        let bytes = fs::read(&path).unwrap();
        let mut damaged = bytes.clone();
        let last = usize::try_from(entry.length).unwrap() - 1;
        damaged[last] ^= 1;
        let replacement = path.with_extension("damaged");
        fs::write(&replacement, &damaged).unwrap();
        fs::rename(replacement, &path).unwrap();
        assert!(ReadTxn::open_coordinated_snapshot(&d.0).is_err());
        let session = PreviousSession::open(&d.0).unwrap();
        assert_eq!(
            session.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"A".as_slice())
        );
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        assert_eq!(
            session.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"C".as_slice())
        );
        assert_eq!(fs::read(path).unwrap(), damaged);
    }
}
#[test]
fn mixed_format_history_is_explicitly_unsupported_and_same_format_maintenance_is_valid() {
    for target in [3, 4] {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, PublicationMode::Legacy);
        if target == 3 {
            env.upgrade_publication().unwrap();
        } else {
            env.upgrade_inventory().unwrap();
        }
        drop(env);
        assert!(matches!(
            PreviousSession::open(&d.0),
            Err(PreviousError::Engine(Error::InvalidInput(_)))
        ));
    }
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        env.compact().unwrap();
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        assert_eq!(session.begin_read().sequence(), 2);
        assert_eq!(
            session.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"B".as_slice())
        );
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        assert_eq!(w.commit().unwrap(), 3);
        assert!(!session.is_previous());
    }
}
#[test]
fn capture_and_candidate_pin_exclusion_fail_without_leaks_or_writer_poison() {
    for mode in MODES {
        let d = Directory::new();
        let (env, _, b) = seed(&d.0, mode);
        drop(env);
        let coordinator =
            ulmdb_storage::coordination::Coordination::open(&d.0, b.uuid, Default::default())
                .unwrap();
        let maintenance = coordinator.try_maintenance().unwrap();
        let baseline = pins(&d.0);
        assert!(matches!(
            PreviousSession::open(&d.0),
            Err(PreviousError::Engine(Error::Busy))
        ));
        assert_eq!(pins(&d.0), baseline);
        drop(maintenance);
        let session = PreviousSession::open(&d.0).unwrap();
        let baseline = pins(&d.0);
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let maintenance = coordinator.try_maintenance().unwrap();
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        assert!(matches!(w.prepare(), Err(Error::Busy)));
        assert_eq!(pins(&d.0), baseline);
        assert!(session.is_previous());
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        drop(maintenance);
        session.begin_write().unwrap().abort();
    }
}
#[test]
fn local_writer_waits_for_prepared_owner_and_uses_c_after_previous_mode_ends() {
    use std::{sync::mpsc, time::Duration};
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        let prepared = w.prepare().unwrap();
        std::thread::scope(|scope| {
            let (start_tx, start_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let session = &session;
            scope.spawn(move || {
                start_tx.send(()).unwrap();
                let mut w = session.begin_write().unwrap();
                assert_eq!(w.get(b"kv", b"key").unwrap(), Some(b"C".as_slice()));
                w.put(b"kv", b"key", b"D").unwrap();
                done_tx.send(w.commit().unwrap()).unwrap();
            });
            start_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(done_rx.recv_timeout(Duration::from_millis(30)).is_err());
            assert_eq!(prepared.commit().unwrap(), 2);
            assert_eq!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 3);
        });
        assert_eq!(
            session.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"D".as_slice())
        );
    }
}
#[test]
fn previous_branch_rejects_stale_replication_cursor_and_new_snapshot_continues() {
    use ulmdb::replication::{
        self, Cursor, PublisherKey, ReplicationError as RError, TransferLimits,
    };
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        let b_read = env.begin_read();
        let b = Cursor::from_read(&b_read).unwrap();
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
        let mut wire = vec![];
        replication::send_snapshot(&env, &b_read, None, &key, &mut wire).unwrap();
        let frame = receive(wire);
        let old_target = Directory::new();
        let old_replica = replication::bootstrap(&frame, &old_target.0).unwrap();
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        w.commit().unwrap();
        let env = session.into_environment().ok().unwrap();
        let c_read = env.begin_read();
        let c = Cursor::from_read(&c_read).unwrap();
        assert_eq!(b.sequence, c.sequence);
        assert_ne!(b.commit, c.commit);
        let mut stale = vec![];
        assert!(matches!(
            replication::send_snapshot(&env, &c_read, Some(&b_read), &key, &mut stale),
            Err(RError::InvalidInput(_))
        ));
        assert!(stale.is_empty());
        let mut wire = vec![];
        replication::send_snapshot(&env, &c_read, None, &key, &mut wire).unwrap();
        let c_frame = receive(wire);
        assert!(matches!(
            old_replica.apply_replication(&c_frame),
            Err(RError::Gap)
        ));
        let fresh = Directory::new();
        let replica = replication::bootstrap(&c_frame, &fresh.0).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"D").unwrap();
        w.commit().unwrap();
        let read = env.begin_read();
        let mut wire = vec![];
        replication::send_snapshot(&env, &read, Some(&c_read), &key, &mut wire).unwrap();
        let delta = receive(wire);
        assert!(matches!(
            old_replica.apply_replication(&delta),
            Err(RError::Gap)
        ));
        replica.apply_replication(&delta).unwrap();
        assert_eq!(
            replica.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"D".as_slice())
        );
        assert_eq!(Cursor::from_read(&old_replica.begin_read()).unwrap(), b);
    }
}
#[test]
fn previous_token_is_distinct_from_ordinary_publication_and_checks_exact_candidate() {
    for mode in MODES {
        let d = Directory::new();
        let (env, a, b) = seed(&d.0, mode);
        let selection = ulmdb_storage::capture_previous_selection(&d.0)
            .unwrap()
            .unwrap();
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let candidate = || {
            let mut c = a.clone();
            c.previous = a.generation;
            c.parent = a.commit;
            c.sequence = a.sequence + 1;
            c.generation = ulmdb_storage::new_identity().unwrap();
            c.commit = c.generation;
            c.next_database = a.next_database.max(b.next_database);
            if c.format_minor == 4 {
                c.prepare_indexed(Some(&a)).unwrap();
            }
            c
        };
        let c = candidate();
        let ordinary = ulmdb_storage::prepare_publication(&d.0, &c).unwrap();
        assert!(matches!(ordinary.publish(), Err(Error::Corrupt(_))));
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        let c = candidate();
        let token = ulmdb_storage::prepare_publication(&d.0, &c).unwrap();
        let mut wrong = c.clone();
        wrong.next_database += 1;
        if wrong.format_minor == 4 {
            wrong.prepare_indexed(Some(&a)).unwrap();
        }
        assert!(ulmdb_storage::prepare_previous_publication(&selection, &wrong, token).is_err());
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        let c = candidate();
        let token = ulmdb_storage::prepare_publication(&d.0, &c).unwrap();
        let prepared = ulmdb_storage::prepare_previous_publication(&selection, &c, token).unwrap();
        prepared.publish().unwrap();
        assert_eq!(
            ulmdb_storage::read_current(&d.0).unwrap().generation,
            c.generation
        );
        assert_eq!(
            ulmdb_storage::read_previous(&d.0)
                .unwrap()
                .unwrap()
                .generation,
            a.generation
        );
        drop(env);
    }
}
#[test]
fn changed_selected_head_poison_precedes_ancillary_history_errors() {
    for mode in MODES {
        let d = Directory::new();
        let (env, _, _) = seed(&d.0, mode);
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let mut damaged = head.clone();
        damaged[0] ^= 1;
        fs::write(d.0.join("CURRENT"), damaged).unwrap();
        let ancillary = fs::read(d.0.join("PREVIOUS")).ok();
        fs::write(d.0.join("PREVIOUS"), b"bad ancillary").unwrap();
        assert!(matches!(session.begin_write(), Err(Error::OutcomeUnknown)));
        assert!(session.is_previous());
        assert_eq!(
            session.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"A".as_slice())
        );
        drop(session);
        fs::write(d.0.join("CURRENT"), head).unwrap();
        if let Some(bytes) = ancillary {
            fs::write(d.0.join("PREVIOUS"), bytes).unwrap();
        } else {
            fs::remove_file(d.0.join("PREVIOUS")).unwrap();
        }
        let env = Environment::open(&d.0).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"B".as_slice())
        );
    }
}
#[test]
fn strict_pair_rejects_foreign_realm_epoch_and_nonempty_false_genesis() {
    for mode in MODES {
        for epoch in [false, true] {
            let d = Directory::new();
            let (env, _, b) = seed(&d.0, mode);
            let mut changed = b.clone();
            changed.previous = b.generation;
            changed.parent = b.commit;
            changed.sequence += 1;
            changed.generation = ulmdb_storage::new_identity().unwrap();
            changed.commit = changed.generation;
            if epoch {
                changed.epoch += 1
            } else {
                changed.uuid[0] ^= 1
            };
            if changed.format_minor == 4 {
                changed.prepare_indexed(Some(&b)).unwrap();
            }
            ulmdb_storage::prepare_publication(&d.0, &changed)
                .unwrap()
                .publish()
                .unwrap();
            drop(env);
            assert!(matches!(
                PreviousSession::open(&d.0),
                Err(PreviousError::Engine(Error::Corrupt(_)))
            ));
        }
    }
    let d = Directory::new();
    let (env, _, b) = seed(&d.0, PublicationMode::Legacy);
    drop(env);
    let mut malformed = b;
    malformed.sequence = 0;
    malformed.parent = 0;
    malformed.previous = 0;
    malformed.generation = ulmdb_storage::new_identity().unwrap();
    malformed.commit = malformed.generation;
    let _proof = ulmdb_storage::prepare_publication(&d.0, &malformed).unwrap();
    fs::write(
        d.0.join("CURRENT"),
        ulmdb_storage::checkpoint_bytes(&malformed).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        PreviousSession::open(&d.0),
        Err(PreviousError::Engine(Error::Corrupt(_)))
    ));
}
#[test]
fn previous_branch_can_rollback_to_a_without_reusing_discarded_ids() {
    for mode in MODES {
        let d = Directory::new();
        let (env, a, b) = seed(&d.0, mode);
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        let mut w = session.begin_write().unwrap();
        w.create_database(b"branch", DatabaseKind::Kv).unwrap();
        let discarded = w.open_database(b"branch").unwrap().id();
        assert!(discarded >= b.next_database);
        w.put(b"kv", b"key", b"C").unwrap();
        w.commit().unwrap();
        let c_read = session.begin_read();
        let env = session.into_environment().ok().unwrap();
        env.rollback_last_commit(2).unwrap();
        let restored = ulmdb_storage::read_current(&d.0).unwrap();
        assert_eq!(restored.sequence, a.sequence);
        assert_eq!(restored.commit, a.commit);
        assert_eq!(restored.previous, 0);
        assert!(restored.next_database > discarded);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"A".as_slice())
        );
        assert_eq!(c_read.get(b"kv", b"key").unwrap(), Some(b"C".as_slice()));
        drop(env);
        assert!(matches!(
            PreviousSession::open(&d.0),
            Err(PreviousError::NoPrevious)
        ));
        let independent = CoordinatedEnvironment::open(&d.0).unwrap();
        let later = independent
            .update(|w| {
                w.create_database(b"later", DatabaseKind::Kv)?;
                Ok(w.open_database(b"later")?.id())
            })
            .unwrap();
        assert!(later > discarded);
    }
}
#[test]
fn authenticated_but_incoherent_previous_lineage_is_corrupt() {
    for mode in MODES {
        for mutation in [0, 1, 2] {
            let d = Directory::new();
            let (env, _, a) = seed(&d.0, mode);
            let mut b = a.clone();
            b.previous = a.generation;
            b.parent = a.commit;
            b.sequence = a.sequence + 1;
            b.generation = ulmdb_storage::new_identity().unwrap();
            b.commit = b.generation;
            match mutation {
                0 => b.sequence += 1,
                1 => b.sequence = a.sequence,
                2 => b.parent = ulmdb_storage::new_identity().unwrap(),
                _ => unreachable!(),
            };
            if b.format_minor == 4 {
                b.prepare_indexed(Some(&a)).unwrap();
            }
            ulmdb_storage::prepare_publication(&d.0, &b)
                .unwrap()
                .publish()
                .unwrap();
            drop(env);
            assert!(
                matches!(
                    PreviousSession::open(&d.0),
                    Err(PreviousError::Engine(Error::Corrupt(_)))
                ),
                "{mode:?}/{mutation}"
            );
        }
    }
}
