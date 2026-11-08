#![cfg(all(target_os = "linux", feature = "fault-injection"))]
//! Deadline/pipe/kernel-lock schedules and injected IO, not storage power loss.
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, ReadTxn};
use ulmdb_storage::coordination::Coordination;
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "file-sync-fault-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(p: &Path, mode: PublicationMode) {
    let e = Environment::create_with_publication(p, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"key", b"B").unwrap();
    w.commit().unwrap();
}
fn checkpoint(p: &Path) -> [u8; 64] {
    ulmdb_storage::checkpoint_bytes(&ulmdb_storage::read_current(p).unwrap()).unwrap()
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
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn marker(s: &str) {
    println!("{s}");
    std::io::stdout().flush().unwrap();
}
fn byte() {
    let mut b = [0];
    std::io::stdin().read_exact(&mut b).unwrap();
}
struct Worker {
    child: Child,
    rx: mpsc::Receiver<String>,
    pump: Option<std::thread::JoinHandle<Vec<String>>>,
    stderr: PathBuf,
    role: String,
}
impl Worker {
    fn spawn(p: &Path, role: &str, pause: Option<&str>, fail: Option<&str>) -> Self {
        let stderr = p.with_extension(format!(
            "{role}-{}.stderr",
            ulmdb_storage::new_identity().unwrap()
        ));
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args(["--exact", "file_sync_fault_worker", "--nocapture"])
            .env("ULMDB_SYNC_PATH", p)
            .env("ULMDB_SYNC_ROLE", role)
            .env_remove("ULMDB_FAIL_AT")
            .env_remove("ULMDB_PAUSE_AT")
            .env_remove("ULMDB_CRASH_AT")
            .env_remove("ULMDB_TRACE_FILE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(&stderr).unwrap());
        if let Some(point) = pause {
            c.env("ULMDB_PAUSE_AT", point);
        }
        if let Some(point) = fail {
            c.env("ULMDB_FAIL_AT", point);
        }
        let mut child = c.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let pump = std::thread::spawn(move || {
            let mut all = vec![];
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                let _ = tx.send(line.clone());
                all.push(line);
            }
            all
        });
        Self {
            child,
            rx,
            pump: Some(pump),
            stderr,
            role: role.into(),
        }
    }
    fn expect(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let line = self
                .rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|e| {
                    panic!(
                        "{} waiting {needle}: {e:?}; {}",
                        self.role,
                        fs::read_to_string(&self.stderr).unwrap()
                    )
                });
            if line.contains(needle) {
                return;
            }
        }
    }
    fn release(&mut self) {
        self.child.stdin.as_mut().unwrap().write_all(b"x").unwrap();
        self.child.stdin.as_mut().unwrap().flush().unwrap();
    }
    fn waiter(&mut self, file: &Path) {
        use std::os::unix::fs::MetadataExt;
        let inode = fs::metadata(file).unwrap().ino().to_string();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "{} exited before expected kernel waiter",
                self.role
            );
            let locks = fs::read_to_string("/proc/locks").unwrap();
            if let Some(line) = locks.lines().find(|line| {
                line.contains("->")
                    && line
                        .split_whitespace()
                        .any(|x| x == self.child.id().to_string())
                    && line
                        .split_whitespace()
                        .any(|x| x.rsplit(':').next() == Some(inode.as_str()))
            }) {
                println!("KERNEL_WAITER role={} {line}", self.role);
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{} did not block on actual inode {inode}",
                self.role
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn done(mut self, success: bool) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                break s;
            }
            assert!(Instant::now() < deadline, "{} did not finish", self.role);
            std::thread::sleep(Duration::from_millis(2));
        };
        let all = self.pump.take().unwrap().join().unwrap();
        let stderr = fs::read_to_string(&self.stderr).unwrap();
        assert_eq!(
            status.success(),
            success,
            "{} status {status}\n{}\n{stderr}",
            self.role,
            all.join("\n")
        );
        for line in &all {
            println!("WORKER {} {line}", self.role);
        }
        let _ = fs::remove_file(&self.stderr);
        all
    }
    fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        if let Some(p) = self.pump.take() {
            p.join().unwrap();
        }
        let _ = fs::remove_file(&self.stderr);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(p) = self.pump.take() {
            let _ = p.join();
        }
        let _ = fs::remove_file(&self.stderr);
    }
}
#[test]
fn file_sync_fault_worker() {
    let Some(p) = std::env::var_os("ULMDB_SYNC_PATH") else {
        return;
    };
    let p = PathBuf::from(p);
    let role = std::env::var("ULMDB_SYNC_ROLE").unwrap();
    if role == "seed" {
        let mode = match std::env::var("ULMDB_SYNC_FORMAT").unwrap().as_str() {
            "2" => PublicationMode::Legacy,
            "3" => PublicationMode::Cohort,
            "4" => PublicationMode::Indexed,
            _ => panic!("bad seed format"),
        };
        seed(&p, mode);
        let manifest = ulmdb_storage::read_current(&p).unwrap();
        marker(&format!(
            "DESCRIPTOR={}",
            ulmdb_storage::descriptor_path(&p, &manifest).display()
        ));
        for origin in manifest.metadata_origins_ref() {
            if *origin != manifest.generation {
                marker(&format!(
                    "ORIGIN={}",
                    p.join("cohorts")
                        .join(format!("{origin:032x}.cohort"))
                        .display()
                ));
            }
        }
        for entry in &manifest.segments {
            marker(&format!(
                "SEGMENT={}",
                ulmdb_storage::segment_path(&p, entry).display()
            ));
        }
        marker("SEEDED");
        return;
    }
    if role == "reject-pointer" {
        marker("BEFORE_SYNC");
        assert!(ulmdb::sync_file_environment(&p).is_err());
        marker("SYNC_REJECTED");
        return;
    }
    if role == "sync" {
        let before = pins(&p);
        let fds = fs::read_dir("/proc/self/fd").unwrap().count();
        marker(&format!("SYNC_START fds={fds} pins={before}"));
        let report = ulmdb::sync_file_environment(&p).unwrap();
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
        marker(&format!(
            "SYNC_RESOURCE pins_before={before} pins_after={}",
            pins(&p)
        ));
        marker(&format!(
            "SYNCED checkpoint={}",
            hex(&report.selected_checkpoint)
        ));
        return;
    }
    if role == "publisher" {
        let e = Environment::open(&p).unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        let prepared = w.prepare().unwrap();
        marker("PREPARED");
        byte();
        prepared.commit().unwrap();
        marker("PUBLISHED");
        return;
    }
    if role == "owned-namespace" {
        let e = Environment::open(&p).unwrap();
        marker("OWNER_READY");
        assert!(e.sync().is_err());
        marker("OWNER_NAMESPACE_ERROR");
        byte();
        e.sync().unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"usable").unwrap();
        w.commit().unwrap();
        marker("OWNER_USABLE");
        return;
    }
    let e = Environment::open(&p).unwrap();
    let head = selectors(&p);
    let old = e.file_checkpoint().unwrap();
    if role == "failure-clean" {
        let w = e.begin_write().unwrap();
        let before = pins(&p);
        let result = e.sync();
        if result.is_ok() {
            marker("UNEXPECTED_SYNC_SUCCESS");
            panic!("fault was not exercised");
        }
        assert!(matches!(result, Err(Error::Io(_))));
        assert_eq!(selectors(&p), head);
        assert_eq!(pins(&p), before);
        assert_eq!(e.file_checkpoint().unwrap(), old);
        w.abort();
        e.begin_write().unwrap().abort();
    } else if role == "failure-dirty" {
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        let before = pins(&p);
        let result = e.sync();
        if result.is_ok() {
            marker("UNEXPECTED_SYNC_SUCCESS");
            panic!("fault was not exercised");
        }
        assert!(matches!(result, Err(Error::Io(_))));
        assert_eq!(selectors(&p), head);
        assert_eq!(pins(&p), before);
        assert_eq!(e.file_checkpoint().unwrap(), old);
        assert_eq!(w.get(b"kv", b"key").unwrap(), Some(b"C".as_slice()));
        w.commit().unwrap();
        assert_eq!(
            e.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"C".as_slice())
        );
    } else if role == "failure-prepared" {
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"key", b"C").unwrap();
        let prepared = w.prepare().unwrap();
        let before = pins(&p);
        let result = e.sync();
        if result.is_ok() {
            marker("UNEXPECTED_SYNC_SUCCESS");
            panic!("fault was not exercised");
        }
        assert!(matches!(result, Err(Error::Io(_))));
        assert_eq!(selectors(&p), head);
        assert_eq!(pins(&p), before);
        assert_eq!(e.file_checkpoint().unwrap(), old);
        prepared.commit().unwrap();
        assert_eq!(
            e.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"C".as_slice())
        );
    } else {
        panic!("bad worker role {role}");
    }
    marker("SYNC_FAULT_PASS");
}
#[test]
fn injected_fence_failures_never_select_or_poison_clean_dirty_or_prepared_owners() {
    let points = [
        "sync.before_file_sync",
        "sync.after_file_sync",
        "sync.before_assets_directory_sync",
        "sync.after_assets_directory_sync",
        "sync.before_current_file_sync",
        "sync.after_current_file_sync",
        "sync.before_selection_directory_sync",
        "sync.after_selection_directory_sync",
    ];
    for mode in MODES {
        let mut selected_points = points.to_vec();
        if mode == PublicationMode::Legacy {
            selected_points.extend([
                "sync.before_previous_file_sync",
                "sync.after_previous_file_sync",
            ]);
        }
        for point in selected_points {
            for role in ["failure-clean", "failure-dirty", "failure-prepared"] {
                let d = Directory::new();
                seed(&d.0, mode);
                let child = Worker::spawn(&d.0, role, None, Some(point));
                let lines = child.done(true);
                assert!(lines.iter().any(|x| x.contains("SYNC_FAULT_PASS")));
            }
        }
    }
}
#[test]
fn nonexistent_injector_is_a_real_rejected_success_control() {
    let d = Directory::new();
    seed(&d.0, PublicationMode::Legacy);
    let child = Worker::spawn(
        &d.0,
        "failure-dirty",
        None,
        Some("sync.nonexistent-control"),
    );
    let lines = child.done(false);
    assert!(lines.iter().any(|x| x.contains("UNEXPECTED_SYNC_SUCCESS")));
}
#[test]
fn sync_waits_for_selected_publication_while_capture_excludes_maintenance() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let mut publisher = Worker::spawn(
            &d.0,
            "publisher",
            Some("pointer.CURRENT.before_write"),
            None,
        );
        publisher.expect("PREPARED");
        publisher.release();
        publisher.expect("PAUSED point=pointer.CURRENT.before_write");
        let before = pins(&d.0);
        let mut sync = Worker::spawn(&d.0, "sync", None, None);
        sync.expect("SYNC_START");
        sync.waiter(&d.0.join(".ulmdb-coordination/history"));
        assert_eq!(pins(&d.0), before);
        let c = Coordination::open(
            &d.0,
            ulmdb_storage::read_current(&d.0).unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        assert!(matches!(c.try_maintenance(), Err(Error::Busy)));
        publisher.release();
        publisher.done(true);
        let selected = checkpoint(&d.0);
        let lines = sync.done(true);
        assert!(
            lines
                .iter()
                .any(|x| x.contains(&format!("SYNCED checkpoint={}", hex(&selected))))
        );
        assert_eq!(
            ReadTxn::open_snapshot(&d.0)
                .unwrap()
                .get(b"kv", b"key")
                .unwrap(),
            Some(b"C".as_slice())
        );
        drop(c.try_maintenance().unwrap());
    }
}
#[test]
fn paused_sync_has_no_pin_and_blocks_only_final_selection_not_prepare_or_current_read() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let selected = checkpoint(&d.0);
        let before = pins(&d.0);
        let mut sync = Worker::spawn(&d.0, "sync", Some("sync.before_current_file_sync"), None);
        sync.expect("PAUSED point=sync.before_current_file_sync");
        assert_eq!(pins(&d.0), before);
        let current = ReadTxn::open_coordinated_snapshot(&d.0).unwrap();
        assert_eq!(current.get(b"kv", b"key").unwrap(), Some(b"B".as_slice()));
        drop(current);
        let c = Coordination::open(
            &d.0,
            ulmdb_storage::read_current(&d.0).unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        assert!(matches!(c.try_maintenance(), Err(Error::Busy)));
        let mut publisher = Worker::spawn(&d.0, "publisher", None, None);
        publisher.expect("PREPARED");
        publisher.release();
        publisher.waiter(&d.0.join(".ulmdb-coordination/history"));
        sync.release();
        let lines = sync.done(true);
        assert!(
            lines
                .iter()
                .any(|x| x.contains(&format!("SYNCED checkpoint={}", hex(&selected))))
        );
        publisher.done(true);
        assert_ne!(checkpoint(&d.0), selected);
        drop(c.try_maintenance().unwrap());
    }
}
#[test]
fn maintenance_excludes_sync_without_poison_and_process_death_releases_both_gates() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let e = Environment::open(&d.0).unwrap();
        let c = Coordination::open(
            &d.0,
            ulmdb_storage::read_current(&d.0).unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        let maintenance = c.try_maintenance().unwrap();
        assert!(matches!(e.sync(), Err(Error::Busy)));
        drop(maintenance);
        e.sync().unwrap();
        e.begin_write().unwrap().abort();
        drop(e);
        let sync = Worker::spawn(&d.0, "sync", Some("sync.before_current_file_sync"), None);
        sync.expect("PAUSED point=sync.before_current_file_sync");
        assert!(matches!(c.try_maintenance(), Err(Error::Busy)));
        sync.kill();
        drop(c.try_maintenance().unwrap());
        let mut publisher = Worker::spawn(&d.0, "publisher", None, None);
        publisher.expect("PREPARED");
        publisher.release();
        publisher.done(true);
        assert_eq!(
            ReadTxn::open_snapshot(&d.0)
                .unwrap()
                .get(b"kv", b"key")
                .unwrap(),
            Some(b"C".as_slice())
        );
    }
}
#[test]
fn owned_namespace_is_rechecked_after_actual_history_wait_and_error_keeps_writer_usable() {
    for mode in MODES {
        let d = Directory::new();
        let displaced = Directory::new();
        seed(&d.0, mode);
        let gate = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(d.0.join(".ulmdb-coordination/history"))
            .unwrap();
        gate.lock().unwrap();
        let mut owner = Worker::spawn(&d.0, "owned-namespace", None, None);
        owner.expect("OWNER_READY");
        owner.waiter(&d.0.join(".ulmdb-coordination/history"));
        fs::rename(&d.0, &displaced.0).unwrap();
        let replacement = Environment::create_with_publication(&d.0, mode).unwrap();
        let head = selectors(&d.0);
        drop(replacement);
        gate.unlock().unwrap();
        owner.expect("OWNER_NAMESPACE_ERROR");
        assert_eq!(selectors(&d.0), head);
        fs::remove_dir_all(&d.0).unwrap();
        fs::rename(&displaced.0, &d.0).unwrap();
        owner.release();
        owner.done(true);
        assert_eq!(
            ReadTxn::open_snapshot(&d.0)
                .unwrap()
                .get(b"kv", b"key")
                .unwrap(),
            Some(b"usable".as_slice())
        );
    }
}

