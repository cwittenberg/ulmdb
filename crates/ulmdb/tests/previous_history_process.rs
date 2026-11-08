#![cfg(all(target_os = "linux", feature = "fault-injection"))]
//! Pipe-ordered real processes and kernel waiters, not power-loss simulation.
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
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
            "previous-history-process-{}",
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
fn byte() {
    let mut b = [0];
    std::io::stdin().read_exact(&mut b).unwrap();
}
fn marker(s: &str) {
    println!("{s}");
    std::io::stdout().flush().unwrap();
}
struct Worker {
    child: Child,
    rx: mpsc::Receiver<String>,
    pump: Option<std::thread::JoinHandle<Vec<String>>>,
    stderr: PathBuf,
    role: String,
}
impl Worker {
    fn spawn(p: &Path, role: &str, pause: Option<&str>) -> Self {
        let stderr = p.with_extension(format!(
            "{role}-{}.stderr",
            ulmdb_storage::new_identity().unwrap()
        ));
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "history_process_worker", "--nocapture"])
            .env("ULMDB_HISTORY_PATH", p)
            .env("ULMDB_HISTORY_ROLE", role)
            .env(
                "ULMDB_HISTORY_FORMAT",
                if p.join("format-test-control").exists() {
                    fs::read_to_string(p.join("format-test-control")).unwrap()
                } else {
                    "2".into()
                },
            )
            .env_remove("ULMDB_FAIL_AT")
            .env_remove("ULMDB_CRASH_AT")
            .env_remove("ULMDB_TRACE_FILE")
            .env_remove("ULMDB_PAUSE_AT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(&stderr).unwrap());
        if let Some(pause) = pause {
            cmd.env("ULMDB_PAUSE_AT", pause);
        }
        let mut child = cmd.spawn().unwrap();
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
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let line = self
                .rx
                .recv_timeout(end.saturating_duration_since(Instant::now()))
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
    fn waiter(&mut self, p: &Path) {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::metadata(p.join(".ulmdb-coordination/history")).unwrap();
        let inode = format!(
            "{:02x}:{:02x}:{}",
            major(meta.dev()),
            minor(meta.dev()),
            meta.ino()
        );
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "{} exited before waiting",
                self.role
            );
            let locks = fs::read_to_string("/proc/locks").unwrap();
            if let Some(line) = locks.lines().find(|l| {
                l.contains("->")
                    && l.split_whitespace()
                        .any(|x| x == self.child.id().to_string())
                    && l.contains(&inode)
            }) {
                println!("KERNEL_WAITER role={} {line}", self.role);
                return;
            }
            assert!(
                Instant::now() < end,
                "{} never blocked on history inode {inode}",
                self.role
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn done(mut self) {
        self.child
            .wait()
            .map(|s| {
                assert!(
                    s.success(),
                    "{} exit {s}; {}",
                    self.role,
                    fs::read_to_string(&self.stderr).unwrap()
                )
            })
            .unwrap();
        self.report();
    }
    fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        self.report();
    }
    fn report(&mut self) {
        if let Some(pump) = self.pump.take() {
            for l in pump.join().unwrap() {
                println!("WORKER {} {l}", self.role);
            }
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
        self.report();
    }
}
fn major(device: u64) -> u64 {
    ((device >> 8) & 0xfff) | ((device >> 32) & 0xfffff000)
}
fn minor(device: u64) -> u64 {
    (device & 0xff) | ((device >> 12) & 0xffffff00)
}
#[test]
fn history_process_worker() {
    let Some(p) = std::env::var_os("ULMDB_HISTORY_PATH") else {
        return;
    };
    let p = PathBuf::from(p);
    match std::env::var("ULMDB_HISTORY_ROLE").unwrap().as_str() {
        role @ ("held-clean" | "held-dirty" | "held-prepared") => {
            let env = Environment::open(&p).unwrap();
            let mut w = env.begin_write().unwrap();
            if role != "held-clean" {
                w.put(b"kv", b"key", b"C").unwrap();
            }
            let (write, prepared) = if role == "held-prepared" {
                (None, Some(w.prepare().unwrap()))
            } else {
                (Some(w), None)
            };
            marker("HELD");
            byte();
            let sequence = if let Some(prepared) = prepared {
                prepared.commit().unwrap()
            } else {
                write.unwrap().commit().unwrap()
            };
            assert_eq!(sequence, if role == "held-clean" { 2 } else { 3 });
            marker("COMMITTED");
            byte();
            env.reclaim().unwrap();
            marker("RECLAIMED");
            byte();
        }
        role @ ("previous-writer" | "rollback" | "compact") => {
            if role == "previous-writer" {
                let session = ulmdb::PreviousSession::open(&p).unwrap();
                let mut w = session.begin_write().unwrap();
                w.put(b"kv", b"key", b"C").unwrap();
                let prepared = w.prepare().unwrap();
                marker("PREPARED");
                byte();
                assert_eq!(prepared.commit().unwrap(), 2);
                marker("COMMITTED");
                byte();
            } else {
                let env = Environment::open(&p).unwrap();
                marker("PREPARED");
                byte();
                if role == "rollback" {
                    env.rollback_last_commit(2).unwrap();
                    assert_eq!(env.begin_read().sequence(), 1);
                } else {
                    env.compact().unwrap();
                    assert_eq!(env.begin_read().sequence(), 2);
                }
                marker("COMMITTED");
                byte();
            }
        }
        "bootstrap" => {
            let file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(p.join("LOCK"))
                .unwrap();
            let _fence = ulmdb_storage::WriterFence::try_lock(file).unwrap();
            let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
            manifest.format_minor = std::env::var("ULMDB_HISTORY_FORMAT")
                .unwrap()
                .parse()
                .unwrap();
            if manifest.format_minor == 4 {
                manifest.prepare_indexed(None).unwrap();
            }
            let token = ulmdb_storage::prepare_publication(&p, &manifest).unwrap();
            marker("PREPARED");
            byte();
            token.publish_bootstrap().unwrap();
            marker("COMMITTED");
            byte();
        }
        "writer-error" => {
            let env = Environment::open(&p).unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", b"C").unwrap();
            let prepared = w.prepare().unwrap();
            marker("PREPARED");
            byte();
            assert!(matches!(prepared.commit(), Err(Error::OutcomeUnknown)));
            assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
            marker("REJECTED");
            byte();
        }
        "reader-error" => {
            assert!(matches!(
                ReadTxn::open_previous_coordinated_snapshot(&p, ResidencyPolicy::Demand),
                Err(PreviousError::Engine(Error::Corrupt(_)))
            ));
            marker("REJECTED");
            byte();
        }
        "inventory" => {
            let mode = match std::env::var("ULMDB_HISTORY_FORMAT").unwrap().as_str() {
                "2" => PublicationMode::Legacy,
                "3" => PublicationMode::Cohort,
                _ => PublicationMode::Indexed,
            };
            seed(&p, mode);
            marker("INVENTORY_CREATED");
        }
        "writer" => {
            let env = Environment::open(&p).unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", b"C").unwrap();
            let prepared = w.prepare().unwrap();
            marker("PREPARED");
            byte();
            assert_eq!(prepared.commit().unwrap(), 3);
            marker("COMMITTED");
            byte();
            env.reclaim().unwrap();
            marker("RECLAIMED");
            byte();
        }
        "reader" => {
            let read =
                ReadTxn::open_previous_coordinated_snapshot(&p, ResidencyPolicy::Demand).unwrap();
            let borrowed = read.get(b"kv", b"key").unwrap().unwrap();
            marker(&format!(
                "CAPTURED sequence={} value={}",
                read.sequence(),
                String::from_utf8_lossy(borrowed)
            ));
            byte();
            assert_eq!(read.get(b"kv", b"key").unwrap().unwrap(), borrowed);
            marker("STABLE");
            byte();
        }
        "previous-error" => {
            assert!(matches!(
                ReadTxn::open_previous_coordinated_snapshot(&p, ResidencyPolicy::Demand),
                Err(PreviousError::Engine(Error::Corrupt(_)))
            ));
        }
        _ => panic!("bad worker role"),
    }
}
#[test]
fn publication_excludes_capture_even_in_legacy_previous_rotated_current_old_window() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        let pause = if mode == PublicationMode::Legacy {
            "pointer.PREVIOUS.after_rename"
        } else {
            "history.publish.acquired"
        };
        let mut writer = Worker::spawn(&d.0, "writer", Some(pause));
        writer.expect("PREPARED");
        writer.release();
        writer.expect("PAUSED point=");
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        if mode == PublicationMode::Legacy {
            assert_eq!(fs::read(d.0.join("PREVIOUS")).unwrap(), head);
        }
        let mut reader = Worker::spawn(&d.0, "reader", None);
        reader.waiter(&d.0);
        writer.release();
        writer.expect("COMMITTED");
        reader.expect("CAPTURED sequence=2 value=B");
        writer.release();
        writer.expect("RECLAIMED");
        reader.release();
        reader.expect("STABLE");
        reader.release();
        reader.done();
        writer.release();
        writer.done();
    }
}
#[test]
fn captured_previous_pin_outlives_publication_and_reclaim_after_history_shared_releases() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let mut reader = Worker::spawn(&d.0, "reader", Some("history.capture.acquired"));
        reader.expect("PAUSED point=");
        let mut writer = Worker::spawn(&d.0, "writer", None);
        writer.expect("PREPARED");
        writer.release();
        writer.waiter(&d.0);
        reader.release();
        reader.expect("CAPTURED sequence=1 value=A");
        writer.expect("COMMITTED");
        writer.release();
        writer.expect("RECLAIMED");
        reader.release();
        reader.expect("STABLE");
        reader.release();
        reader.done();
        writer.release();
        writer.done();
    }
}
#[test]
fn process_death_releases_history_shared_and_exclusive_without_clearing_stable_corruption() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        let reader = Worker::spawn(&d.0, "reader", Some("history.capture.acquired"));
        reader.expect("PAUSED point=");
        let mut writer = Worker::spawn(&d.0, "writer", None);
        writer.expect("PREPARED");
        writer.release();
        writer.waiter(&d.0);
        reader.kill();
        writer.expect("COMMITTED");
        writer.release();
        writer.expect("RECLAIMED");
        writer.release();
        writer.done();
        let d = Directory::new();
        seed(&d.0, mode);
        let pause = if mode == PublicationMode::Legacy {
            "pointer.PREVIOUS.after_rename"
        } else {
            "history.publish.acquired"
        };
        let mut writer = Worker::spawn(&d.0, "writer", Some(pause));
        writer.expect("PREPARED");
        writer.release();
        writer.expect("PAUSED point=");
        writer.kill();
        if mode == PublicationMode::Legacy {
            Worker::spawn(&d.0, "previous-error", None).done();
        } else {
            let r =
                ReadTxn::open_previous_coordinated_snapshot(&d.0, ResidencyPolicy::Demand).unwrap();
            assert_eq!(r.sequence(), 1);
        }
        let env = Environment::open(&d.0).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"B".as_slice())
        );
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"recovered").unwrap();
        w.commit().unwrap();
        assert_eq!(
            ReadTxn::open_previous_coordinated_snapshot(&d.0, ResidencyPolicy::Demand)
                .unwrap()
                .get(b"kv", b"key")
                .unwrap(),
            Some(b"B".as_slice())
        );
    }
}

