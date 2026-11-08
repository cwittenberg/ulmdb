#![cfg(target_os = "linux")]
use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{CoordinatedEnvironment, DatabaseKind, Environment, Error, PublicationMode, ReadTxn};
#[path = "support/pin_records.rs"]
mod pin_records;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-physical-negative-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(path: &Path, mode: PublicationMode) -> Environment {
    seed_with_maintenance(path, mode, ulmdb::MaintenancePolicy::AUTOMATIC)
}
fn seed_with_maintenance(
    path: &Path,
    mode: PublicationMode,
    policy: ulmdb::MaintenancePolicy,
) -> Environment {
    let env = Environment::create_with_publication(path, mode).unwrap();
    env.set_maintenance_policy(policy).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"records", DatabaseKind::Kv).unwrap();
    write.put(b"records", b"key", b"original").unwrap();
    write.commit().unwrap();
    env
}
fn orphan(path: &Path) -> PathBuf {
    let file = path.join("commits").join(format!(
        "{:032x}.root",
        ulmdb_storage::new_identity().unwrap()
    ));
    fs::write(&file, b"unselected orphan").unwrap();
    file
}
fn registry(path: &Path) -> PathBuf {
    path.join(".ulmdb-coordination/pins")
}

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
fn case_pass(test: &str, mode: PublicationMode) {
    let backend = match mode {
        PublicationMode::Legacy => "file-legacy",
        PublicationMode::Cohort => "file-cohort",
        PublicationMode::Indexed => "file-indexed",
        _ => unreachable!(),
    };
    println!("CASE_PASS test={test} backend={backend}");
}

#[test]
fn malformed_live_physical_pin_stops_reclaim_before_any_unlink() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let directory = Directory::new();
        let env = seed(&directory.0, mode);
        let live = pin_records::unique_live_ordinary(&directory.0);
        let original = fs::read(&live).unwrap();
        let manifest = ulmdb_storage::read_current(&directory.0).unwrap();
        let coordination = ulmdb_storage::coordination::Coordination::open(
            &directory.0,
            manifest.uuid,
            Default::default(),
        )
        .unwrap();
        let stale_pin = coordination.try_capture().unwrap().pin_current().unwrap();
        let stale = stale_pin.record_path().to_owned();
        {
            // A genuine record dropped while the scanner owns Registry stays
            // stale, with correct generation reservations and authentic bytes.
            let mut maintenance = coordination.try_maintenance().unwrap();
            let scan = maintenance.scan().unwrap();
            assert_eq!(scan.live_roots().len(), 2);
            drop(stale_pin);
            drop(scan);
        }
        let mut damaged = original.clone();
        damaged[0] ^= 1;
        fs::write(&live, damaged).unwrap();
        let orphan = orphan(&directory.0);
        assert!(matches!(env.reclaim(), Err(Error::Corrupt(_))));
        assert!(orphan.exists());
        assert!(stale.exists());
        fs::write(&live, original).unwrap();
        let reclaimed = env.reclaim().unwrap();
        assert!(reclaimed.removed_descriptors >= 1);
        assert!(!orphan.exists());
        assert!(!stale.exists());
        assert_eq!(
            env.begin_read().get(b"records", b"key").unwrap(),
            Some(b"original".as_slice())
        );
        case_pass(
            "malformed_live_physical_pin_stops_reclaim_before_any_unlink",
            mode,
        );
    }
}

#[test]
fn capture_window_excludes_native_deletion_without_blocking_on_itself() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory.0, mode);
        let current = ulmdb_storage::read_current(&directory.0).unwrap();
        let coordination = ulmdb_storage::coordination::Coordination::open(
            &directory.0,
            current.uuid,
            Default::default(),
        )
        .unwrap();
        let capture = coordination.try_capture().unwrap();
        let orphan = orphan(&directory.0);
        assert!(matches!(env.reclaim(), Err(Error::Busy)));
        assert!(orphan.exists());
        drop(capture);
        env.reclaim().unwrap();
        assert!(!orphan.exists());
        case_pass(
            "capture_window_excludes_native_deletion_without_blocking_on_itself",
            mode,
        );
    }
}