#[test]
fn fifo_selection_metadata_is_rejected_promptly_without_repair() {
    for mode in MODES {
        for name in if mode == PublicationMode::Legacy {
            vec!["CURRENT", "PREVIOUS"]
        } else {
            vec!["CURRENT"]
        } {
            let d = Directory::new();
            seed(&d.0, mode);
            let named = d.0.join(name);
            let saved = d.0.join(format!("saved-{name}"));
            fs::rename(&named, &saved).unwrap();
            assert!(
                Command::new("mkfifo")
                    .arg(&named)
                    .status()
                    .unwrap()
                    .success()
            );
            let child = Worker::spawn(&d.0, "reject-pointer", None, None);
            child.expect("BEFORE_SYNC");
            let lines = child.done(true);
            assert!(lines.iter().any(|line| line.contains("SYNC_REJECTED")));
            use std::os::unix::fs::FileTypeExt;
            assert!(fs::symlink_metadata(&named).unwrap().file_type().is_fifo());
            fs::remove_file(&named).unwrap();
            fs::rename(saved, named).unwrap();
            let e = Environment::open(&d.0).unwrap();
            e.sync().unwrap();
            e.begin_write().unwrap().abort();
        }
    }
}

#[test]
fn fifo_selected_descriptors_origins_and_assets_are_rejected_promptly() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let manifest = ulmdb_storage::read_current(&d.0).unwrap();
        let mut paths =
            std::collections::BTreeSet::from([ulmdb_storage::descriptor_path(&d.0, &manifest)]);
        paths.extend(
            manifest
                .metadata_origins_ref()
                .iter()
                .map(|origin| d.0.join("cohorts").join(format!("{origin:032x}.cohort"))),
        );
        paths.extend(
            manifest
                .segments
                .iter()
                .map(|entry| ulmdb_storage::segment_path(&d.0, entry)),
        );
        for named in paths {
            let saved = named.with_extension("saved");
            fs::rename(&named, &saved).unwrap();
            assert!(
                Command::new("mkfifo")
                    .arg(&named)
                    .status()
                    .unwrap()
                    .success()
            );
            let child = Worker::spawn(&d.0, "reject-pointer", None, None);
            child.expect("BEFORE_SYNC");
            let lines = child.done(true);
            assert!(lines.iter().any(|line| line.contains("SYNC_REJECTED")));
            use std::os::unix::fs::FileTypeExt;
            assert!(fs::symlink_metadata(&named).unwrap().file_type().is_fifo());
            fs::remove_file(&named).unwrap();
            fs::rename(saved, named).unwrap();
        }
        let e = Environment::open(&d.0).unwrap();
        e.sync().unwrap();
        e.begin_write().unwrap().abort();
    }
}

