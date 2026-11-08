#![cfg(target_os = "linux")]
//! Explicit fsync behavior on named committed roots, never a power-loss test.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, ReadTxn};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "file-sync-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(p: &Path, mode: PublicationMode) -> Environment {
    let e = Environment::create_with_publication(p, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"key", b"B").unwrap();
    w.commit().unwrap();
    e
}
fn selectors(p: &Path) -> (Vec<u8>, Option<Vec<u8>>) {
    (
        fs::read(p.join("CURRENT")).unwrap(),
        fs::read(p.join("PREVIOUS")).ok(),
    )
}
fn pins(p: &Path) -> usize {
    fs::read_dir(p.join(".ulmdb-coordination/pins"))
        .unwrap()
        .count()
}
fn assets(p: &Path) -> BTreeSet<PathBuf> {
    ["segments", "commits", "cohorts"]
        .into_iter()
        .flat_map(|name| {
            fs::read_dir(p.join(name))
                .into_iter()
                .flatten()
                .map(|x| x.unwrap().path())
        })
        .filter(|p| p.is_file())
        .collect()
}
fn child(p: &Path, role: &str) {
    let out = p.with_extension(format!("{role}.stdout"));
    let err = p.with_extension(format!("{role}.stderr"));
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "file_sync_worker", "--nocapture"])
        .env("ULMDB_SYNC_PATH", p)
        .env("ULMDB_SYNC_ROLE", role)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_PAUSE_AT")
        .env_remove("ULMDB_TRACE_FILE")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&out).unwrap())
        .stderr(fs::File::create(&err).unwrap());
    let mut c = c.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            panic!(
                "same-thread sync worker timed out: {}\n{}",
                fs::read_to_string(&out).unwrap(),
                fs::read_to_string(&err).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let stdout = fs::read_to_string(&out).unwrap();
    let stderr = fs::read_to_string(&err).unwrap();
    assert!(status.success(), "{role} {status}\n{stdout}\n{stderr}");
    assert!(stdout.contains("SYNC_WORKER_PASS"));
    for line in stdout.lines() {
        println!("WORKER {role} {line}");
    }
    let _ = fs::remove_file(out);
    let _ = fs::remove_file(err);
}
#[test]
fn file_sync_worker() {
    let Some(p) = std::env::var_os("ULMDB_SYNC_PATH") else {
        return;
    };
    let p = PathBuf::from(p);
    let e = Environment::open(&p).unwrap();
    let head = selectors(&p);
    let checkpoint = e.file_checkpoint().unwrap();
    let pin_count = pins(&p);
    let fd_count = fs::read_dir("/proc/self/fd").unwrap().count();
    match std::env::var("ULMDB_SYNC_ROLE").unwrap().as_str() {
        "writer-states" => {
            let w = e.begin_write().unwrap();
            assert_eq!(e.sync().unwrap().selected_checkpoint, checkpoint);
            w.abort();
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"key", b"C").unwrap();
            assert_eq!(e.sync().unwrap().selected_checkpoint, checkpoint);
            assert_eq!(w.get(b"kv", b"key").unwrap(), Some(b"C".as_slice()));
            assert_eq!(selectors(&p), head);
            w.abort();
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"key", b"D").unwrap();
            let prepared = w.prepare().unwrap();
            let prepared_pins = pins(&p);
            assert_eq!(e.sync().unwrap().selected_checkpoint, checkpoint);
            assert_eq!(pins(&p), prepared_pins);
            assert_eq!(selectors(&p), head);
            assert_eq!(
                e.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"B".as_slice())
            );
            prepared.commit().unwrap();
            assert_eq!(
                e.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"D".as_slice())
            );
            e.sync().unwrap();
        }
        "resources" => {
            let initial = e.sync().unwrap();
            for _ in 0..32 {
                let report = e.sync().unwrap();
                assert_eq!(report.selected_checkpoint, checkpoint);
                assert_eq!(report.previous_checkpoint, initial.previous_checkpoint);
                assert_eq!(
                    (
                        report.files_synced,
                        report.directories_synced,
                        report.file_bytes_covered
                    ),
                    (
                        initial.files_synced,
                        initial.directories_synced,
                        initial.file_bytes_covered
                    )
                );
                assert_eq!(pins(&p), pin_count);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fd_count);
            }
            assert_eq!(selectors(&p), head);
            println!(
                "RESOURCE pins_before={pin_count} pins_after={} fds_before={fd_count} fds_after={} files={} dirs={} bytes={}",
                pins(&p),
                fs::read_dir("/proc/self/fd").unwrap().count(),
                initial.files_synced,
                initial.directories_synced,
                initial.file_bytes_covered
            );
        }
        other => panic!("unknown role {other}"),
    }
    println!("SYNC_WORKER_PASS");
}
#[test]
fn same_thread_clean_dirty_and_prepared_writers_remain_usable_without_deadlock() {
    for mode in MODES {
        let d = Directory::new();
        drop(seed(&d.0, mode));
        child(&d.0, "writer-states");
        let e = Environment::open(&d.0).unwrap();
        assert_eq!(
            e.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"D".as_slice())
        );
    }
}
#[test]
fn repeated_sync_has_no_pin_fd_or_selector_growth() {
    for mode in MODES {
        let d = Directory::new();
        drop(seed(&d.0, mode));
        child(&d.0, "resources");
    }
}
#[test]
fn coordinated_sync_uses_actual_namespace_without_taking_owner_writer_lock() {
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let coordinated = ulmdb::CoordinatedEnvironment::open(&d.0).unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        let prepared = w.prepare().unwrap();
        let before = pins(&d.0);
        assert_eq!(
            coordinated.sync().unwrap().selected_checkpoint,
            e.file_checkpoint().unwrap()
        );
        assert_eq!(pins(&d.0), before);
        prepared.commit().unwrap();
        assert_eq!(
            coordinated.sync().unwrap().selected_checkpoint,
            e.file_checkpoint().unwrap()
        );
    }
}
#[test]
fn hidden_unselected_prepared_assets_are_never_opened_or_included() {
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let head = selectors(&d.0);
        let old = e.begin_read();
        let baseline = e.sync().unwrap();
        let before = assets(&d.0);
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        let prepared = w.prepare().unwrap();
        let pin_count = pins(&d.0);
        let added = assets(&d.0)
            .difference(&before)
            .cloned()
            .collect::<Vec<_>>();
        assert!(!added.is_empty());
        for p in &added {
            fs::rename(p, p.with_extension("hidden-under-test")).unwrap();
            fs::create_dir(p).unwrap();
        }
        let report = e.sync().unwrap();
        assert_eq!(report.selected_checkpoint, baseline.selected_checkpoint);
        assert_eq!(report.previous_checkpoint, baseline.previous_checkpoint);
        assert_eq!(
            (
                report.files_synced,
                report.directories_synced,
                report.file_bytes_covered
            ),
            (
                baseline.files_synced,
                baseline.directories_synced,
                baseline.file_bytes_covered
            )
        );
        assert_eq!(pins(&d.0), pin_count);
        assert_eq!(selectors(&d.0), head);
        for p in &added {
            fs::remove_dir(p).unwrap();
            fs::rename(p.with_extension("hidden-under-test"), p).unwrap();
        }
        prepared.commit().unwrap();
        assert_eq!(old.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
        assert_eq!(
            e.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"C".as_slice())
        );
        assert_ne!(
            e.sync().unwrap().selected_checkpoint,
            baseline.selected_checkpoint
        );
    }
}
#[test]
fn namespace_replacement_and_symlink_assets_fail_without_poison_or_repair() {
    for mode in MODES {
        let d = Directory::new();
        let displaced = Directory::new();
        let e = seed(&d.0, mode);
        let head = selectors(&d.0);
        fs::rename(&d.0, &displaced.0).unwrap();
        let other = Environment::create_with_publication(&d.0, mode).unwrap();
        let replacement = selectors(&d.0);
        assert!(e.sync().is_err());
        assert_eq!(selectors(&d.0), replacement);
        drop(other);
        fs::remove_dir_all(&d.0).unwrap();
        fs::rename(&displaced.0, &d.0).unwrap();
        assert_eq!(selectors(&d.0), head);
        let manifest = ulmdb_storage::read_current(&d.0).unwrap();
        let descriptor = ulmdb_storage::descriptor_path(&d.0, &manifest);
        let saved = descriptor.with_extension("saved");
        fs::rename(&descriptor, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &descriptor).unwrap();
        assert!(e.sync().is_err());
        assert_eq!(selectors(&d.0), head);
        fs::remove_file(&descriptor).unwrap();
        fs::rename(saved, descriptor).unwrap();
        e.sync().unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"usable").unwrap();
        w.commit().unwrap();
    }
    let missing = Directory::new();
    assert!(ulmdb::sync_file_environment(&missing.0).is_err());
    assert!(!missing.0.exists());
}
#[test]
fn hash_valid_unrelated_legacy_previous_is_rejected_without_repair() {
    let d = Directory::new();
    let e = seed(&d.0, PublicationMode::Legacy);
    let head = selectors(&d.0);
    let selected = e.file_checkpoint().unwrap();
    fs::write(d.0.join("PREVIOUS"), selected).unwrap();
    assert!(matches!(e.sync(), Err(Error::Corrupt(_))));
    assert_eq!(fs::read(d.0.join("PREVIOUS")).unwrap(), selected);
    assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head.0);
    fs::write(d.0.join("PREVIOUS"), head.1.unwrap()).unwrap();
    e.sync().unwrap();
    e.begin_write().unwrap().abort();
}
#[test]
fn absent_legacy_history_is_valid_but_present_history_binds_realm_and_epoch() {
    let d = Directory::new();
    let e = seed(&d.0, PublicationMode::Legacy);
    let head = selectors(&d.0);
    let (_, previous) = ulmdb_storage::read_roots(&d.0).unwrap();
    let previous = previous.unwrap();
    let descriptor = ulmdb_storage::descriptor_path(&d.0, &previous);
    let original = fs::read(&descriptor).unwrap();
    fs::remove_file(d.0.join("PREVIOUS")).unwrap();
    let report = e.sync().unwrap();
    assert!(report.previous_checkpoint.is_none());
    assert_eq!(report.selected_checkpoint, e.file_checkpoint().unwrap());
    assert!(!d.0.join("PREVIOUS").exists());
    for foreign_epoch in [false, true] {
        let mut bad = previous.clone();
        if foreign_epoch {
            bad.epoch += 1;
        } else {
            bad.uuid = [0x6b; 16];
        }
        fs::remove_file(&descriptor).unwrap();
        drop(ulmdb_storage::prepare_publication(&d.0, &bad).unwrap());
        let encoded = fs::read(&descriptor).unwrap();
        let checkpoint = ulmdb_storage::checkpoint_bytes(&bad).unwrap();
        fs::write(d.0.join("PREVIOUS"), checkpoint).unwrap();
        assert!(matches!(e.sync(), Err(Error::Corrupt(_))));
        assert_eq!(fs::read(&descriptor).unwrap(), encoded);
        assert_eq!(fs::read(d.0.join("PREVIOUS")).unwrap(), checkpoint);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head.0);
    }
    fs::write(descriptor, original).unwrap();
    fs::write(d.0.join("PREVIOUS"), head.1.unwrap()).unwrap();
    e.sync().unwrap();
    e.begin_write().unwrap().abort();
}
#[test]
fn present_malformed_or_dangling_history_never_becomes_absent_history() {
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let head = selectors(&d.0);
        if mode == PublicationMode::Legacy {
            fs::write(d.0.join("PREVIOUS"), b"bad present pointer").unwrap();
            assert!(e.sync().is_err());
            assert_eq!(
                fs::read(d.0.join("PREVIOUS")).unwrap(),
                b"bad present pointer"
            );
            fs::remove_file(d.0.join("PREVIOUS")).unwrap();
            std::os::unix::fs::symlink(d.0.join("missing-pointer-target"), d.0.join("PREVIOUS"))
                .unwrap();
            assert!(matches!(e.sync(), Err(Error::Corrupt(_))));
            assert!(
                fs::symlink_metadata(d.0.join("PREVIOUS"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            fs::remove_file(d.0.join("PREVIOUS")).unwrap();
            fs::write(d.0.join("PREVIOUS"), head.1.as_ref().unwrap()).unwrap();
        } else {
            let previous = ulmdb_storage::read_roots(&d.0).unwrap().1.unwrap();
            let descriptor = ulmdb_storage::descriptor_path(&d.0, &previous);
            let saved = descriptor.with_extension("saved");
            fs::rename(&descriptor, &saved).unwrap();
            assert!(e.sync().is_err());
            assert!(!descriptor.exists());
            fs::rename(saved, descriptor).unwrap();
            let mut broken = head.0.clone();
            broken[80] ^= 0x40;
            fs::write(d.0.join("CURRENT"), &broken).unwrap();
            assert!(e.sync().is_err());
            assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), broken);
            fs::write(d.0.join("CURRENT"), &head.0).unwrap();
        }
        assert_eq!(selectors(&d.0), head);
        e.sync().unwrap();
        e.begin_write().unwrap().abort();
    }
}
#[test]
fn empty_genesis_flushes_only_its_real_descriptor_and_current_pointer() {
    for mode in MODES {
        let d = Directory::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let head = selectors(&d.0);
        let report = e.sync().unwrap();
        assert_eq!(report.selected_checkpoint, e.file_checkpoint().unwrap());
        assert!(report.previous_checkpoint.is_none());
        assert_eq!(report.files_synced, 2);
        assert_eq!(report.directories_synced, 2);
        let manifest = ulmdb_storage::read_current(&d.0).unwrap();
        assert!(manifest.segments.is_empty());
        assert_eq!(
            report.file_bytes_covered,
            fs::metadata(ulmdb_storage::descriptor_path(&d.0, &manifest))
                .unwrap()
                .len()
                + fs::metadata(d.0.join("CURRENT")).unwrap().len()
        );
        assert_eq!(selectors(&d.0), head);
    }
}
#[test]
fn selected_pointer_damage_fails_without_repair_or_owner_poison() {
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let head = selectors(&d.0);
        let mut damaged = head.0.clone();
        damaged[0] ^= 0x40;
        fs::write(d.0.join("CURRENT"), &damaged).unwrap();
        assert!(matches!(e.sync(), Err(Error::Corrupt(_))));
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), damaged);
        fs::write(d.0.join("CURRENT"), head.0).unwrap();
        e.sync().unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"usable").unwrap();
        w.commit().unwrap();
    }
}
#[test]
fn replicas_and_range_are_explicitly_rejected() {
    use ulmdb::replication::{self, Cursor, PublisherKey, TransferLimits};
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let read = e.begin_read();
        let cursor = Cursor::from_read(&read).unwrap();
        let key = PublisherKey::new(71, cursor.uuid, cursor.epoch, [0x36; 32]).unwrap();
        let mut wire = vec![];
        replication::send_snapshot(&e, &read, None, &key, &mut wire).unwrap();
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
        let head = selectors(&target.0);
        assert!(matches!(replica.sync(), Err(Error::ReadOnly)));
        assert!(matches!(
            ulmdb::sync_file_environment(&target.0),
            Err(Error::ReadOnly)
        ));
        assert_eq!(selectors(&target.0), head);
    }
    for mode in [PublicationMode::RangeV5]
        .into_iter()
        .chain(cfg!(feature = "range-lifecycle").then_some(PublicationMode::Range))
    {
        let d = Directory::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let root_files = || {
            fs::read_dir(&d.0)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.is_file())
                .map(|path| (path.clone(), fs::read(path).unwrap()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let head = root_files();
        assert!(matches!(e.sync(), Err(Error::InvalidInput(_))));
        assert!(matches!(
            ulmdb::sync_file_environment(&d.0),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(root_files(), head);
        e.begin_write().unwrap().abort();
    }
}
#[test]
fn force_sync_is_not_a_logical_graph_audit_or_an_admission_bypass() {
    use ulmdb_storage::{Arena, SegmentTable, WriteStore};
    for mode in MODES {
        let d = Directory::new();
        drop(seed(&d.0, mode));
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
        let head = selectors(&d.0);
        ulmdb::sync_file_environment(&d.0).unwrap();
        assert_eq!(selectors(&d.0), head);
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot(&d.0),
            Err(Error::Corrupt(_))
        ));
        let lazy = Environment::open(&d.0).unwrap();
        lazy.sync().unwrap();
        assert!(ulmdb::backup::verify(&lazy).is_err());
    }
}

#[test]
fn ordinary_pointer_reader_retains_its_symlink_contract_while_sync_rejects_it() {
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let expected = ulmdb_storage::read_current(&d.0).unwrap();
        let named = d.0.join("CURRENT");
        let saved = d.0.join("saved-CURRENT");
        fs::rename(&named, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &named).unwrap();
        let ordinary = ulmdb_storage::read_current(&d.0).unwrap();
        assert_eq!(
            ulmdb_storage::checkpoint_bytes(&ordinary).unwrap(),
            ulmdb_storage::checkpoint_bytes(&expected).unwrap()
        );
        assert!(e.sync().is_err());
        assert!(
            fs::symlink_metadata(&named)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(&named).unwrap();
        fs::rename(saved, named).unwrap();
        e.sync().unwrap();
        e.begin_write().unwrap().abort();
    }
}