#[test]
fn readonly_previous_capture_in_a_peer_process_does_not_wait_for_active_writer_lifetime() {
    for mode in MODES {
        for role in ["held-clean", "held-dirty", "held-prepared"] {
            let d = Directory::new();
            seed(&d.0, mode);
            let mut writer = Worker::spawn(&d.0, role, None);
            writer.expect("HELD");
            let mut reader = Worker::spawn(&d.0, "reader", None);
            reader.expect("CAPTURED sequence=1 value=A");
            writer.release();
            writer.expect("COMMITTED");
            writer.release();
            writer.expect("RECLAIMED");
            reader.release();
            reader.expect("STABLE");
            reader.release();
            reader.done();
            writer.release();
            writer.done();
        }
    }
}
#[test]
fn previous_rollback_and_physical_maintenance_selectors_share_publication_serialization() {
    for mode in MODES {
        for role in ["previous-writer", "rollback", "compact"] {
            let d = Directory::new();
            seed(&d.0, mode);
            let gate = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(d.0.join(".ulmdb-coordination/history"))
                .unwrap();
            gate.lock_shared().unwrap();
            let mut worker = Worker::spawn(&d.0, role, None);
            worker.expect("PREPARED");
            worker.release();
            worker.waiter(&d.0);
            gate.unlock().unwrap();
            worker.expect("COMMITTED");
            worker.release();
            worker.done();
            let env = Environment::open(&d.0).unwrap();
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(if role == "previous-writer" {
                    b"C".as_slice()
                } else if role == "rollback" {
                    b"A".as_slice()
                } else {
                    b"B".as_slice()
                })
            );
        }
    }
}
#[test]
fn bootstrap_token_also_waits_for_history_capture_and_publishes_a_complete_genesis() {
    for mode in MODES {
        let d = Directory::new();
        ulmdb_storage::create_layout(&d.0).unwrap();
        drop(
            ulmdb_storage::coordination::Coordination::capture_directory(&d.0, Default::default())
                .unwrap(),
        );
        fs::write(
            d.0.join("format-test-control"),
            match mode {
                PublicationMode::Legacy => "2",
                PublicationMode::Cohort => "3",
                _ => "4",
            },
        )
        .unwrap();
        let gate = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(d.0.join(".ulmdb-coordination/history"))
            .unwrap();
        gate.lock_shared().unwrap();
        let mut worker = Worker::spawn(&d.0, "bootstrap", None);
        worker.expect("PREPARED");
        worker.release();
        worker.waiter(&d.0);
        gate.unlock().unwrap();
        worker.expect("COMMITTED");
        worker.release();
        worker.done();
        fs::remove_file(d.0.join("format-test-control")).unwrap();
        let env = Environment::open(&d.0).unwrap();
        assert_eq!(env.begin_read().sequence(), 0);
    }
}

#[test]
fn history_path_replacement_after_a_real_kernel_wait_is_rejected_without_selection() {
    for mode in MODES {
        for writer in [false, true] {
            let d = Directory::new();
            seed(&d.0, mode);
            let head = fs::read(d.0.join("CURRENT")).unwrap();
            let path = d.0.join(".ulmdb-coordination/history");
            let gate = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            if writer {
                gate.lock_shared().unwrap()
            } else {
                gate.lock().unwrap()
            };
            let mut worker = Worker::spawn(
                &d.0,
                if writer {
                    "writer-error"
                } else {
                    "reader-error"
                },
                None,
            );
            if writer {
                worker.expect("PREPARED");
                worker.release();
            }
            worker.waiter(&d.0);
            fs::rename(&path, path.with_extension("retired-test")).unwrap();
            drop(
                fs::OpenOptions::new()
                    .create_new(true)
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap(),
            );
            gate.unlock().unwrap();
            worker.expect("REJECTED");
            assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
            worker.release();
            worker.done();
            assert_eq!(fs::metadata(&path).unwrap().len(), 0);
        }
    }
}
