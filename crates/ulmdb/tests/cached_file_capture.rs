#![cfg(target_os = "linux")]
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
use ulmdb::{
    CoordinatedEnvironment, DatabaseKind, Environment, Error, OwnedFileCapture, PublicationMode,
    ResidencyPolicy,
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
            "cached-file-{:032x}",
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
    fs::read_dir(p.join(".ulmdb-coordination/pins"))
        .unwrap()
        .count()
}
fn seed(p: &Path, m: PublicationMode) -> Environment {
    let e = Environment::create_with_publication(p, m).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    w.commit().unwrap();
    e
}
fn current(c: &CoordinatedEnvironment, p: &Path) -> OwnedFileCapture {
    OwnedFileCapture::current(p, c.capture_directory().unwrap()).unwrap()
}
#[test]
fn cached_capture_sees_fresh_checkpoints_and_previous_under_clean_dirty_and_prepared_writer() {
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let old = e.begin_read();
        let a = old.file_checkpoint().unwrap();
        let c = CoordinatedEnvironment::open(&d.0).unwrap();
        let baseline = pins(&d.0);
        for _ in 0..16 {
            let frame = current(&c, &d.0);
            assert_eq!(*frame.selected_checkpoint(), a);
            let (read, _) = frame.admit(ResidencyPolicy::Demand).unwrap();
            assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
            drop(read);
            assert_eq!(pins(&d.0), baseline);
        }
        let clean = e.begin_write().unwrap();
        drop(current(&c, &d.0));
        drop(clean);
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"B").unwrap();
        assert_eq!(*current(&c, &d.0).selected_checkpoint(), a);
        w.commit().unwrap();
        let b = e.file_checkpoint().unwrap();
        assert_ne!(a, b);
        assert_eq!(*current(&c, &d.0).selected_checkpoint(), b);
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"UNSELECTED").unwrap();
        let prepared = w.prepare().unwrap();
        let frame = current(&c, &d.0).select_previous().unwrap();
        assert_eq!(*frame.selected_checkpoint(), b);
        assert_eq!(*frame.view_checkpoint(), a);
        let (read, _) = frame.admit(ResidencyPolicy::Demand).unwrap();
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
        drop(read);
        prepared.abort();
        assert_eq!(e.file_checkpoint().unwrap(), b);
        assert_eq!(old.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
    }
}
#[test]
fn cached_capability_rejects_foreign_directory_and_parent_alias_with_no_pin_leak() {
    for mode in MODES {
        let parent = Directory::new();
        fs::create_dir(&parent.0).unwrap();
        let path = parent.0.join("db");
        let e = seed(&path, mode);
        let c = CoordinatedEnvironment::open(&path).unwrap();
        let clone = Directory::new();
        ulmdb::backup::export(&e, &clone.0).unwrap();
        let before = pins(&path);
        assert!(matches!(
            OwnedFileCapture::current(&clone.0, c.capture_directory().unwrap()),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(pins(&path), before);
        let moved = parent.0.with_extension("moved");
        fs::rename(&parent.0, &moved).unwrap();
        symlink(&moved, &parent.0).unwrap();
        let cap = c.capture_directory().unwrap();
        assert!(matches!(
            OwnedFileCapture::current(&path, cap),
            Err(Error::InvalidInput(_))
        ));
        fs::remove_file(&parent.0).unwrap();
        fs::rename(moved, &parent.0).unwrap();
        assert_eq!(pins(&path), before);
        drop(current(&c, &path));
        assert_eq!(pins(&path), before);
    }
}
#[test]
fn cached_namespace_pin_is_never_a_logical_graph_or_residency_certificate() {
    use ulmdb_storage::{Arena, SegmentTable, WriteStore};
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        let old_read = e.begin_read();
        let c = CoordinatedEnvironment::open(&d.0).unwrap();
        let before = pins(&d.0);
        assert!(
            current(&c, &d.0)
                .admit(ResidencyPolicy::Prefault { max_bytes: 0 })
                .is_err()
        );
        assert_eq!(pins(&d.0), before);
        drop(e);
        let old = ulmdb_storage::read_current(&d.0).unwrap();
        let table = SegmentTable::load(&d.0, &old.segments).unwrap();
        let mut descriptor = ulmdb_kv::get(&table, old.catalog, b"kv")
            .unwrap()
            .unwrap()
            .to_vec();
        let mut arena = Arena::new(table, old.next_segment).unwrap();
        let invalid = arena
            .allocate(b"hash-valid bytes that are not a user tree", 8)
            .unwrap();
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
        let frame = current(&c, &d.0);
        assert_eq!(frame.selected_sequence(), 2);
        assert!(matches!(
            frame.admit(ResidencyPolicy::Demand),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(pins(&d.0), before);
        assert_eq!(old_read.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
    }
}

#[test]
fn admitted_wrapper_reuses_original_two_pin_limit_instead_of_defaulting() {
    use ulmdb_storage::coordination::{Coordination, Limits};
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode);
        e.set_maintenance_policy(ulmdb::MaintenancePolicy::Disabled)
            .unwrap();
        let uuid = e.begin_read().file_manifest().unwrap().uuid;
        drop(e);
        let coordination = Coordination::open(
            &d.0,
            uuid,
            Limits {
                max_pin_records: 2,
                ..Default::default()
            },
        )
        .unwrap();
        {
            let mut maintenance = coordination.try_maintenance().unwrap();
            maintenance.scan().unwrap().cleanup_stale().unwrap();
        }
        let frame =
            OwnedFileCapture::current(&d.0, coordination.capture_bound_directory().unwrap())
                .unwrap();
        let (read, _) = frame.admit(ResidencyPolicy::Demand).unwrap();
        let wrapper =
            CoordinatedEnvironment::from_admitted_file_read(&d.0, &read, ResidencyPolicy::Demand)
                .unwrap();
        let checkpoint = read.file_checkpoint().unwrap();
        let assert_pins = |expected| {
            let mut maintenance = coordination.try_maintenance().unwrap();
            let scan = maintenance.scan().unwrap();
            assert_eq!(scan.live_roots().len(), expected, "{mode:?}");
            assert_eq!(scan.stale_records(), 0, "{mode:?}");
            for root in scan.live_roots() {
                assert_eq!(root.checkpoint(), &checkpoint, "{mode:?}");
            }
        };
        // Consuming admission releases Capture SH, allowing an independent
        // validated scan of the two actual reader records. Generation gates
        // and profile entries are infrastructure, never reader counts.
        let (second, _) = current(&wrapper, &d.0)
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        assert_pins(2);
        assert!(matches!(
            OwnedFileCapture::current(&d.0, wrapper.capture_directory().unwrap()),
            Err(Error::LimitExceeded)
        ));
        assert_pins(2);
        drop(second);
        assert_pins(1);
        let (replacement, _) = current(&wrapper, &d.0)
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        assert_pins(2);
        drop((replacement, read));
        assert_pins(0);
    }
}