#[test]
fn exhausted_physical_registry_fails_before_publication_then_recovers() {
    for mode in MODES {
        let directory = Directory::new();
        // Isolate the compatibility flat-registry admission bound: automatic
        // generation negotiation deliberately gives stale rows another budget.
        let env = seed_with_maintenance(
            &directory.0,
            mode,
            ulmdb::MaintenancePolicy::Disabled,
        );
        let before = fs::read(directory.0.join("CURRENT")).unwrap();
        // One live pin belongs to CURRENT; fill the remaining default admission
        // slots with real stale files. Maintenance can still scan exactly this cap.
        let limit = ulmdb_storage::coordination::Limits::default().max_pin_records;
        assert_eq!(pin_records::paths(&directory.0).len(), 1);
        for serial in 1..limit {
            let path = registry(&directory.0).join(format!("pin-{serial:032x}.lease"));
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .unwrap();
        }
        let mut write = env.begin_write().unwrap();
        write.put(b"records", b"key", b"unpublished").unwrap();
        assert!(matches!(write.commit(), Err(Error::LimitExceeded)));
        assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), before);
        assert_eq!(
            env.begin_read().get(b"records", b"key").unwrap(),
            Some(b"original".as_slice())
        );
        env.reclaim().unwrap();
        assert_eq!(pin_records::paths(&directory.0).len(), 1);
        let mut write = env.begin_write().unwrap();
        write.put(b"records", b"key", b"retry").unwrap();
        write.commit().unwrap();
        assert_eq!(
            env.begin_read().get(b"records", b"key").unwrap(),
            Some(b"retry".as_slice())
        );
        case_pass(
            "exhausted_physical_registry_fails_before_publication_then_recovers",
            mode,
        );
    }
}

#[test]
fn physical_export_rejects_a_copied_environment_with_identical_uuid_and_root() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory.0, mode);
        let read = env.begin_read();
        let copied = directory.0.join("copy");
        let target = directory.0.join("wrong-source-export");
        ulmdb::backup::export(&env, &copied).unwrap();
        let original = ulmdb_storage::read_current(&directory.0).unwrap();
        let duplicate = ulmdb_storage::read_current(&copied).unwrap();
        assert_eq!(original.uuid, duplicate.uuid);
        assert_eq!(
            ulmdb_storage::checkpoint_bytes(&original).unwrap(),
            ulmdb_storage::checkpoint_bytes(&duplicate).unwrap()
        );
        assert!(matches!(
            ulmdb::backup::export_independent_snapshot(&copied, &read, &target),
            Err(Error::InvalidInput(_))
        ));
        assert!(!target.exists());
        case_pass(
            "physical_export_rejects_a_copied_environment_with_identical_uuid_and_root",
            mode,
        );
    }
}

#[test]
fn selected_head_drift_poisoning_precedes_a_corrupt_physical_pin() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory.0, mode);
        let old = fs::read(directory.0.join("CURRENT")).unwrap();
        let mut write = env.begin_write().unwrap();
        write.put(b"records", b"key", b"latest").unwrap();
        write.commit().unwrap();
        let live = pin_records::unique_live_ordinary(&directory.0);
        let mut bytes = fs::read(&live).unwrap();
        bytes[0] ^= 1;
        fs::write(&live, bytes).unwrap();
        fs::write(directory.0.join("CURRENT"), old).unwrap();
        let orphan = orphan(&directory.0);
        assert!(matches!(env.reclaim(), Err(Error::OutcomeUnknown)));
        assert!(orphan.exists());
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        case_pass(
            "selected_head_drift_poisoning_precedes_a_corrupt_physical_pin",
            mode,
        );
    }
}

