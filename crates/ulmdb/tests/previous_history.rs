#![cfg(target_os = "linux")]
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};
use ulmdb::{
    DatabaseKind, Environment, Error, PreviousError, PublicationMode, ReadTxn, ResidencyPolicy,
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
            "previous-history-{}",
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
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    w.commit().unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"B").unwrap();
    w.commit().unwrap();
    env
}
fn capture(path: &Path) -> ReadTxn {
    ReadTxn::open_previous_coordinated_snapshot(path, ResidencyPolicy::Demand).unwrap()
}
#[test]
fn previous_capture_completes_while_clean_dirty_and_prepared_writer_stays_alive() {
    for mode in MODES {
        for kind in [0, 1, 2] {
            let d = Directory::new();
            let env = seed(&d.0, mode);
            let mut w = env.begin_write().unwrap();
            if kind != 0 {
                w.put(b"kv", b"key", b"C").unwrap();
            }
            if kind == 2 {
                let prepared = w.prepare().unwrap();
                let a = capture(&d.0);
                assert_eq!(a.sequence(), 1);
                assert_eq!(a.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
                assert_eq!(prepared.commit().unwrap(), 3);
                assert_eq!(a.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
            } else {
                let a = capture(&d.0);
                assert_eq!(a.sequence(), 1);
                assert_eq!(a.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
                w.abort();
            }
        }
    }
}
#[test]
fn history_contention_delays_ordinary_publication_without_poisoning() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let gate = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(d.0.join(".ulmdb-coordination/history"))
            .unwrap();
        gate.lock_shared().unwrap();
        let (mut_tx, mut_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let env = &env;
            scope.spawn(move || {
                let mut w = env.begin_write().unwrap();
                w.put(b"kv", b"key", b"C").unwrap();
                mut_tx.send(()).unwrap();
                done_tx.send(w.commit()).unwrap();
            });
            mut_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(done_rx.recv_timeout(Duration::from_millis(40)).is_err());
            gate.unlock().unwrap();
            assert_eq!(
                done_rx
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .unwrap(),
                3
            );
        });
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"C".as_slice())
        );
        env.begin_write().unwrap().abort();
    }
}
#[test]
fn stable_bad_previous_is_corrupt_even_with_an_active_writer_and_never_repaired() {
    let d = Directory::new();
    let env = seed(&d.0, PublicationMode::Legacy);
    let b = fs::read(d.0.join("CURRENT")).unwrap();
    fs::write(d.0.join("PREVIOUS"), &b).unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"C").unwrap();
    assert!(matches!(
        ReadTxn::open_previous_coordinated_snapshot(&d.0, ResidencyPolicy::Demand),
        Err(PreviousError::Engine(Error::Corrupt(_)))
    ));
    assert_eq!(fs::read(d.0.join("PREVIOUS")).unwrap(), b);
    w.abort();
}
#[test]
fn malformed_history_gate_is_rejected_without_selecting_or_leaking_pins() {
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let pins = fs::read_dir(d.0.join(".ulmdb-coordination/pins"))
            .unwrap()
            .count();
        fs::write(d.0.join(".ulmdb-coordination/history"), b"nonempty").unwrap();
        assert!(matches!(
            ReadTxn::open_previous_coordinated_snapshot(&d.0, ResidencyPolicy::Demand),
            Err(PreviousError::Engine(Error::Corrupt(_)))
        ));
        assert_eq!(
            fs::read_dir(d.0.join(".ulmdb-coordination/pins"))
                .unwrap()
                .count(),
            pins
        );
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        drop(env);
    }
}
#[test]
fn readonly_database_permissions_preserve_pure_reads_and_explicit_coordination_errors() {
    use std::os::unix::fs::PermissionsExt;
    for mode in MODES {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let current = fs::read(d.0.join("CURRENT")).unwrap();
        let previous = fs::read(d.0.join("PREVIOUS")).ok();
        fs::set_permissions(&d.0, fs::Permissions::from_mode(0o555)).unwrap();
        let read = capture(&d.0);
        assert_eq!(read.sequence(), 1);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), current);
        assert_eq!(fs::read(d.0.join("PREVIOUS")).ok(), previous);
        drop(read);
        fs::set_permissions(&d.0, fs::Permissions::from_mode(0o700)).unwrap();
        let history = d.0.join(".ulmdb-coordination/history");
        fs::set_permissions(&history, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(
            ReadTxn::open_snapshot(&d.0)
                .unwrap()
                .get(b"kv", b"key")
                .unwrap(),
            Some(b"B".as_slice())
        );
        let status = fs::read_to_string("/proc/self/status").unwrap();
        let effective_uid: u32 = status
            .lines()
            .find(|line| line.starts_with("Uid:"))
            .unwrap()
            .split_whitespace()
            .nth(2)
            .unwrap()
            .parse()
            .unwrap();
        let ordinary_user = effective_uid != 0;
        if ordinary_user {
            assert!(
                matches!(ReadTxn::open_previous_coordinated_snapshot(&d.0,ResidencyPolicy::Demand),Err(PreviousError::Engine(Error::Io(error))) if error.kind()==std::io::ErrorKind::PermissionDenied)
            );
        } else {
            println!("PERMISSION_CONTROL unavailable: effective UID0");
        }
        fs::set_permissions(&history, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(capture(&d.0).sequence(), 1);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), current);
        env.begin_write().unwrap().abort();
    }
}
#[test]
fn foreign_directory_binding_is_rejected_before_creating_history_metadata() {
    for mode in MODES {
        let d = Directory::new();
        ulmdb_storage::create_layout(&d.0).unwrap();
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(d.0.join("LOCK"))
            .unwrap();
        let _fence = ulmdb_storage::WriterFence::try_lock(lock).unwrap();
        let mut a = ulmdb_storage::Manifest::empty().unwrap();
        a.format_minor = match mode {
            PublicationMode::Legacy => 2,
            PublicationMode::Cohort => 3,
            _ => 4,
        };
        if a.format_minor == 4 {
            a.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::prepare_publication(&d.0, &a)
            .unwrap()
            .publish_bootstrap()
            .unwrap();
        let mut b = a.clone();
        b.previous = a.generation;
        b.parent = a.commit;
        b.generation = ulmdb_storage::new_identity().unwrap();
        b.commit = b.generation;
        b.sequence = 1;
        if b.format_minor == 4 {
            b.prepare_indexed(Some(&a)).unwrap();
        }
        let prepared = ulmdb_storage::prepare_publication(&d.0, &b).unwrap();
        let retired = d.0.with_extension("retired");
        fs::rename(&d.0, &retired).unwrap();
        ulmdb_storage::create_layout(&d.0).unwrap();
        assert!(matches!(prepared.publish(), Err(Error::Corrupt(_))));
        assert!(!d.0.join(".ulmdb-coordination").exists());
        assert!(!d.0.join("CURRENT").exists());
        fs::remove_dir_all(retired).unwrap();
    }
}
