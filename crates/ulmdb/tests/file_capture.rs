use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{
    CommitIntent, DatabaseKind, Environment, Error, OwnedFileCapture, PreviousError,
    PreviousSession, PublicationMode, ResidencyPolicy,
};
use ulmdb_storage::coordination::{Coordination, DirectoryCapture, Limits};
#[path = "support/pin_records.rs"]
mod pin_records;
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
const _: fn() = || {
    fn sync<T: Sync>() {}
    sync::<ulmdb::WriteTxn<'static>>();
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "owned-file-capture-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn capture(path: &Path) -> DirectoryCapture {
    Coordination::capture_directory(path, Default::default()).unwrap()
}
fn pins(path: &Path) -> usize {
    pin_records::paths(path).len()
}
fn seed(path: &Path, mode: PublicationMode) -> Environment {
    let e = Environment::create_with_publication(path, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"key", b"B").unwrap();
    w.commit().unwrap();
    e
}
#[test]
fn metadata_capture_defers_admission_and_preserves_exact_d_and_v_during_choice_changes() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let b = env.file_checkpoint().unwrap();
        let a = *ulmdb_storage::capture_previous_selection(&d.0)
            .unwrap()
            .unwrap()
            .previous_checkpoint();
        let frame = OwnedFileCapture::current(&d.0, capture(&d.0)).unwrap();
        assert_eq!(*frame.selected_checkpoint(), b);
        assert_eq!(*frame.view_checkpoint(), b);
        assert_eq!(frame.selected_sequence(), 2);
        assert_eq!(frame.view_sequence(), 2);
        let meta =
            ulmdb_platform::file_identity(&ulmdb_platform::open_directory(&d.0).unwrap()).unwrap();
        let actual = frame.directory_identity();
        assert_eq!(actual.0, meta.volume);
        assert_eq!(u128::from(actual.1), meta.file_id);
        assert_eq!(frame.directory(), d.0.canonicalize().unwrap());
        let frame = frame.select_previous().unwrap();
        assert_eq!(*frame.selected_checkpoint(), b);
        assert_eq!(*frame.view_checkpoint(), a);
        assert_eq!(frame.view_sequence(), 1);
        let frame = frame.select_selected().unwrap();
        assert_eq!(*frame.view_checkpoint(), b);
        let frame = frame.select_previous().unwrap();
        let (read, pair) = frame.admit(ResidencyPolicy::Demand).unwrap();
        let pair = pair.unwrap();
        assert_eq!(*pair.selected_checkpoint(), b);
        assert_eq!(*pair.previous_checkpoint(), a);
        assert!(pair.matches_directory(&d.0).unwrap());
        assert_eq!(read.file_checkpoint().unwrap(), a);
        assert_eq!(read.file_manifest().unwrap().sequence, 1);
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
        let lease = Coordination::open(&d.0, pair.selected().uuid, Default::default())
            .unwrap()
            .try_maintenance()
            .unwrap();
        drop(lease);
    }
}
#[test]
fn cloned_namespace_captures_cannot_be_mixed_even_when_realm_and_data_are_shared() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let clone = Directory::new();
        ulmdb::backup::export(&env, &clone.0).unwrap();
        let before = pins(&clone.0);
        let wrong = capture(&clone.0);
        assert!(matches!(
            OwnedFileCapture::current(&d.0, wrong),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(pins(&clone.0), before);
        let wrong = capture(&clone.0);
        assert!(matches!(
            OwnedFileCapture::previous(&d.0, wrong),
            Err(PreviousError::Engine(Error::InvalidInput(_)))
        ));
        assert_eq!(pins(&clone.0), before);
        let pair = ulmdb_storage::capture_previous_selection(&d.0)
            .unwrap()
            .unwrap();
        assert!(!pair.matches_directory(&clone.0).unwrap());
    }
}
#[test]
fn choosing_cached_d_never_rediscovers_current_after_publication_or_path_hiding() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let b = env.file_checkpoint().unwrap();
        let frame = OwnedFileCapture::previous(&d.0, capture(&d.0)).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        w.commit().unwrap();
        let saved = d.0.join("CURRENT.saved-test");
        fs::rename(d.0.join("CURRENT"), &saved).unwrap();
        let (read, pair) = frame
            .select_selected()
            .unwrap()
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        assert!(pair.is_some());
        assert_eq!(read.file_checkpoint().unwrap(), b);
        assert_eq!(read.sequence(), 2);
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
        fs::rename(saved, d.0.join("CURRENT")).unwrap();
        assert_ne!(env.file_checkpoint().unwrap(), b);
        env.reclaim().unwrap();
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
    }
}
#[test]
fn current_first_detects_same_sequence_branch_change_and_rollback_without_opening_stale_v() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        drop(env);
        let old = OwnedFileCapture::current(&d.0, capture(&d.0)).unwrap();
        let b = *old.selected_checkpoint();
        let session = PreviousSession::open(&d.0).unwrap();
        assert_eq!(session.selected_checkpoint().unwrap(), b);
        let mut w = session.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        assert_ne!(session.selected_checkpoint().unwrap(), b);
        assert!(matches!(
            old.select_previous(),
            Err(PreviousError::Engine(Error::Busy))
        ));
        let env = session.into_environment().ok().unwrap();
        let old = OwnedFileCapture::current(&d.0, capture(&d.0)).unwrap();
        env.rollback_last_commit(2).unwrap();
        let r = env.file_checkpoint().unwrap();
        if mode == PublicationMode::Legacy {
            fs::write(d.0.join("PREVIOUS"), b"irrelevant stale history").unwrap();
        }
        assert!(matches!(
            old.select_previous(),
            Err(PreviousError::Engine(Error::Busy))
        ));
        let (read, pair) = OwnedFileCapture::current(&d.0, capture(&d.0))
            .unwrap()
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        assert!(pair.is_none());
        assert_eq!(read.file_checkpoint().unwrap(), r);
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
        assert!(matches!(
            OwnedFileCapture::current(&d.0, capture(&d.0))
                .unwrap()
                .select_previous(),
            Err(PreviousError::NoPrevious)
        ));
    }
}
#[test]
fn canonical_current_ignores_bad_legacy_ancillary_but_matching_history_is_corrupt() {
    let d = Directory::new();
    let env = seed(&d.0, PublicationMode::Legacy);
    fs::write(d.0.join("PREVIOUS"), b"bad stable predecessor").unwrap();
    let frame = OwnedFileCapture::current(&d.0, capture(&d.0)).unwrap();
    assert!(matches!(
        frame.select_previous(),
        Err(PreviousError::Engine(Error::Corrupt(_)))
    ));
    let (read, _) = OwnedFileCapture::current(&d.0, capture(&d.0))
        .unwrap()
        .admit(ResidencyPolicy::Demand)
        .unwrap();
    assert_eq!(
        read.file_checkpoint().unwrap(),
        env.file_checkpoint().unwrap()
    );
    assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
    assert_eq!(
        fs::read(d.0.join("PREVIOUS")).unwrap(),
        b"bad stable predecessor"
    );
}
#[test]
fn graph_and_residency_failure_never_expose_a_reader_or_bypass_full_audit() {
    use ulmdb_storage::{Arena, SegmentTable, WriteStore};
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        drop(env);
        let old = ulmdb_storage::read_current(&d.0).unwrap();
        let table = SegmentTable::load(&d.0, &old.segments).unwrap();
        let mut descriptor = ulmdb_kv::get(&table, old.catalog, b"kv")
            .unwrap()
            .unwrap()
            .to_vec();
        let mut arena = Arena::new(table, old.next_segment).unwrap();
        let invalid = arena.allocate(b"not a B+tree", 8).unwrap();
        descriptor[16..32].copy_from_slice(&invalid.encode());
        let mut bad = old.clone();
        bad.catalog = ulmdb_kv::put(&mut arena, old.catalog, b"kv", &descriptor).unwrap();
        bad.segments = arena.seal(&d.0).unwrap().0.entries().into();
        bad.next_segment += 1;
        bad.previous = old.generation;
        bad.parent = old.commit;
        bad.sequence += 1;
        bad.generation = ulmdb_storage::new_identity().unwrap();
        bad.commit = bad.generation;
        if bad.format_minor == 4 {
            bad.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::publish(&d.0, &bad).unwrap();
        let baseline = pins(&d.0);
        let frame = OwnedFileCapture::current(&d.0, capture(&d.0)).unwrap();
        assert_eq!(frame.view_sequence(), 3);
        assert!(matches!(
            frame.admit(ResidencyPolicy::Demand),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(pins(&d.0), baseline);
        let frame = OwnedFileCapture::previous(&d.0, capture(&d.0)).unwrap();
        assert!(matches!(
            frame
                .select_selected()
                .unwrap()
                .admit(ResidencyPolicy::Demand),
            Err(Error::Corrupt(_))
        ));
        let (read, _) = OwnedFileCapture::previous(&d.0, capture(&d.0))
            .unwrap()
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
        drop(read);
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let frame = OwnedFileCapture::previous(&d.0, capture(&d.0)).unwrap();
        assert!(matches!(
            frame.admit(ResidencyPolicy::Prefault { max_bytes: 0 }),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(pins(&d.0), baseline);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
    }
}
#[test]
fn view_switches_and_genesis_reuse_stay_within_one_live_pin_quota() {
    for mode in MODES {
        for genesis in [false, true] {
            let d = Directory::new();
            if genesis {
                drop(Environment::create_with_publication(&d.0, mode).unwrap());
            } else {
                drop(seed(&d.0, mode));
            }
            let limits = Limits {
                max_pin_records: 1,
                ..Default::default()
            };
            let uuid = ulmdb_storage::read_current(&d.0).unwrap().uuid;
            let coordinator = Coordination::open(&d.0, uuid, limits).unwrap();
            {
                let mut maintenance = coordinator.try_maintenance().unwrap();
                maintenance.scan().unwrap().cleanup_stale().unwrap();
            }
            let deny_second = || {
                let cap = Coordination::capture_directory(&d.0, limits).unwrap();
                assert!(matches!(
                    OwnedFileCapture::current(&d.0, cap),
                    Err(Error::LimitExceeded)
                ));
                assert_eq!(pins(&d.0), 1);
            };
            let cap = Coordination::capture_directory(&d.0, limits).unwrap();
            let frame = OwnedFileCapture::current(&d.0, cap).unwrap();
            assert_eq!(pins(&d.0), 1);
            deny_second();
            let frame = frame.select_previous().unwrap();
            assert_eq!(pins(&d.0), 1);
            let frame = frame.select_selected().unwrap();
            assert_eq!(pins(&d.0), 1);
            let frame = frame.select_previous().unwrap();
            let (read, _) = frame.admit(ResidencyPolicy::Demand).unwrap();
            assert_eq!(pins(&d.0), 1);
            assert_eq!(read.sequence(), if genesis { 0 } else { 1 });
            if !genesis {
                assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
            }
            deny_second();
            drop(read);
            assert_eq!(pins(&d.0), 0);
            let mut maintenance = coordinator.try_maintenance().unwrap();
            let scan = maintenance.scan().unwrap();
            assert!(scan.live_roots().is_empty());
            assert_eq!(scan.stale_records(), 0);
        }
    }
}
#[test]
fn pending_session_checkpoint_and_floor_are_safe_while_writer_and_prepared_guard_exist() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let b = env.file_checkpoint().unwrap();
        drop(env);
        let session = PreviousSession::open(&d.0).unwrap();
        let a = session.begin_read().file_checkpoint().unwrap();
        assert_eq!(session.selected_checkpoint().unwrap(), b);
        assert_eq!(session.file_checkpoint().unwrap(), a);
        let floor = session.database_id_floor();
        session.raise_database_id_floor(floor + 100).unwrap();
        assert_eq!(session.database_id_floor(), floor + 100);
        let mut w = session.begin_write().unwrap();
        assert_eq!(session.selected_checkpoint().unwrap(), b);
        assert!(matches!(
            session.raise_database_id_floor(floor + 200),
            Err(Error::Busy)
        ));
        w.create_database(b"new", DatabaseKind::Kv).unwrap();
        assert_eq!(w.open_database(b"new").unwrap().id(), floor + 100);
        let p = w
            .prepare_for_commit_with_intent(CommitIntent::ForcePublication)
            .unwrap();
        assert_eq!(session.selected_checkpoint().unwrap(), b);
        assert!(matches!(
            session.raise_database_id_floor(floor + 200),
            Err(Error::Busy)
        ));
        p.commit().unwrap();
        assert!(!session.is_previous());
        let c = session.file_checkpoint().unwrap();
        assert_ne!(c, b);
        assert_eq!(session.selected_checkpoint().unwrap(), c);
        assert_eq!(
            ulmdb_storage::checkpoint_bytes(&ulmdb_storage::read_current(&d.0).unwrap()).unwrap(),
            c
        );
        let read = session.begin_read();
        assert_eq!(
            read.file_manifest().unwrap().next_database,
            session.database_id_floor()
        );
    }
}
#[test]
fn ordinary_commit_preparation_preserves_noop_force_and_range_restrictions() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let b = env.file_checkpoint().unwrap();
        let p = env.begin_write().unwrap().prepare_for_commit().unwrap();
        assert!(!p.will_publish());
        assert_eq!(p.commit().unwrap(), 2);
        assert_eq!(env.file_checkpoint().unwrap(), b);
        let p = env
            .begin_write()
            .unwrap()
            .prepare_for_commit_with_intent(CommitIntent::ForcePublication)
            .unwrap();
        assert!(p.will_publish());
        assert_eq!(env.file_checkpoint().unwrap(), b);
        assert_eq!(p.commit().unwrap(), 3);
        assert_ne!(env.file_checkpoint().unwrap(), b);
    }
    for mode in [PublicationMode::RangeV5]
        .into_iter()
        .chain(cfg!(feature = "range-lifecycle").then_some(PublicationMode::Range))
    {
        let d = Directory::new();
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        assert!(matches!(env.file_checkpoint(), Err(Error::InvalidInput(_))));
        assert!(matches!(
            env.begin_read().file_checkpoint(),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            env.begin_read().file_manifest(),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            env.begin_write().unwrap().prepare_for_commit(),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            OwnedFileCapture::current(&d.0, capture(&d.0)),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(env.begin_write().unwrap().commit().unwrap(), 0);
    }
}
#[test]
fn namespace_wrapper_reuses_only_pin_binding_and_never_turns_lazy_read_into_graph_certificate() {
    use ulmdb::{CoordinatedEnvironment, ReadTxn};
    use ulmdb_storage::{Arena, SegmentTable, WriteStore};
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let pure = ReadTxn::open_snapshot(&d.0).unwrap();
        assert!(
            CoordinatedEnvironment::from_admitted_file_read(&d.0, &pure, ResidencyPolicy::Demand)
                .is_err()
        );
        let valid = env.begin_read();
        assert!(matches!(
            CoordinatedEnvironment::from_admitted_file_read(
                &d.0,
                &valid,
                ResidencyPolicy::Prefault { max_bytes: 0 }
            ),
            Err(Error::InvalidInput(_))
        ));
        let clone = Directory::new();
        ulmdb::backup::export(&env, &clone.0).unwrap();
        assert!(
            CoordinatedEnvironment::from_admitted_file_read(
                &clone.0,
                &valid,
                ResidencyPolicy::Demand
            )
            .is_err()
        );
        drop(valid);
        drop(pure);
        drop(env);
        let old = ulmdb_storage::read_current(&d.0).unwrap();
        let table = SegmentTable::load(&d.0, &old.segments).unwrap();
        let mut descriptor = ulmdb_kv::get(&table, old.catalog, b"kv")
            .unwrap()
            .unwrap()
            .to_vec();
        let mut arena = Arena::new(table, old.next_segment).unwrap();
        let invalid = arena.allocate(b"hash-valid invalid user tree", 8).unwrap();
        descriptor[16..32].copy_from_slice(&invalid.encode());
        let mut bad = old.clone();
        bad.catalog = ulmdb_kv::put(&mut arena, old.catalog, b"kv", &descriptor).unwrap();
        bad.segments = arena.seal(&d.0).unwrap().0.entries().into();
        bad.next_segment += 1;
        bad.previous = old.generation;
        bad.parent = old.commit;
        bad.sequence += 1;
        bad.generation = ulmdb_storage::new_identity().unwrap();
        bad.commit = bad.generation;
        if bad.format_minor == 4 {
            bad.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::publish(&d.0, &bad).unwrap();
        let (read, _) = OwnedFileCapture::previous(&d.0, capture(&d.0))
            .unwrap()
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        let wrapper =
            CoordinatedEnvironment::from_admitted_file_read(&d.0, &read, ResidencyPolicy::Demand)
                .unwrap();
        assert!(matches!(wrapper.begin_read(), Err(Error::Corrupt(_))));
        let lazy = Environment::open(&d.0).unwrap();
        let wrapper = CoordinatedEnvironment::from_admitted_file_read(
            &d.0,
            &lazy.begin_read(),
            ResidencyPolicy::Demand,
        )
        .unwrap();
        assert!(matches!(wrapper.begin_read(), Err(Error::Corrupt(_))));
        assert!(ulmdb::backup::verify(&lazy).is_err());
        let target = Directory::new();
        assert!(ulmdb::backup::export(&lazy, &target.0).is_err());
        assert!(!target.0.exists());
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
    }
}
#[test]
fn current_replica_capture_is_readable_but_previous_and_range_authorities_stay_restricted() {
    use ulmdb::replication::{self, Cursor, PublisherKey, TransferLimits};
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let read = env.begin_read();
        let cursor = Cursor::from_read(&read).unwrap();
        let key = PublisherKey::new(13, cursor.uuid, cursor.epoch, [0x55; 32]).unwrap();
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
        let replica = replication::bootstrap(&frame, &target.0).unwrap();
        let current = OwnedFileCapture::current(&target.0, capture(&target.0)).unwrap();
        assert!(matches!(
            current.select_previous(),
            Err(PreviousError::Engine(Error::ReadOnly))
        ));
        assert!(matches!(
            OwnedFileCapture::previous(&target.0, capture(&target.0)),
            Err(PreviousError::Engine(Error::ReadOnly))
        ));
        let (read, _) = OwnedFileCapture::current(&target.0, capture(&target.0))
            .unwrap()
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
        let wrapper = ulmdb::CoordinatedEnvironment::from_admitted_file_read(
            &target.0,
            &read,
            ResidencyPolicy::Demand,
        )
        .unwrap();
        assert_eq!(
            wrapper.begin_read().unwrap().sequence(),
            replica.begin_read().sequence()
        );
    }
}
#[test]
fn directory_replacement_revokes_pending_admission_before_any_reader_is_exposed() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let frame = OwnedFileCapture::current(&d.0, capture(&d.0)).unwrap();
        let retired = d.0.with_extension("retired");
        fs::rename(&d.0, &retired).unwrap();
        ulmdb_storage::create_layout(&d.0).unwrap();
        assert!(matches!(
            frame.admit(ResidencyPolicy::Demand),
            Err(Error::Corrupt(_))
        ));
        assert!(!d.0.join("CURRENT").exists());
        drop(env);
        fs::remove_dir_all(retired).unwrap();
    }
}
#[test]
fn immutable_metadata_getters_track_real_legacy_upgrade_without_descriptor_rediscovery() {
    let d = Directory::new();
    ulmdb_storage::create_layout(&d.0).unwrap();
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(d.0.join("LOCK"))
        .unwrap();
    let fence = ulmdb_storage::WriterFence::try_lock(lock).unwrap();
    let mut v1 = ulmdb_storage::Manifest::empty().unwrap();
    v1.format_minor = 1;
    ulmdb_storage::prepare_publication(&d.0, &v1)
        .unwrap()
        .publish_bootstrap()
        .unwrap();
    drop(fence);
    let env = Environment::open(&d.0).unwrap();
    let old = env.begin_read();
    assert_eq!(old.file_manifest().unwrap().format_minor, 1);
    let before = old.file_checkpoint().unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"upgraded").unwrap();
    let prepared = w.prepare_for_commit().unwrap();
    assert_eq!(env.file_checkpoint().unwrap(), before);
    prepared.commit().unwrap();
    let current = env.begin_read();
    assert_eq!(current.file_manifest().unwrap().format_minor, 2);
    assert_ne!(current.file_checkpoint().unwrap(), before);
    let saved = d.0.join("CURRENT.saved");
    fs::rename(d.0.join("CURRENT"), &saved).unwrap();
    assert_eq!(
        env.file_checkpoint().unwrap(),
        current.file_checkpoint().unwrap()
    );
    assert_eq!(current.file_manifest().unwrap().format_minor, 2);
    assert_eq!(old.file_manifest().unwrap().format_minor, 1);
    fs::rename(saved, d.0.join("CURRENT")).unwrap();
}