#[test]
fn coordinated_capture_rejects_damaged_publication_without_repair() {
    let replace = |path: &Path, bytes: &[u8]| {
        // Immutable segments are read-only. Replace the fixture inode instead
        // of weakening its permissions or changing an existing reader's map.
        let temporary = path.with_file_name(format!(
            ".corruption-control-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::write(&temporary, bytes).unwrap();
        fs::rename(temporary, path).unwrap();
    };
    for mode in MODES {
        let directory = Directory::new();
        let _env = seed(&directory.0, mode);
        let wrapper = CoordinatedEnvironment::open(&directory.0).unwrap();
        let manifest = ulmdb_storage::read_current(&directory.0).unwrap();
        let pin_count = pin_records::paths(&directory.0).len();
        let head = directory.0.join("CURRENT");
        let selected = fs::read(&head).unwrap();
        let paths = [
            head.clone(),
            ulmdb_storage::descriptor_path(&directory.0, &manifest),
            ulmdb_storage::segment_path(&directory.0, &manifest.segments[0]),
        ];
        for path in paths {
            let original = fs::read(&path).unwrap();
            let mut damaged = original.clone();
            damaged[0] ^= 1;
            replace(&path, &damaged);
            assert!(ReadTxn::open_coordinated_snapshot(&directory.0).is_err());
            assert!(CoordinatedEnvironment::open_with_initial_read(&directory.0).is_err());
            assert!(wrapper.begin_read().is_err());
            assert_eq!(
                pin_records::paths(&directory.0).len(),
                pin_count
            );
            assert_eq!(fs::read(&path).unwrap(), damaged);
            if path != head {
                assert_eq!(fs::read(&head).unwrap(), selected);
            }
            replace(&path, &original);
            assert_eq!(
                wrapper
                    .begin_read()
                    .unwrap()
                    .get(b"records", b"key")
                    .unwrap(),
                Some(b"original".as_slice())
            );
        }
        case_pass(
            "coordinated_capture_rejects_damaged_publication_without_repair",
            mode,
        );
    }
}

#[test]
fn valid_physical_pin_does_not_bypass_logical_graph_validation() {
    use ulmdb_storage::{Arena, SegmentTable, WriteStore};
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory.0, mode);
        let wrapper = CoordinatedEnvironment::open(&directory.0).unwrap();
        drop(env);
        let old = ulmdb_storage::read_current(&directory.0).unwrap();
        let table = SegmentTable::load(&directory.0, &old.segments).unwrap();
        let mut database = ulmdb_kv::get(&table, old.catalog, b"records")
            .unwrap()
            .unwrap()
            .to_vec();
        let mut arena = Arena::new(table, old.next_segment).unwrap();
        // Keep catalog encoding and every physical checksum valid, but make the
        // database's tree point to an object that is not a B+tree page. Catalog
        // loading alone accepts this; the complete logical audit must reject it.
        let invalid_tree = arena.allocate(b"not a B+tree page", 8).unwrap();
        database[16..32].copy_from_slice(&invalid_tree.encode());
        let mut forged = old.clone();
        forged.catalog = ulmdb_kv::put(&mut arena, old.catalog, b"records", &database).unwrap();
        forged.segments = arena.seal(&directory.0).unwrap().0.entries().into();
        forged.next_segment += 1;
        forged.generation = ulmdb_storage::new_identity().unwrap();
        forged.sequence += 1;
        forged.commit = forged.generation;
        forged.parent = old.commit;
        forged.previous = old.generation;
        if mode == PublicationMode::Indexed {
            forged.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::publish(&directory.0, &forged).unwrap();
        let selected = fs::read(directory.0.join("CURRENT")).unwrap();
        let coordination = ulmdb_storage::coordination::Coordination::open(
            &directory.0,
            forged.uuid,
            Default::default(),
        )
        .unwrap();
        let capture = coordination.try_capture().unwrap();
        let pin = capture.pin_current().unwrap();
        assert!(pin.matches_environment(&directory.0, &forged).unwrap());
        drop(capture);
        let pin_count = pin_records::paths(&directory.0).len();
        assert!(matches!(wrapper.begin_read(), Err(Error::Corrupt(_))));
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot(&directory.0),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            CoordinatedEnvironment::open_with_initial_read(&directory.0),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), selected);
        assert_eq!(
            pin_records::paths(&directory.0).len(),
            pin_count
        );
        assert!(coordination.try_maintenance().is_ok());
        case_pass(
            "valid_physical_pin_does_not_bypass_logical_graph_validation",
            mode,
        );
    }
}

#[test]
fn failed_residency_admission_releases_pin_and_capture() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory.0, mode);
        let selected = fs::read(directory.0.join("CURRENT")).unwrap();
        let pin_count = pin_records::paths(&directory.0).len();
        assert!(matches!(
            CoordinatedEnvironment::open_with_residency(
                &directory.0,
                ulmdb::ResidencyPolicy::Prefault { max_bytes: 0 },
            ),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(
            pin_records::paths(&directory.0).len(),
            pin_count
        );
        assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), selected);
        // Neither a leaked shared capture nor a leaked physical root may remain.
        env.reclaim().unwrap();
        let reopened = CoordinatedEnvironment::open(&directory.0).unwrap();
        assert_eq!(
            reopened
                .begin_read()
                .unwrap()
                .get(b"records", b"key")
                .unwrap(),
            Some(b"original".as_slice())
        );
        case_pass("failed_residency_admission_releases_pin_and_capture", mode);
    }
}