#[test]
fn regular_to_fifo_replacement_between_check_and_open_never_blocks() {
    for mode in MODES {
        for descriptor in [false, true] {
            let d = Directory::new();
            seed(&d.0, mode);
            let manifest = ulmdb_storage::read_current(&d.0).unwrap();
            let named = if descriptor {
                ulmdb_storage::descriptor_path(&d.0, &manifest)
            } else {
                d.0.join("CURRENT")
            };
            let saved = named.with_extension("saved");
            let mut child = Worker::spawn(
                &d.0,
                "reject-pointer",
                Some("sync.metadata.before_open"),
                None,
            );
            child.expect("PAUSED point=sync.metadata.before_open");
            if descriptor {
                child.release();
                child.expect("PAUSED point=sync.metadata.before_open");
            }
            fs::rename(&named, &saved).unwrap();
            assert!(
                Command::new("mkfifo")
                    .arg(&named)
                    .status()
                    .unwrap()
                    .success()
            );
            child.release();
            let lines = child.done(true);
            assert!(lines.iter().any(|line| line.contains("SYNC_REJECTED")));
            use std::os::unix::fs::FileTypeExt;
            assert!(fs::symlink_metadata(&named).unwrap().file_type().is_fifo());
            fs::remove_file(&named).unwrap();
            fs::rename(saved, named).unwrap();
            let e = Environment::open(&d.0).unwrap();
            e.sync().unwrap();
            e.begin_write().unwrap().abort();
        }
    }
}
