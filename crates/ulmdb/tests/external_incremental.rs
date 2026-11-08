#![cfg(target_os = "linux")]
//! Caller-managed exclusion is explicit. These tests serialize every operation
//! and retain all old immutable assets; external handles are not physical pins.
use std::{fs, io::Cursor, path::PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Error, PageSize, Prefix, PublicationMode, backup};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "external-incremental-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn modes() -> [PublicationMode; 3] {
    [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ]
}
fn seed(source: &Dir, destination: &Dir, mode: PublicationMode) {
    let env =
        Environment::create_with_page_size(&source.0, mode, PageSize::new(512).unwrap()).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(4) })
        .unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    for n in 0..64u32 {
        w.put(b"kv", &n.to_be_bytes(), &[n as u8; 64]).unwrap();
        w.insert_duplicate(b"dups", b"k", &n.to_be_bytes()).unwrap();
    }
    w.put(b"kv", b"key", b"A").unwrap();
    w.insert_prefix(b"ip", Prefix::new(Address::V4(0), 0).unwrap(), b"A")
        .unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    backup::export(&env, &destination.0).unwrap();
}
fn delta(source: &Environment) -> Vec<u8> {
    let mut w = source.begin_write().unwrap();
    w.put(b"kv", b"key", b"B").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
    let mut w = source.begin_write().unwrap();
    w.put(b"kv", b"key", b"C").unwrap();
    w.delete(b"kv", &0u32.to_be_bytes()).unwrap();
    w.insert_duplicate(b"dups", b"k", &64u32.to_be_bytes())
        .unwrap();
    w.insert_prefix(b"ip", Prefix::new(Address::V4(0), 0).unwrap(), b"C")
        .unwrap();
    assert_eq!(w.commit().unwrap(), 3);
    let mut bytes = Vec::new();
    let report = source.dump_incremental(1, &mut bytes).unwrap();
    assert_eq!((report.base_sequence, report.sequence), (1, 3));
    assert!(report.files > 0);
    bytes
}
fn namespace_metadata(dir: &Dir) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &std::path::Path, here: &std::path::Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in fs::read_dir(here).unwrap() {
            let e = e.unwrap();
            let path = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(root, &path, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let root = dir.0.join(".ulmdb-coordination");
    let mut result = Vec::new();
    walk(&root, &root, &mut result);
    result.sort();
    assert!(
        result
            .iter()
            .any(|(name, _)| name == std::path::Path::new("capture"))
    );
    result
}

#[test]
fn external_roundtrip_under_capture_exclusion_keeps_old_borrow_actual_predecessor_and_external_owner()
 {
    for mode in modes() {
        let source = Dir::new();
        let destination = Dir::new();
        seed(&source, &destination, mode);
        let uuid = ulmdb_storage::read_current(&source.0).unwrap().uuid;
        let source_coord =
            ulmdb_storage::coordination::Coordination::open(&source.0, uuid, Default::default())
                .unwrap();
        let dest_coord = ulmdb_storage::coordination::Coordination::open(
            &destination.0,
            uuid,
            Default::default(),
        )
        .unwrap();
        // These genuine Capture EX leases would reject ordinary coordinated
        // pin/capture. Explicit external paths must not touch that protocol.
        let _source_maintenance = source_coord.try_maintenance().unwrap();
        let _dest_maintenance = dest_coord.try_maintenance().unwrap();
        let source_meta = namespace_metadata(&source);
        let dest_meta = namespace_metadata(&destination);
        let source_env = Environment::open_externally_synchronized(&source.0).unwrap();
        let target = Environment::open_externally_synchronized(&destination.0).unwrap();
        let old = target.begin_read();
        let borrow = old.get(b"kv", b"key").unwrap().unwrap();
        let source_old = source_env.begin_read();
        let bytes = delta(&source_env);
        let mut captured = Vec::new();
        source_env
            .begin_read()
            .dump_incremental_snapshot(
                &source.0,
                1,
                &mut captured,
                backup::IncrementalLimits::default(),
            )
            .unwrap();
        assert_eq!(bytes, captured);
        let mut noop = Vec::new();
        assert_eq!(
            source_old
                .dump_incremental_snapshot(
                    &source.0,
                    1,
                    &mut noop,
                    backup::IncrementalLimits::default()
                )
                .unwrap()
                .files,
            0
        );
        assert!(noop.is_empty());
        let report = target.load_incremental(&mut Cursor::new(&bytes)).unwrap();
        assert_eq!((report.base_sequence, report.sequence), (1, 3));
        assert!(target.is_externally_synchronized());
        assert!(target.begin_read().external_synchronization().is_some());
        assert_eq!(borrow, b"A");
        assert_eq!(old.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
        let read = target.begin_read();
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"C".as_slice()));
        assert_eq!(read.get(b"kv", &0u32.to_be_bytes()).unwrap(), None);
        assert_eq!(read.duplicates(b"dups").unwrap().count(b"k").unwrap(), 65);
        assert_eq!(
            read.longest_match(b"ip", Address::V4(42))
                .unwrap()
                .unwrap()
                .1,
            b"C"
        );
        assert_eq!(read.stats().page_size, 512);
        assert_eq!(
            read.file_checkpoint().unwrap(),
            source_env.begin_read().file_checkpoint().unwrap()
        );
        let (head, previous) = ulmdb_storage::read_roots(&destination.0).unwrap();
        assert_eq!(head.sequence, 3);
        assert_eq!(previous.unwrap().sequence, 2);
        assert_eq!(source_meta, namespace_metadata(&source));
        assert_eq!(dest_meta, namespace_metadata(&destination));
        let mut next = target.begin_write().unwrap();
        next.put(b"kv", b"key", b"next").unwrap();
        assert_eq!(next.commit().unwrap(), 4);
        drop(read);
        drop(old);
        drop(source_old);
        drop(target);
        drop(source_env);
        let reopened = Environment::open_externally_synchronized(&destination.0).unwrap();
        assert_eq!(
            reopened.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"next".as_slice())
        );
    }
}
#[test]
fn external_malformed_admission_replay_and_limits_never_select() {
    for mode in modes() {
        let source = Dir::new();
        let destination = Dir::new();
        seed(&source, &destination, mode);
        let s = Environment::open_externally_synchronized(&source.0).unwrap();
        let t = Environment::open_externally_synchronized(&destination.0).unwrap();
        let bytes = delta(&s);
        let before = fs::read(destination.0.join("CURRENT")).unwrap();
        let mut damaged = bytes.clone();
        *damaged.last_mut().unwrap() ^= 1;
        assert!(t.load_incremental(&mut Cursor::new(damaged)).is_err());
        assert_eq!(before, fs::read(destination.0.join("CURRENT")).unwrap());
        assert!(matches!(
            t.load_incremental_checked(
                &mut Cursor::new(&bytes),
                backup::IncrementalLimits {
                    max_files: 0,
                    ..Default::default()
                },
                |_| Ok(())
            ),
            Err(Error::LimitExceeded)
        ));
        let denied =
            t.load_incremental_checked(&mut Cursor::new(&bytes), Default::default(), |r| {
                assert!(r.external_synchronization().is_some());
                assert_eq!(r.get(b"kv", b"key")?, Some(b"C".as_slice()));
                Err(Error::InvalidInput("application rejected candidate"))
            });
        assert!(matches!(
            denied,
            Err(Error::InvalidInput("application rejected candidate"))
        ));
        assert_eq!(before, fs::read(destination.0.join("CURRENT")).unwrap());
        t.load_incremental(&mut Cursor::new(&bytes)).unwrap();
        let current = fs::read(destination.0.join("CURRENT")).unwrap();
        assert!(t.load_incremental(&mut Cursor::new(&bytes)).is_err());
        assert_eq!(current, fs::read(destination.0.join("CURRENT")).unwrap());
        assert!(
            fs::read_dir(destination.0.join("transactions"))
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(t.load_incremental(&mut Cursor::new([])).unwrap().files, 0);
    }
}
#[test]
fn external_drift_and_cross_namespace_snapshot_dump_reject_before_output() {
    let source = Dir::new();
    let destination = Dir::new();
    seed(&source, &destination, PublicationMode::Indexed);
    let stale = Environment::open_externally_synchronized(&source.0).unwrap();
    let other = Environment::open_externally_synchronized(&source.0).unwrap();
    let _ = delta(&other);
    assert!(matches!(
        stale.load_incremental(&mut Cursor::new([])),
        Err(Error::Busy)
    ));
    let read = other.begin_read();
    let mut out = Vec::new();
    assert!(
        read.dump_incremental_snapshot(&destination.0, 1, &mut out, Default::default())
            .is_err()
    );
    assert!(out.is_empty());
}
